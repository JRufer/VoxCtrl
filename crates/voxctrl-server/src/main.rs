//! `voxctrl-server` — the headless half of VoxCtrl's client-server setup.
//!
//! It loads one speech model and serves it over an OpenAI-compatible HTTP API,
//! so a VoxCtrl client (or anything else that speaks that API) can dictate
//! without holding a model in its own memory. No window, no tray, no audio
//! device: it runs on a machine nobody is sitting at. See `docs/server.md`.

mod audio;
mod cleanup;
#[cfg(test)]
mod client_contract_tests;
mod config;
mod engine;
mod http;
mod speech;
mod tls;
mod voices;

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
        --pairing-code    Print the code a VoxCtrl app pastes to connect to
                          this server (address, API key and, with encryption
                          on, the certificate fingerprint) and exit
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
    VOXCTRL_SERVER_TLS               true = encrypt connections (HTTPS only);
                                     clients connect with the pairing code
    VOXCTRL_SERVER_PUBLIC_HOST       Name or IP address clients reach this
                                     machine by, for the pairing code
    VOXCTRL_SERVER_CLEANUP           true = clean up dictation for clients
                                     (downloads the S1-mini model, ~470 MB)
    VOXCTRL_SERVER_CLEANUP_GPU       false = keep the cleanup model on the CPU
    VOXCTRL_SERVER_TTS               true = speak for clients (text-to-speech)
    VOXCTRL_SERVER_TTS_ENGINE        piper, espeak, pocket_tts, breeze_tts_2,
                                     vox_cpm_2 or kitten_tts_2
    VOXCTRL_SERVER_TTS_ENGINES       Several engines for clients to choose
                                     between, comma-separated; first = default
    VOXCTRL_SERVER_TTS_VOICE         Default voice for the default engine

Full guide: docs/server.md
";

#[derive(Debug, Default, PartialEq)]
struct Cli {
    config: Option<PathBuf>,
    bind: Option<String>,
    print_config: bool,
    healthcheck: bool,
    pairing_code: bool,
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
            "--pairing-code" => cli.pairing_code = true,
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

