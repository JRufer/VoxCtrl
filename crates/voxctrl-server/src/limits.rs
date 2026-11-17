//! How long the server waits on a client that has stopped sending.
//!
//! Without these, a connection that opens and never sends a request, or a
//! request whose body stops part way, is held open for good: a client that
//! crashed mid-upload, or anyone on the network opening connections and
//! leaving them. Each one costs a task and a socket, and enough of them use up
//! the process's file descriptors.
//!
//! What a legitimate client does fits easily inside both limits. A dictation
//! is streamed while it is spoken, but the app sends audio continuously for as
//! long as the key is held (silence included), so the body never pauses for
//! long; and a request's headers arrive in one go.
//!
//! There is no cap on the number of connections. The server is meant for a
//! home network or a VPN such as Tailscale (see "Security" in
//! docs/server.md); a cap would mostly turn away the user's own devices.

use std::time::Duration;

use axum::Router;

/// From a connection opening, or from the end of the previous request on a
/// kept-alive one, to the end of the next request's headers. Also how long an
/// idle kept-alive connection is kept.
pub const HEADER_READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Longest pause between two pieces of a request body.
pub const BODY_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

/// `router` with the body idle timeout applied to every route.
pub fn with_body_timeout(router: Router) -> Router {
    body_timeout_of(router, BODY_IDLE_TIMEOUT)
}

fn body_timeout_of(router: Router, timeout: Duration) -> Router {
    router.layer(tower_http::timeout::RequestBodyTimeoutLayer::new(timeout))
}

/// Give `server`'s HTTP/1 connections a timer, which is what turns on hyper's
/// header read timeout (30 s by default, but only once a timer is set; neither
/// `axum::serve` nor `axum_server` sets one).
pub fn apply<A>(server: axum_server::Server<A>) -> axum_server::Server<A> {
    header_timeout_of(server, HEADER_READ_TIMEOUT)
}

fn header_timeout_of<A>(mut server: axum_server::Server<A>, timeout: Duration) -> axum_server::Server<A> {
    server.http_builder().http1().timer(hyper_util::rt::TokioTimer::new()).header_read_timeout(timeout);
    server
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::post;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const SHORT: Duration = Duration::from_millis(300);

    /// A server whose limits are [`SHORT`], answering POST / with the body's
    /// length; returns its address.
    async fn serve() -> std::net::SocketAddr {
        let router = body_timeout_of(
            Router::new().route(
                "/",
                post(|body: axum::body::Bytes| async move { body.len().to_string() }),
            ),
            SHORT,
        );
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let server = header_timeout_of(axum_server::from_tcp(listener), SHORT);
        tokio::spawn(async move { server.serve(router.into_make_service()).await });
        addr
    }

    /// Read until the server closes the connection, or give up after `limit`.
    async fn read_until_closed(stream: &mut tokio::net::TcpStream, limit: Duration) -> Option<String> {
        let mut seen = Vec::new();
        let read = tokio::time::timeout(limit, stream.read_to_end(&mut seen)).await;
        read.ok().map(|_| String::from_utf8_lossy(&seen).into_owned())
    }

    #[tokio::test]
    async fn a_connection_that_never_sends_a_request_is_closed() {
        let addr = serve().await;
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        // Half a request line, then nothing.
        stream.write_all(b"POST / HT").await.unwrap();
        assert!(read_until_closed(&mut stream, Duration::from_secs(5)).await.is_some(), "the server kept it open");
    }

    #[tokio::test]
    async fn a_body_that_stops_part_way_is_cut_off() {
        let addr = serve().await;
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(b"POST / HTTP/1.1\r\nHost: x\r\nContent-Length: 100\r\n\r\nonly ten b")
            .await
            .unwrap();
        let reply = read_until_closed(&mut stream, Duration::from_secs(5)).await.expect("the server kept it open");
        assert!(!reply.contains("\r\n\r\n100"), "the stalled body was accepted: {reply}");
    }

    #[tokio::test]
    async fn a_body_that_keeps_coming_is_not_cut_off() {
        let addr = serve().await;
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        stream.write_all(b"POST / HTTP/1.1\r\nHost: x\r\nContent-Length: 8\r\nConnection: close\r\n\r\n").await.unwrap();
        // Longer in all than the limit, but no pause as long as it.
        for _ in 0..8 {
            tokio::time::sleep(SHORT / 3).await;
            stream.write_all(b"a").await.unwrap();
        }
        let reply = read_until_closed(&mut stream, Duration::from_secs(5)).await.unwrap();
        assert!(reply.starts_with("HTTP/1.1 200"), "{reply}");
        assert!(reply.ends_with("\r\n\r\n8"), "{reply}");
    }
}
