//! User configuration (`config.toml`). Human-editable; unknown keys are rejected
//! so typos surface instead of being silently ignored.

use crate::paths::Paths;
use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    /// Top-level domain for local sites, without the dot.
    pub tld: String,
    pub loopback: Ipv4Addr,
    /// Public HTTP/HTTPS ports. 80/443 require the privileged helper.
    pub http_port: u16,
    pub https_port: u16,
    /// Ports the unprivileged web server actually binds; the helper forwards
    /// the public ports here.
    pub internal_http_port: u16,
    pub internal_https_port: u16,
    /// Port of the built-in DNS resolver.
    pub dns_port: u16,
    /// Default PHP version (`major.minor`).
    pub default_php: Option<String>,
    /// Directories whose children are served as `<child>.<tld>`.
    pub parked: Vec<PathBuf>,
    /// Serve every site over HTTPS by default.
    pub secure_by_default: bool,
    /// Default Node.js major version for the `node`/`npm` shims.
    pub default_node: Option<u32>,
    /// Catch outgoing mail on 127.0.0.1:`mail_port` (0 disables it).
    pub mail_port: u16,
    /// Send `dump()` output from PHP to Corral's dump viewer.
    pub dumps: bool,
    /// Route every PHP request to the Xdebug pool while an IDE listens on
    /// 127.0.0.1:9003 (otherwise only Xdebug-triggered requests).
    pub xdebug_auto: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            tld: "test".into(),
            loopback: Ipv4Addr::LOCALHOST,
            http_port: 80,
            https_port: 443,
            internal_http_port: 18080,
            internal_https_port: 18443,
            dns_port: 15353,
            default_php: None,
            parked: Vec::new(),
            secure_by_default: true,
            default_node: None,
            mail_port: 2525,
            dumps: true,
            xdebug_auto: true,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("invalid TLD {0:?}: use lowercase letters, digits and '-' only")]
    InvalidTld(String),
    #[error("TLD {0:?} is a public suffix and would hijack real domains")]
    PublicTld(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Parse(#[from] toml::de::Error),
    #[error(transparent)]
    Serialize(#[from] toml::ser::Error),
}

/// TLDs we refuse because they resolve publicly (or are commonly used for real hosts).
const FORBIDDEN_TLDS: &[&str] = &["com", "net", "org", "io", "dev", "app", "local"];

impl Config {
    pub fn validate(&self) -> Result<(), ConfigError> {
        validate_tld(&self.tld)
    }

    pub fn load(paths: &Paths) -> Result<Self, ConfigError> {
        let file = paths.config_file();
        let cfg: Config = match std::fs::read_to_string(&file) {
            Ok(s) => toml::from_str(&s)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Config::default(),
            Err(e) => return Err(e.into()),
        };
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn save(&self, paths: &Paths) -> Result<(), ConfigError> {
        self.validate()?;
        std::fs::create_dir_all(&paths.root)?;
        crate::fsutil::write_atomic(
            &paths.config_file(),
            toml::to_string_pretty(self)?.as_bytes(),
        )?;
        Ok(())
    }
}

pub fn validate_tld(tld: &str) -> Result<(), ConfigError> {
    let ok = !tld.is_empty()
        && tld.len() <= 63
        && !tld.starts_with('-')
        && !tld.ends_with('-')
        && tld
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if !ok {
        return Err(ConfigError::InvalidTld(tld.into()));
    }
    if FORBIDDEN_TLDS.contains(&tld) {
        return Err(ConfigError::PublicTld(tld.into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_defaults() {
        let t = tempfile::tempdir().unwrap();
        let p = Paths::at(t.path());
        assert_eq!(Config::load(&p).unwrap(), Config::default());
        let c = Config {
            tld: "localhost".into(),
            parked: vec!["/Users/me/Sites".into()],
            ..Default::default()
        };
        c.save(&p).unwrap();
        assert_eq!(Config::load(&p).unwrap(), c);
    }

    #[test]
    fn rejects_bad_tlds() {
        assert!(validate_tld("test").is_ok());
        assert!(validate_tld("my-dev").is_ok());
        assert!(matches!(
            validate_tld("Test"),
            Err(ConfigError::InvalidTld(_))
        ));
        assert!(matches!(
            validate_tld("a.b"),
            Err(ConfigError::InvalidTld(_))
        ));
        assert!(matches!(
            validate_tld("com"),
            Err(ConfigError::PublicTld(_))
        ));
    }

    #[test]
    fn rejects_unknown_keys() {
        let t = tempfile::tempdir().unwrap();
        let p = Paths::at(t.path());
        std::fs::write(p.config_file(), "tdl = \"test\"\n").unwrap();
        assert!(matches!(Config::load(&p), Err(ConfigError::Parse(_))));
    }
}
