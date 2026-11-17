//! An answer for a client that speaks plain HTTP to an encrypted server.
//!
//! With encryption on, the server speaks only TLS on its port. A client set
//! up with a bare address (`http://speechbox:8787`) would otherwise see the
//! connection close with no reply, and report something like "connection
//! closed before message completed", which says nothing about the fix.
//!
//! This acceptor runs before the TLS handshake. It looks at the first byte the
//! client sent without consuming it: every TLS connection starts with a
//! handshake record (`0x16`), every HTTP request with a letter. A letter gets
//! one plain-HTTP reply in the server's usual error envelope (status 426,
//! Upgrade Required), saying to pair the app, and the connection is closed.
//! Anything else is passed on to TLS untouched.

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::time::Duration;

use axum_server::accept::Accept;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

/// The first byte of a TLS handshake record.
const TLS_HANDSHAKE: u8 = 0x16;

/// How long a client may take to send its first byte. TLS's own handshake
/// timeout (10 s in axum-server) only starts after this acceptor.
const FIRST_BYTE_TIMEOUT: Duration = Duration::from_secs(10);

pub const MESSAGE: &str = "This server only accepts encrypted connections. In VoxCtrl, paste the server's \
                           pairing code instead of its address (run `voxctrl-server --pairing-code` on the \
                           server to print it), or use an https:// address with the certificate fingerprint.";

/// The reply sent to a plain-HTTP request, whole: status line, headers, body.
pub fn refusal() -> Vec<u8> {
    let body = serde_json::json!({
        "error": { "message": MESSAGE, "type": "encryption_required" }
    })
    .to_string();
    format!(
        "HTTP/1.1 426 Upgrade Required\r\n\
         Upgrade: TLS/1.2, HTTP/1.1\r\n\
         Connection: close\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

/// Whether a connection whose first byte is `first` is plain HTTP rather than
/// TLS. Request methods are ASCII letters; nothing else is judged plain.
pub fn looks_like_plain_http(first: u8) -> bool {
    first != TLS_HANDSHAKE && first.is_ascii_alphabetic()
}

/// The HTTPS server on `listener`: TLS with `tls`, and the plain-HTTP answer
/// in front of it. The one place this is put together, so the tests serve
/// exactly what a real server does.
pub fn https_server(
    listener: std::net::TcpListener,
    tls: axum_server::tls_rustls::RustlsConfig,
) -> axum_server::Server<axum_server::tls_rustls::RustlsAcceptor<RefusePlainHttp>> {
    let acceptor = axum_server::tls_rustls::RustlsAcceptor::new(tls).acceptor(RefusePlainHttp);
    crate::limits::apply(axum_server::from_tcp(listener).acceptor(acceptor))
}

/// The inner acceptor of the server's `RustlsAcceptor`: it sees the raw TCP
/// stream before TLS does.
#[derive(Clone, Copy, Debug, Default)]
pub struct RefusePlainHttp;

impl<S: Send + 'static> Accept<TcpStream, S> for RefusePlainHttp {
    type Stream = TcpStream;
    type Service = S;
    type Future = Pin<Box<dyn Future<Output = io::Result<(TcpStream, S)>> + Send>>;

    fn accept(&self, mut stream: TcpStream, service: S) -> Self::Future {
        Box::pin(async move {
            let mut first = [0u8; 1];
            let peeked = tokio::time::timeout(FIRST_BYTE_TIMEOUT, stream.peek(&mut first))
                .await
                .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "no data from the client"))??;
            if peeked == 1 && looks_like_plain_http(first[0]) {
                // Read what the client sent so far before answering: closing a
                // socket with unread data makes the kernel send a reset, and
                // the client then reports the reset, not this reply.
                let mut scratch = [0u8; 8192];
                let _ = tokio::time::timeout(Duration::from_millis(200), async {
                    use tokio::io::AsyncReadExt;
                    let _ = stream.read(&mut scratch).await;
                })
                .await;
                let _ = stream.write_all(&refusal()).await;
                let _ = stream.shutdown().await;
                tracing::info!("Refused a plain-HTTP request: this server is encrypted (see the pairing code)");
                return Err(io::Error::new(io::ErrorKind::InvalidData, "plain HTTP on the encrypted port"));
            }
            Ok((stream, service))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tls_is_passed_on_and_http_methods_are_refused() {
        assert!(!looks_like_plain_http(TLS_HANDSHAKE));
        for request in ["GET / HTTP/1.1", "POST /v1/audio/transcriptions HTTP/1.1", "PUT", "DELETE", "HEAD"] {
            assert!(looks_like_plain_http(request.as_bytes()[0]), "{request}");
        }
        // Not a letter: left to TLS, which rejects it as it always has.
        assert!(!looks_like_plain_http(0x00));
        assert!(!looks_like_plain_http(b'{'));
    }

    #[test]
    fn the_refusal_is_a_complete_http_reply_in_the_error_envelope() {
        let reply = String::from_utf8(refusal()).unwrap();
        let (head, body) = reply.split_once("\r\n\r\n").unwrap();
        assert!(head.starts_with("HTTP/1.1 426 "));
        assert!(head.contains(&format!("Content-Length: {}", body.len())));
        let json: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(json["error"]["type"], "encryption_required");
        assert!(json["error"]["message"].as_str().unwrap().contains("pairing code"));
    }
}
