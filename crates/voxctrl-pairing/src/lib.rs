//! Pairing between the VoxCtrl app and a VoxCtrl server.
//!
//! A server with encryption on has a certificate it made for itself, which no
//! certificate authority vouches for. The app trusts it by *pinning*: it
//! remembers the SHA-256 fingerprint of that one certificate and accepts
//! nothing else. The fingerprint, together with the API key and the port,
//! travels to the app in a pairing code the server prints.
//!
//! Pinning replaces the usual checks rather than adding to them: the name in
//! the certificate is not compared with the address, because a server on a
//! home network is reached by whatever IP address it has that day.
//!
//! The pairing-code format and the fingerprint are also read by clients in
//! other repositories (the Android keyboard, JRufer/voxctrl-keys), which
//! reimplement this file. Check a change here against `docs/clients.md`.

use std::sync::Arc;

use anyhow::{bail, Context, Result};
use base64::Engine as _;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// What every pairing code starts with, so one is recognisable when pasted.
pub const PREFIX: &str = "vxpair_";

/// Everything the app needs to reach a server, as the server prints it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairingCode {
    /// The server's address, when it knows one that clients can use. A server
    /// in a container does not: the app then keeps the address already typed.
    #[serde(rename = "h", default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(rename = "p")]
    pub port: u16,
    /// The API key, when the server requires one.
    #[serde(rename = "k", default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// The certificate fingerprint; present exactly when the server encrypts.
    #[serde(rename = "f", default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
}

impl PairingCode {
    pub fn encode(&self) -> String {
        let json = serde_json::to_vec(self).expect("a pairing code is plain data");
        format!("{PREFIX}{}", base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json))
    }

    pub fn decode(text: &str) -> Result<Self> {
        let text = text.trim();
        let Some(body) = text.strip_prefix(PREFIX) else {
            bail!("That is not a pairing code. A pairing code starts with \"{PREFIX}\".");
        };
        let incomplete = "The pairing code is incomplete or damaged. Copy the whole code from the server again.";
        let json = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(body.trim()).ok().context(incomplete)?;
        let code: PairingCode = serde_json::from_slice(&json).ok().context(incomplete)?;
        if code.port == 0 || code.fingerprint.as_deref().is_some_and(|f| !valid_fingerprint(f)) {
            bail!(incomplete);
        }
        Ok(code)
    }

    pub fn encrypted(&self) -> bool {
        self.fingerprint.is_some()
    }

    /// The endpoint for the app's settings. `typed` is the address already in
    /// the endpoint field, used for its host when the code carries none.
    pub fn endpoint(&self, typed: &str) -> Result<String> {
        let host = match self.host.as_deref().filter(|h| !h.is_empty()) {
            Some(host) => host.to_string(),
            None => host_of(typed).context(
                "This pairing code does not say where the server is. Enter the server's name or IP address \
                 as the endpoint first, then paste the code again.",
            )?,
        };
        let scheme = if self.encrypted() { "https" } else { "http" };
        let host = if host.contains(':') && !host.starts_with('[') { format!("[{host}]") } else { host };
        Ok(format!("{scheme}://{host}:{}/v1", self.port))
    }
}

/// The host part of an address as a person types it: with or without a
/// scheme, port or path.
pub fn host_of(address: &str) -> Option<String> {
    let rest = address.trim();
    let rest = rest.split_once("://").map_or(rest, |(_, r)| r);
    let authority = rest.split('/').next().unwrap_or("");
    let host = if let Some(v6) = authority.strip_prefix('[') {
        v6.split(']').next().unwrap_or("")
    } else {
        authority.rsplit_once(':').map_or(authority, |(h, port)| {
            if port.chars().all(|c| c.is_ascii_digit()) {
                h
            } else {
                authority
            }
        })
    };
    (!host.is_empty()).then(|| host.to_string())
}