    // Beside the config file, so it is kept wherever the API key is kept.
    let identity_dir = path.parent().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));

    if cli.healthcheck {
        let fingerprint = match effective.tls.enabled {
            true => Some(tls::Identity::load_or_create(&identity_dir)?.0.fingerprint()),
            false => None,
        };
        return healthcheck(&effective.bind, fingerprint.as_deref());
    }

    if cli.pairing_code {
        if effective.require_api_key && effective.api_key.is_empty() {
            bail!(
                "This server has no API key yet: one is made the first time it starts. \
                 Start the server once, then ask for the pairing code again."
            );
        }
        let fingerprint = match effective.tls.enabled {
            true => Some(tls::Identity::load_or_create(&identity_dir)?.0.fingerprint()),
            false => None,
        };
        println!("{}", pairing_code(&effective, fingerprint).encode());
        return Ok(());
    }

    if cli.print_config {
        let mut shown = effective.clone();
        if !shown.api_key.is_empty() {
            shown.api_key = "(set)".into();
        }
        // The Hugging Face token is a secret too.
        if shown.tts.hf_token.as_deref().is_some_and(|t| !t.is_empty()) {
            shown.tts.hf_token = Some("(set)".into());
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

    let cleaner: Option<Arc<dyn cleanup::Cleaner>> = if effective.cleanup.enabled {
        Some(Arc::new(cleanup::S1MiniCleaner::prepare(effective.cleanup.gpu).await?))
    } else {
        None
    };
    let cleanup_line = match &cleaner {
        Some(c) => format!("on ({})", c.model()),
        None => "off (set cleanup.enabled or VOXCTRL_SERVER_CLEANUP to offer it)".to_string(),
    };

    let speaker: Option<Arc<dyn speech::Speaker>> = if effective.tts.enabled {
        Some(Arc::new(speech::EngineSpeaker::prepare(effective.tts.clone(), &effective.speech.engines).await?))
    } else {
        None
    };
    let speech_line = match &speaker {
        Some(s) => {
            let engines: Vec<String> = s.engines().into_iter().map(|e| e.id).collect();
            format!("on ({}), clients may upload voices", engines.join(", "))
        }
        None => "off (set tts.enabled or VOXCTRL_SERVER_TTS to offer it)".to_string(),
    };

    let state = Arc::new(http::AppState {
        engine,
        cleanup: cleaner,
        voices: speaker.is_some().then(|| voices::VoiceStore::new(voices::VoiceStore::default_root())),
        speech: speaker,
        api_key: effective.require_api_key.then(|| effective.api_key.clone()),
        max_upload_bytes: usize::try_from(effective.max_upload_mb.saturating_mul(1024 * 1024)).unwrap_or(usize::MAX),
    });
    let app = http::router(state);

    let identity = match effective.tls.enabled {
        true => {
            let (identity, made) = tls::Identity::load_or_create(&identity_dir)?;
            if made {
                info!("Made a certificate for this server and saved it in {}", identity_dir.display());
            }
            Some(identity)
        }
        false => None,
    };

    let listener = std::net::TcpListener::bind(&effective.bind)
        .with_context(|| format!("Could not listen on {}", effective.bind))?;
    listener.set_nonblocking(true).context("Could not prepare the listening socket")?;
    let addr = listener.local_addr().unwrap_or_else(|_| effective.bind.parse().expect("validated above"));
    let scheme = if identity.is_some() { "https" } else { "http" };

    info!("voxctrl-server {} is ready", env!("CARGO_PKG_VERSION"));
    info!("  Model:    {model_id}");
    info!("  Cleanup:  {cleanup_line}");
    info!("  Speech:   {speech_line}");
    info!("  Endpoint: {scheme}://{addr}/v1  (use this machine's address from a client)");
    match &identity {
        Some(identity) => info!(
            "  Encryption: on. Certificate fingerprint {}",
            voxctrl_pairing::display_fingerprint(&identity.fingerprint())
        ),
        None => info!("  Encryption: off (set tls.enabled or VOXCTRL_SERVER_TLS to turn it on)"),
    }
    if generated_key {
        // The one time the key is shown: nobody has seen it yet, and on a
        // container the log is the only place to read it.
        info!("  API key:  {}", effective.api_key);
        info!(
            "  Pairing code (paste it into the app instead of the address and key): {}",
            pairing_code(&effective, identity.as_ref().map(tls::Identity::fingerprint)).encode()
        );
    } else if effective.require_api_key {
        info!("  API key:  required (set in {} or VOXCTRL_SERVER_API_KEY)", path.display());
        // The code holds the key, so it is not written to the log on every
        // start.
        info!("  Pairing code: run `voxctrl-server --pairing-code` to print it");
    }

    match identity {
        Some(identity) => {
            let handle = axum_server::Handle::new();
            let stop = handle.clone();
            tokio::spawn(async move {
                shutdown_signal().await;
                stop.graceful_shutdown(Some(std::time::Duration::from_secs(10)));
            });
            let tls = axum_server::tls_rustls::RustlsConfig::from_config(Arc::new(identity.server_config()?));
            axum_server::from_tcp_rustls(listener, tls)
                .handle(handle)
                .serve(app.into_make_service())
                .await
                .context("The HTTPS server stopped with an error")?;
        }
        None => {
            let listener =
                tokio::net::TcpListener::from_std(listener).context("Could not prepare the listening socket")?;
            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown_signal())
                .await
                .context("The HTTP server stopped with an error")?;
        }
    }
    info!("voxctrl-server stopped");
    Ok(())
}

