//! Encryption for the server: a certificate it makes for itself on first
//! start, and the pairing code that carries its fingerprint to clients.
//!
//! No certificate authority is involved. A client trusts this server by
//! remembering the fingerprint of this one certificate (see `voxctrl-pairing`),
//! so the certificate must stay the same across restarts: it is kept beside
//! `server.toml`, and replacing it means pairing every client again.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};

const CERT_FILE: &str = "server-cert.pem";
const KEY_FILE: &str = "server-key.pem";

/// The server's certificate and the private key that goes with it.
pub struct Identity {
    cert_pem: String,
    key_pem: String,
    certificate: CertificateDer<'static>,
}

impl Identity {
    /// Read the identity kept in `dir`, making one first if there is none.
    /// The `bool` says whether it was just made.
    pub fn load_or_create(dir: &Path) -> Result<(Self, bool)> {
        let (cert_path, key_path) = paths(dir);
        if cert_path.exists() && key_path.exists() {
            let cert_pem = std::fs::read_to_string(&cert_path)
                .with_context(|| format!("Could not read {}", cert_path.display()))?;
            let key_pem =
                std::fs::read_to_string(&key_path).with_context(|| format!("Could not read {}", key_path.display()))?;
            return Ok((
                Self::from_pem(cert_pem, key_pem).with_context(|| format!("In {}", cert_path.display()))?,
                false,
            ));
        }

        let made = rcgen::generate_simple_self_signed(vec!["voxctrl-server".to_string(), "localhost".to_string()])
            .context("Could not make a certificate")?;
        let identity = Self::from_pem(made.cert.pem(), made.key_pair.serialize_pem())?;
        std::fs::create_dir_all(dir).with_context(|| format!("Could not create {}", dir.display()))?;
        // The key first: a certificate without its key would be read as an
        // identity on the next start and fail.
        crate::config::write_private(&key_path, &identity.key_pem)
            .with_context(|| format!("Could not write {}", key_path.display()))?;
        crate::config::write_private(&cert_path, &identity.cert_pem)
            .with_context(|| format!("Could not write {}", cert_path.display()))?;
        Ok((identity, true))
    }

    fn from_pem(cert_pem: String, key_pem: String) -> Result<Self> {
        let certificate = rustls_pemfile::certs(&mut cert_pem.as_bytes())
            .next()
            .context("the certificate file holds no certificate")?
            .context("the certificate file is damaged")?;
        // Parsed here so a damaged key is reported at start, not at the first
        // connection.
        private_key(&key_pem)?;
        Ok(Self { cert_pem, key_pem, certificate })
    }

    /// What clients pin: SHA-256 of the certificate, in hex.
    pub fn fingerprint(&self) -> String {
        voxctrl_pairing::fingerprint(self.certificate.as_ref())
    }

    pub fn server_config(&self) -> Result<rustls::ServerConfig> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut config = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .context("Could not set up encryption")?
            .with_no_client_auth()
            .with_single_cert(vec![self.certificate.clone()], private_key(&self.key_pem)?)
            .context("The certificate and its key do not belong together")?;
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        Ok(config)
    }
}

fn private_key(pem: &str) -> Result<PrivateKeyDer<'static>> {
    rustls_pemfile::private_key(&mut pem.as_bytes())
        .context("the key file is damaged")?
        .context("the key file holds no private key")
}

fn paths(dir: &Path) -> (PathBuf, PathBuf) {
    (dir.join(CERT_FILE), dir.join(KEY_FILE))
}

/// The address clients on the network use for this machine, when it can be
/// told. Inside a container the only address visible is the container's own,
/// which no client can reach, so there the answer is `None` and the client
/// keeps the address its user typed.
pub fn guess_public_host() -> Option<String> {
    if Path::new("/.dockerenv").exists() || Path::new("/run/.containerenv").exists() {
        return None;
    }
    // Nothing is sent: connecting a UDP socket only asks the system which of
    // its addresses it would use to reach the wider network.
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then(|| ip.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_identity_is_made_once_and_kept() {
        let dir = tempfile::tempdir().unwrap();
        let (first, made) = Identity::load_or_create(dir.path()).unwrap();
        assert!(made);
        assert!(voxctrl_pairing::valid_fingerprint(&first.fingerprint()));
        let (again, made) = Identity::load_or_create(dir.path()).unwrap();
        assert!(!made);
        assert_eq!(first.fingerprint(), again.fingerprint());
        assert!(first.server_config().is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn the_private_key_is_readable_by_its_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        Identity::load_or_create(dir.path()).unwrap();
        let mode = std::fs::metadata(dir.path().join(KEY_FILE)).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0, "mode {mode:o}");
    }

    #[test]
    fn a_damaged_certificate_is_reported_rather_than_replaced() {
        let dir = tempfile::tempdir().unwrap();
        Identity::load_or_create(dir.path()).unwrap();
        std::fs::write(dir.path().join(CERT_FILE), "not a certificate").unwrap();
        let error = format!("{:#}", Identity::load_or_create(dir.path()).err().unwrap());
        assert!(error.contains("holds no certificate"), "{error}");
    }
}
