//! `voxctrl-server` — the headless half of VoxCtrl's client-server setup.
//!
//! It loads one speech model and serves it over an OpenAI-compatible HTTP API,
//! so a VoxCtrl client (or anything else that speaks that API) can dictate
//! without holding a model in its own memory. No window, no tray, no audio
//! device: it runs on a machine nobody is sitting at. See `docs/server.md`.

mod audio;
#[cfg(test)]
mod client_contract_tests;
mod config;
mod engine;
mod http;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use tracing::{info, warn};

use config::ServerConfig;

const HELP: &str = "\
voxctrl-server — run VoxCtrl's speech models for clients on your network

USAGE:
    voxctrl-server [OPTIONS]

OPTIONS:
    -c, --config <PATH>   Config file to use
                          [default: ~/.config/voxctrl/server.toml]
    -b, --bind <ADDR>     Address and port to listen on, e.g. 0.0.0.0:8787
                          (overrides the config file and VOXCTRL_SERVER_BIND)
        --print-config    Print the settings the server would start with
                          (API key hidden) and exit
        --healthcheck     Ask the server already running with these settings
                          whether it is up; exits 0 if so, 1 if not. This is
                          what the container's health check runs
    -h, --help            Print this help
    -V, --version         Print the version

ENVIRONMENT (each overrides the config file):
    VOXCTRL_SERVER_CONFIG            Config file path
    VOXCTRL_SERVER_BIND              Address and port to listen on
    VOXCTRL_SERVER_API_KEY           Key clients must present
    VOXCTRL_SERVER_REQUIRE_API_KEY   false = accept requests with no key
    VOXCTRL_SERVER_BACKEND           whisper-cpp | moonshine | parakeet
    VOXCTRL_SERVER_MODEL             Model for that backend, e.g. large-v3-turbo
    VOXCTRL_SERVER_DEVICE            auto | cpu
    VOXCTRL_SERVER_LANGUAGE          Language code, or auto
    VOXCTRL_SERVER_MAX_UPLOAD_MB     Largest upload accepted

Full guide: docs/server.md
";

#[derive(Debug, Default, PartialEq)]
struct Cli {
    config: Option<PathBuf>,
    bind: Option<String>,
    print_config: bool,
    healthcheck: bool,
}

/// What the command line asked for.
#[derive(Debug, PartialEq)]
enum Invocation {
    Run(Cli),
    Help,
    Version,
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Invocation> {
    let mut cli = Cli::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        // Accept both `--flag value` and `--flag=value`.
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f.to_string(), Some(v.to_string())),
            _ => (arg.clone(), None),
        };
        let mut value = |name: &str| -> Result<String> {
            inline
                .clone()
                .or_else(|| args.next())
                .with_context(|| format!("{name} needs a value. Run with --help for usage."))
        };
        match flag.as_str() {
            "-h" | "--help" => return Ok(Invocation::Help),
            "-V" | "--version" => return Ok(Invocation::Version),
            "-c" | "--config" => cli.config = Some(PathBuf::from(value("--config")?)),
            "-b" | "--bind" => cli.bind = Some(value("--bind")?),
            "--print-config" => cli.print_config = true,
            "--healthcheck" => cli.healthcheck = true,
            other => bail!("Unknown option '{other}'. Run with --help for usage."),
        }
    }
    Ok(Invocation::Run(cli))
}