/// The fingerprint of a certificate: SHA-256 of its DER bytes, in hex.
pub fn fingerprint(certificate_der: &[u8]) -> String {
    hex::encode(Sha256::digest(certificate_der))
}

pub fn valid_fingerprint(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// A fingerprint as shown to a person: `AB:CD:…`, easier to compare by eye.
pub fn display_fingerprint(fingerprint: &str) -> String {
    let upper = fingerprint.to_ascii_uppercase();
    upper.as_bytes().chunks(2).map(|pair| String::from_utf8_lossy(pair).into_owned()).collect::<Vec<_>>().join(":")
}

fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// Accepts the one certificate with this fingerprint and no other.
#[derive(Debug)]
struct PinnedCertificate {
    fingerprint: String,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for PinnedCertificate {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        if fingerprint(end_entity.as_ref()).eq_ignore_ascii_case(&self.fingerprint) {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General(CERTIFICATE_MISMATCH.into()))
        }
    }

    // The handshake signature is still checked, so the server has to hold the
    // private key of the pinned certificate; presenting a copy of the
    // certificate alone is not enough.
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

/// The text of the error a mismatched certificate produces, so callers can
/// recognise it in a connection error and explain it.
pub const CERTIFICATE_MISMATCH: &str = "the server's certificate is not the one this app was paired with";

/// What to tell a person when [`CERTIFICATE_MISMATCH`] turns up.
pub const CERTIFICATE_MISMATCH_HELP: &str = "The server's certificate is not the one this app was paired with. \
     If the server was reinstalled or its certificate replaced, paste its new pairing code. \
     Otherwise something else is answering at this address.";

/// TLS settings that trust only the certificate with this fingerprint.
pub fn client_tls(fingerprint: &str) -> Result<rustls::ClientConfig> {
    if !valid_fingerprint(fingerprint) {
        bail!("The saved certificate fingerprint is not valid. Paste the server's pairing code again.");
    }
    let provider = provider();
    let verifier =
        Arc::new(PinnedCertificate { fingerprint: fingerprint.to_ascii_lowercase(), provider: provider.clone() });
    Ok(rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .context("Could not set up encryption")?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth())
}

/// The fingerprint to pin for `endpoint`, if any. A paired server is never
/// spoken to unencrypted: an `http://` address with a fingerprint saved is an
/// error rather than a silent downgrade.
fn pin_for<'a>(endpoint: &str, fingerprint: Option<&'a str>) -> Result<Option<&'a str>> {
    let Some(fingerprint) = fingerprint.map(str::trim).filter(|f| !f.is_empty()) else {
        return Ok(None);
    };
    if !endpoint.trim().to_ascii_lowercase().starts_with("https://") {
        bail!(
            "This server was paired with encryption on, so its address must start with https://. \
             Paste the pairing code again, or remove the pairing to connect without encryption."
        );
    }
    Ok(Some(fingerprint))
}

/// Make `builder` trust only the paired certificate when one is saved.
pub fn pin(
    builder: reqwest::ClientBuilder,
    endpoint: &str,
    fingerprint: Option<&str>,
) -> Result<reqwest::ClientBuilder> {
    Ok(match pin_for(endpoint, fingerprint)? {
        Some(fingerprint) => builder.use_preconfigured_tls(client_tls(fingerprint)?),
        None => builder,
    })
}

/// [`pin`] for the blocking client.
pub fn pin_blocking(
    builder: reqwest::blocking::ClientBuilder,
    endpoint: &str,
    fingerprint: Option<&str>,
) -> Result<reqwest::blocking::ClientBuilder> {
    Ok(match pin_for(endpoint, fingerprint)? {
        Some(fingerprint) => builder.use_preconfigured_tls(client_tls(fingerprint)?),
        None => builder,
    })
}