/// `--healthcheck`: ask the server listening on `bind` for `/health`.
///
/// Built in so the container image needs no curl or wget just to answer
/// Docker's health check. Plain blocking sockets: it runs for a moment in its
/// own short-lived process.
///
/// With encryption on, `fingerprint` is this server's own certificate: the
/// check speaks TLS and accepts that certificate only, as a client would.
fn healthcheck(bind: &str, fingerprint: Option<&str>) -> Result<()> {
    use std::io::{Read, Write};
    use std::time::Duration;

    const REQUEST: &[u8] = b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    let target = health_target(bind)?;
    let timeout = Duration::from_secs(5);
    let mut stream = std::net::TcpStream::connect_timeout(&target, timeout)
        .with_context(|| format!("No server is answering on {target}"))?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;

    let mut response = String::new();
    // A reset after the status line has arrived still tells us the status.
    match fingerprint {
        Some(fingerprint) => {
            let config = Arc::new(voxctrl_pairing::client_tls(fingerprint)?);
            let name = rustls::pki_types::ServerName::try_from("localhost").expect("a valid name");
            let session = rustls::ClientConnection::new(config, name).context("Could not set up encryption")?;
            let mut tls = rustls::StreamOwned::new(session, stream);
            tls.write_all(REQUEST).with_context(|| format!("The server on {target} did not accept an encrypted connection"))?;
            let _ = tls.read_to_string(&mut response);
        }
        None => {
            stream.write_all(REQUEST)?;
            let _ = stream.read_to_string(&mut response);
        }
    }
    if is_healthy_response(&response) {
        println!("ok");
        Ok(())
    } else {
        bail!("The server on {target} did not report healthy: {}", response.lines().next().unwrap_or("no response"))
    }
}

/// The code a client pastes to connect: where the server is, the key, and the
/// certificate fingerprint when it encrypts.
fn pairing_code(config: &ServerConfig, fingerprint: Option<String>) -> voxctrl_pairing::PairingCode {
    let bound: Option<std::net::SocketAddr> = config.bind.parse().ok();
    let host = match config.public_host.trim() {
        "" => match bound {
            // Bound to one particular address: that is the one to use.
            Some(addr) if !addr.ip().is_unspecified() && !addr.ip().is_loopback() => Some(addr.ip().to_string()),
            _ => tls::guess_public_host(),
        },
        host => Some(host.to_string()),
    };
    voxctrl_pairing::PairingCode {
        host,
        port: bound.map_or(8787, |addr| addr.port()),
        api_key: config.require_api_key.then(|| config.api_key.clone()),
        fingerprint,
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
            pairing_code: false,
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
    fn the_pairing_code_carries_what_a_client_needs() {
        let mut config = ServerConfig { api_key: "vx_key".into(), public_host: "speechbox.lan".into(), ..Default::default() };
        config.bind = "0.0.0.0:9000".into();
        let encrypted = pairing_code(&config, Some("ab".repeat(32)));
        assert_eq!(encrypted.endpoint("").unwrap(), "https://speechbox.lan:9000/v1");
        assert_eq!(encrypted.api_key.as_deref(), Some("vx_key"));
        assert_eq!(voxctrl_pairing::PairingCode::decode(&encrypted.encode()).unwrap(), encrypted);

        // Without encryption the code is still a shortcut for address and key.
        assert_eq!(pairing_code(&config, None).endpoint("").unwrap(), "http://speechbox.lan:9000/v1");

        // Bound to one address: that address is the one clients use.
        config.public_host.clear();
        config.bind = "192.168.1.50:8787".into();
        assert_eq!(pairing_code(&config, None).host.as_deref(), Some("192.168.1.50"));

        // No key required, none handed out.
        config.require_api_key = false;
        assert_eq!(pairing_code(&config, None).api_key, None);
    }

    #[test]
    fn the_pairing_code_flag_is_understood() {
        let Invocation::Run(cli) = parse_args(["--pairing-code".to_string()]).unwrap() else { panic!("expected a run") };
        assert!(cli.pairing_code);
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
        let state = Arc::new(http::AppState {
            engine,
            cleanup: None,
            speech: None,
            voices: None,
            api_key: Some("vx_key".into()),
            max_upload_bytes: 1024,
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, http::router(state)).await });

        let bind = addr.to_string();
        let up = tokio::task::spawn_blocking(move || healthcheck(&bind, None)).await.unwrap();
        assert!(up.is_ok(), "{up:?}");

        server.abort();
        let _ = server.await;
        let bind = addr.to_string();
        let down = tokio::task::spawn_blocking(move || healthcheck(&bind, None)).await.unwrap();
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
            "VOXCTRL_SERVER_CLEANUP",
            "VOXCTRL_SERVER_CLEANUP_GPU",
            "VOXCTRL_SERVER_TTS",
            "VOXCTRL_SERVER_TTS_ENGINE",
            "VOXCTRL_SERVER_TTS_ENGINES",
            "VOXCTRL_SERVER_TTS_VOICE",
        ] {
            assert!(HELP.contains(name), "--help does not mention {name}");
        }
    }
}