fn main() {
    let cli = match parse_args(std::env::args().skip(1)) {
        Ok(Invocation::Help) => {
            print!("{HELP}");
            return;
        }
        Ok(Invocation::Version) => {
            println!("voxctrl-server {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        Ok(Invocation::Run(cli)) => cli,
        Err(e) => {
            eprintln!("voxctrl-server: {e:#}");
            std::process::exit(2);
        }
    };

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        // Colour codes only for a terminal: in `journalctl` and
        // `docker logs` they would show up as escape sequences.
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()))
        .init();

    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("voxctrl-server: could not start the async runtime: {e}");
            std::process::exit(1);
        }
    };
    if let Err(e) = runtime.block_on(run(cli)) {
        // `{:#}` prints the whole cause chain on one line.
        eprintln!("voxctrl-server: {e:#}");
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> Result<()> {
    let env = |name: &str| std::env::var(name).ok();
    let path = cli
        .config
        .clone()
        .or_else(|| env("VOXCTRL_SERVER_CONFIG").filter(|v| !v.trim().is_empty()).map(PathBuf::from))
        .unwrap_or_else(ServerConfig::default_path);

    // The file's own contents are kept apart from the effective settings, so
    // that saving a generated key never writes an environment override into it.
    let mut file_config = ServerConfig::load(&path)?;
    let mut effective = file_config.clone();
    effective.apply_env(env)?;
    if let Some(bind) = &cli.bind {
        effective.bind = bind.clone();
    }
    effective.validate()?;

    if cli.healthcheck {
        return healthcheck(&effective.bind);
    }

    if cli.print_config {
        let mut shown = effective.clone();
        if !shown.api_key.is_empty() {
            shown.api_key = "(set)".into();
        }
        println!("# Effective settings: {} plus environment and flags", path.display());
        print!("{}", toml::to_string_pretty(&shown).context("Could not print the config")?);
        return Ok(());
    }

    let mut generated_key = false;
    if effective.require_api_key && effective.api_key.is_empty() {
        let key = config::generate_api_key()?;
        file_config.api_key = key.clone();
        effective.api_key = key;
        generated_key = true;
        match file_config.save(&path) {
            Ok(()) => info!("Generated an API key and saved it to {}", path.display()),
            Err(e) => warn!(
                "Generated an API key but could not save it ({e:#}). It will change on every \
                 restart: set VOXCTRL_SERVER_API_KEY, or make {} writable.",
                path.display()
            ),
        }
    }
    if !effective.require_api_key {
        warn!(
            "require_api_key is off: anyone who can reach {} can send audio to this server",
            effective.bind
        );
    }

    let engine = engine::Engine::prepare(&effective).await?;
    let model_id = engine.info().model_id();

    let state = Arc::new(http::AppState {
        engine,
        api_key: effective.require_api_key.then(|| effective.api_key.clone()),
        max_upload_bytes: usize::try_from(effective.max_upload_mb.saturating_mul(1024 * 1024)).unwrap_or(usize::MAX),
    });
    let app = http::router(state);

    let listener = tokio::net::TcpListener::bind(&effective.bind)
        .await
        .with_context(|| format!("Could not listen on {}", effective.bind))?;
    let addr = listener.local_addr().unwrap_or_else(|_| effective.bind.parse().expect("validated above"));

    info!("voxctrl-server {} is ready", env!("CARGO_PKG_VERSION"));
    info!("  Model:    {model_id}");
    info!("  Endpoint: http://{addr}/v1  (use this machine's address from a client)");
    if generated_key {
        // The one time the key is shown: nobody has seen it yet, and on a
        // container the log is the only place to read it.
        info!("  API key:  {}", effective.api_key);
    } else if effective.require_api_key {
        info!("  API key:  required (set in {} or VOXCTRL_SERVER_API_KEY)", path.display());
    }

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("The HTTP server stopped with an error")?;
    info!("voxctrl-server stopped");
    Ok(())
}

/// `--healthcheck`: ask the server listening on `bind` for `/health`.
///
/// Built in so the container image needs no curl or wget just to answer
/// Docker's health check. Plain blocking sockets: it runs for a moment in its
/// own short-lived process.
fn healthcheck(bind: &str) -> Result<()> {
    use std::io::{Read, Write};
    use std::time::Duration;

    let target = health_target(bind)?;
    let timeout = Duration::from_secs(5);
    let mut stream = std::net::TcpStream::connect_timeout(&target, timeout)
        .with_context(|| format!("No server is answering on {target}"))?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    stream.write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;

    let mut response = String::new();
    // A reset after the status line has arrived still tells us the status.
    let _ = stream.read_to_string(&mut response);
    if is_healthy_response(&response) {
        println!("ok");
        Ok(())
    } else {
        bail!("The server on {target} did not report healthy: {}", response.lines().next().unwrap_or("no response"))
    }
}

/// Where to reach a server bound to `bind` from the same machine: a wildcard
/// address is not connectable, so it becomes loopback.
fn health_target(bind: &str) -> Result<std::net::SocketAddr> {
    let mut addr: std::net::SocketAddr =
        bind.parse().with_context(|| format!("bind is not an address and port: '{bind}'"))?;
    if addr.ip().is_unspecified() {
        addr.set_ip(match addr {
            std::net::SocketAddr::V4(_) => std::net::Ipv4Addr::LOCALHOST.into(),
            std::net::SocketAddr::V6(_) => std::net::Ipv6Addr::LOCALHOST.into(),
        });
    }
    Ok(addr)
}

fn is_healthy_response(response: &str) -> bool {
    let status_ok = response
        .lines()
        .next()
        .is_some_and(|line| line.starts_with("HTTP/1.") && line.split_whitespace().nth(1) == Some("200"));
    status_ok && response.contains("\"status\":\"ok\"")
}

/// Resolves on Ctrl-C or, on Unix, SIGTERM — what `docker stop` and
/// `systemctl stop` send.
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
    info!("Shutting down");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn no_arguments_runs_with_defaults() {
        assert_eq!(parse_args(args(&[])).unwrap(), Invocation::Run(Cli::default()));
    }

    #[test]
    fn flags_take_values_separately_or_inline() {
        let expected = Invocation::Run(Cli {
            config: Some(PathBuf::from("/etc/voxctrl/server.toml")),
            bind: Some("127.0.0.1:9000".into()),
            print_config: true,
            healthcheck: false,
        });
        assert_eq!(
            parse_args(args(&["--config", "/etc/voxctrl/server.toml", "-b", "127.0.0.1:9000", "--print-config"])).unwrap(),
            expected
        );
        assert_eq!(
            parse_args(args(&["--config=/etc/voxctrl/server.toml", "--bind=127.0.0.1:9000", "--print-config"])).unwrap(),
            expected
        );
    }

    #[test]
    fn help_and_version_win_over_other_flags() {
        assert_eq!(parse_args(args(&["--bind", "x", "--help"])).unwrap(), Invocation::Help);
        assert_eq!(parse_args(args(&["-V"])).unwrap(), Invocation::Version);
    }

    #[test]
    fn mistakes_are_reported() {
        assert!(parse_args(args(&["--port", "1"])).unwrap_err().to_string().contains("--port"));
        assert!(parse_args(args(&["--config"])).unwrap_err().to_string().contains("needs a value"));
    }

    #[test]
    fn healthcheck_targets_loopback_for_a_wildcard_bind() {
        assert_eq!(health_target("0.0.0.0:8787").unwrap().to_string(), "127.0.0.1:8787");
        assert_eq!(health_target("[::]:8787").unwrap().to_string(), "[::1]:8787");
        assert_eq!(health_target("192.168.1.20:9000").unwrap().to_string(), "192.168.1.20:9000");
        assert!(health_target("nonsense").is_err());
    }

    #[test]
    fn healthy_means_a_200_with_status_ok() {
        assert!(is_healthy_response("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\r\n{\"model\":\"x\",\"status\":\"ok\",\"version\":\"0.9.0\"}"));
        assert!(!is_healthy_response("HTTP/1.1 503 Service Unavailable\r\n\r\n{\"status\":\"ok\"}"));
        assert!(!is_healthy_response("HTTP/1.1 200 OK\r\n\r\n<html>some other service</html>"));
        assert!(!is_healthy_response(""));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn healthcheck_passes_against_a_running_server_and_fails_without_one() {
        let (engine, _) = engine::test_support::fake_engine("x", false);
        let state = Arc::new(http::AppState { engine, api_key: Some("vx_key".into()), max_upload_bytes: 1024 });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, http::router(state)).await });

        let bind = addr.to_string();
        let up = tokio::task::spawn_blocking(move || healthcheck(&bind)).await.unwrap();
        assert!(up.is_ok(), "{up:?}");

        server.abort();
        let _ = server.await;
        let bind = addr.to_string();
        let down = tokio::task::spawn_blocking(move || healthcheck(&bind)).await.unwrap();
        assert!(down.is_err());
    }

    #[test]
    fn help_names_every_environment_variable_the_config_reads() {
        for name in [
            "VOXCTRL_SERVER_CONFIG",
            "VOXCTRL_SERVER_BIND",
            "VOXCTRL_SERVER_API_KEY",
            "VOXCTRL_SERVER_REQUIRE_API_KEY",
            "VOXCTRL_SERVER_BACKEND",
            "VOXCTRL_SERVER_MODEL",
            "VOXCTRL_SERVER_DEVICE",
            "VOXCTRL_SERVER_LANGUAGE",
            "VOXCTRL_SERVER_MAX_UPLOAD_MB",
        ] {
            assert!(HELP.contains(name), "--help does not mention {name}");
        }
    }
}