/// Whether a connection error came from a certificate that is not the paired
/// one, however deep in the error chain that is.
pub fn is_certificate_mismatch(error: &(dyn std::error::Error + 'static)) -> bool {
    let mut current = Some(error);
    while let Some(e) = current {
        if e.to_string().contains(CERTIFICATE_MISMATCH) {
            return true;
        }
        current = e.source();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code() -> PairingCode {
        PairingCode {
            host: Some("192.168.1.50".into()),
            port: 8787,
            api_key: Some("vx_key".into()),
            fingerprint: Some("ab".repeat(32)),
        }
    }

    #[test]
    fn a_code_survives_the_round_trip() {
        let text = code().encode();
        assert!(text.starts_with(PREFIX));
        assert_eq!(PairingCode::decode(&format!("  {text}\n")).unwrap(), code());
    }

    #[test]
    fn what_is_not_a_code_is_refused_with_a_reason() {
        let not_one = PairingCode::decode("vx_some_api_key").unwrap_err().to_string();
        assert!(not_one.contains("starts with \"vxpair_\""), "{not_one}");
        let text = code().encode();
        let cut = PairingCode::decode(&text[..text.len() - 9]).unwrap_err().to_string();
        assert!(cut.contains("incomplete or damaged"), "{cut}");
        let bad_print = PairingCode { fingerprint: Some("zz".into()), ..code() }.encode();
        assert!(PairingCode::decode(&bad_print).is_err());
    }

    #[test]
    fn the_endpoint_is_https_exactly_when_the_server_encrypts() {
        assert_eq!(code().endpoint("").unwrap(), "https://192.168.1.50:8787/v1");
        let plain = PairingCode { fingerprint: None, ..code() };
        assert_eq!(plain.endpoint("").unwrap(), "http://192.168.1.50:8787/v1");
        let v6 = PairingCode { host: Some("fd00::1".into()), ..code() };
        assert_eq!(v6.endpoint("").unwrap(), "https://[fd00::1]:8787/v1");
    }

    #[test]
    fn a_code_without_an_address_uses_the_one_already_typed() {
        let anywhere = PairingCode { host: None, port: 9000, ..code() };
        assert_eq!(anywhere.endpoint("http://mybox:8787/v1").unwrap(), "https://mybox:9000/v1");
        assert_eq!(anywhere.endpoint("192.168.1.162").unwrap(), "https://192.168.1.162:9000/v1");
        let nowhere = anywhere.endpoint(" ").unwrap_err().to_string();
        assert!(nowhere.contains("Enter the server's name or IP address"), "{nowhere}");
    }

    #[test]
    fn hosts_are_read_out_of_typed_addresses() {
        assert_eq!(host_of("http://box:8787/v1").as_deref(), Some("box"));
        assert_eq!(host_of("box").as_deref(), Some("box"));
        assert_eq!(host_of("https://[fd00::1]:8787/v1").as_deref(), Some("fd00::1"));
        assert_eq!(host_of(""), None);
    }

    #[test]
    fn fingerprints_are_sha256_in_hex() {
        let print = fingerprint(b"certificate");
        assert!(valid_fingerprint(&print));
        assert_eq!(display_fingerprint("ab01").as_str(), "AB:01");
        assert!(!valid_fingerprint("abc"));
    }

    #[test]
    fn a_paired_server_is_never_spoken_to_unencrypted() {
        let print = "ab".repeat(32);
        assert_eq!(pin_for("https://box:8787/v1", Some(&print)).unwrap(), Some(print.as_str()));
        assert_eq!(pin_for("http://box:8787/v1", None).unwrap(), None);
        assert_eq!(pin_for("http://box:8787/v1", Some("  ")).unwrap(), None);
        let downgrade = pin_for("http://box:8787/v1", Some(&print)).unwrap_err().to_string();
        assert!(downgrade.contains("must start with https://"), "{downgrade}");
    }

    #[test]
    fn pinned_settings_build_and_a_bad_fingerprint_does_not() {
        assert!(client_tls(&"ab".repeat(32)).is_ok());
        assert!(client_tls("nonsense").is_err());
    }
}
