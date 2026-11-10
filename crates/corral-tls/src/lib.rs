//! Corral's local certificate authority.
//!
//! - One CA per user, ECDSA P-256, valid 10 years, **name-constrained to the
//!   configured TLD** so the key cannot mint certificates for real domains.
//! - Leaf certificates per site (`host` + `*.host`), valid 397 days (the
//!   maximum Apple platforms accept for TLS server certs), renewed 30 days
//!   before expiry or whenever the CA changes.
//! - Private keys are written with mode 0600.
//!
//! Trusting the CA in the System keychain is the privileged helper's job; this
//! crate only produces the DER/PEM and its fingerprint.

use corral_core::fsutil::write_atomic;
use corral_core::paths::Paths;
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose,
    GeneralSubtree, IsCa, Issuer, KeyPair, KeyUsagePurpose, NameConstraints,
};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use time::{Duration, OffsetDateTime};

const CA_VALIDITY_DAYS: i64 = 3650;
const LEAF_VALIDITY_DAYS: i64 = 397;
const RENEW_BEFORE_DAYS: i64 = 30;

#[derive(Debug, thiserror::Error)]
pub enum TlsError {
    #[error(transparent)]
    Rcgen(#[from] rcgen::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("certificate could not be parsed: {0}")]
    Parse(String),
    #[error("host {host:?} is outside the CA's permitted TLD {tld:?}")]
    OutsideTld { host: String, tld: String },
}

pub struct LocalCa {
    dir: PathBuf,
    tld: String,
    cert_pem: String,
    key: KeyPair,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeafFiles {
    pub cert: PathBuf,
    pub key: PathBuf,
}

fn write_private(path: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    write_atomic(path, data)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

fn pem_to_der(pem: &str) -> Result<Vec<u8>, TlsError> {
    let (_, p) = x509_parser::pem::parse_x509_pem(pem.as_bytes())
        .map_err(|e| TlsError::Parse(e.to_string()))?;
    Ok(p.contents)
}

/// Seconds-since-epoch `not_after` and the issuer DN of a PEM certificate.
fn inspect(pem: &str) -> Result<(i64, String, Vec<String>), TlsError> {
    let der = pem_to_der(pem)?;
    let (_, cert) =
        x509_parser::parse_x509_certificate(&der).map_err(|e| TlsError::Parse(e.to_string()))?;
    let sans = cert
        .subject_alternative_name()
        .ok()
        .flatten()
        .map(|ext| {
            ext.value
                .general_names
                .iter()
                .filter_map(|n| match n {
                    x509_parser::extensions::GeneralName::DNSName(d) => Some(d.to_string()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    Ok((
        cert.validity().not_after.timestamp(),
        cert.issuer().to_string(),
        sans,
    ))
}

impl LocalCa {
    pub fn cert_path(dir: &Path) -> PathBuf {
        dir.join("ca.crt")
    }
    pub fn key_path(dir: &Path) -> PathBuf {
        dir.join("ca.key")
    }

    /// Load the CA from `paths.certs_dir()`, creating it if missing or if it
    /// was constrained to a different TLD.
    pub fn load_or_create(paths: &Paths, tld: &str) -> Result<Self, TlsError> {
        let dir = paths.certs_dir();
        if let Some(ca) = Self::load(&dir, tld)? {
            return Ok(ca);
        }
        Self::create(&dir, tld)
    }

    fn tld_marker(dir: &Path) -> PathBuf {
        dir.join("ca.tld")
    }

    fn load(dir: &Path, tld: &str) -> Result<Option<Self>, TlsError> {
        let (Ok(cert_pem), Ok(key_pem)) = (
            std::fs::read_to_string(Self::cert_path(dir)),
            std::fs::read_to_string(Self::key_path(dir)),
        ) else {
            return Ok(None);
        };
        let stored_tld = std::fs::read_to_string(Self::tld_marker(dir)).unwrap_or_default();
        let (not_after, _, _) = inspect(&cert_pem)?;
        let expired =
            not_after - OffsetDateTime::now_utc().unix_timestamp() < RENEW_BEFORE_DAYS * 86_400;
        if stored_tld.trim() != tld || expired {
            return Ok(None);
        }
        Ok(Some(Self {
            dir: dir.to_path_buf(),
            tld: tld.into(),
            cert_pem,
            key: KeyPair::from_pem(&key_pem)?,
        }))
    }

    fn create(dir: &Path, tld: &str) -> Result<Self, TlsError> {
        std::fs::create_dir_all(dir)?;
        let key = KeyPair::generate()?;
        let mut params = CertificateParams::new(Vec::<String>::new())?;
        let mut dn = DistinguishedName::new();
        let who = std::env::var("USER").unwrap_or_else(|_| "user".into());
        dn.push(
            DnType::CommonName,
            format!("Corral Local CA ({who}, .{tld})"),
        );
        dn.push(DnType::OrganizationName, "Corral local development");
        params.distinguished_name = dn;
        params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        params.name_constraints = Some(NameConstraints {
            permitted_subtrees: vec![GeneralSubtree::DnsName(tld.into())],
            excluded_subtrees: vec![],
        });
        let now = OffsetDateTime::now_utc();
        params.not_before = now - Duration::days(1);
        params.not_after = now + Duration::days(CA_VALIDITY_DAYS);
        let cert = params.self_signed(&key)?;

        write_private(&Self::key_path(dir), key.serialize_pem().as_bytes())?;
        write_atomic(&Self::cert_path(dir), cert.pem().as_bytes())?;
        write_atomic(&Self::tld_marker(dir), tld.as_bytes())?;
        // A new CA invalidates every leaf.
        for ent in std::fs::read_dir(dir)?.flatten() {
            let p = ent.path();
            if p.extension().is_some_and(|e| e == "crt" || e == "key")
                && p.file_stem().is_none_or(|s| s != "ca")
            {
                std::fs::remove_file(p)?;
            }
        }
        Ok(Self {
            dir: dir.to_path_buf(),
            tld: tld.into(),
            cert_pem: cert.pem(),
            key,
        })
    }

    pub fn cert_pem(&self) -> &str {
        &self.cert_pem
    }

    pub fn cert_der(&self) -> Result<Vec<u8>, TlsError> {
        pem_to_der(&self.cert_pem)
    }

    /// SHA-256 of the CA certificate DER, hex. Used to untrust exactly this CA.
    pub fn fingerprint(&self) -> Result<String, TlsError> {
        Ok(hex::encode(Sha256::digest(self.cert_der()?)))
    }

    fn issuer_dn(&self) -> Result<String, TlsError> {
        Ok(inspect(&self.cert_pem)?.1)
    }

    pub fn leaf_files(&self, host: &str) -> LeafFiles {
        LeafFiles {
            cert: self.dir.join(format!("{host}.crt")),
            key: self.dir.join(format!("{host}.key")),
        }
    }

    /// Return a valid leaf for `host`, issuing a new one when needed.
    pub fn ensure_leaf(&self, host: &str) -> Result<LeafFiles, TlsError> {
        if !(host == self.tld || host.ends_with(&format!(".{}", self.tld))) {
            return Err(TlsError::OutsideTld {
                host: host.into(),
                tld: self.tld.clone(),
            });
        }
        let files = self.leaf_files(host);
        if let (Ok(pem), true) = (std::fs::read_to_string(&files.cert), files.key.exists()) {
            let (not_after, issuer, sans) = inspect(&pem)?;
            let fresh =
                not_after - OffsetDateTime::now_utc().unix_timestamp() > RENEW_BEFORE_DAYS * 86_400;
            if fresh && issuer == self.issuer_dn()? && sans.iter().any(|s| s == host) {
                return Ok(files);
            }
        }
        let key = KeyPair::generate()?;
        let mut params = CertificateParams::new(vec![host.to_string(), format!("*.{host}")])?;
        params.distinguished_name = DistinguishedName::new();
        params.distinguished_name.push(DnType::CommonName, host);
        params.is_ca = IsCa::ExplicitNoCa;
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        params.use_authority_key_identifier_extension = true;
        let now = OffsetDateTime::now_utc();
        params.not_before = now - Duration::days(1);
        params.not_after = now + Duration::days(LEAF_VALIDITY_DAYS);

        let issuer = Issuer::from_ca_cert_pem(&self.cert_pem, &self.key)?;
        let cert = params.signed_by(&key, &issuer)?;
        write_private(&files.key, key.serialize_pem().as_bytes())?;
        write_atomic(&files.cert, cert.pem().as_bytes())?;
        Ok(files)
    }

    /// Delete leaf files for hosts not in `keep`.
    pub fn prune_leaves(&self, keep: &[&str]) -> std::io::Result<usize> {
        let mut removed = 0;
        for ent in std::fs::read_dir(&self.dir)?.flatten() {
            let p = ent.path();
            let Some(stem) = p.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let is_leaf = p.extension().is_some_and(|e| e == "crt" || e == "key") && stem != "ca";
            if is_leaf && !keep.contains(&stem) {
                std::fs::remove_file(p)?;
                removed += 1;
            }
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn ca() -> (tempfile::TempDir, Paths, LocalCa) {
        let t = tempfile::tempdir().unwrap();
        let p = Paths::at(t.path());
        let ca = LocalCa::load_or_create(&p, "test").unwrap();
        (t, p, ca)
    }

    fn parse(pem: &str) -> Vec<u8> {
        pem_to_der(pem).unwrap()
    }

    #[test]
    fn ca_is_name_constrained_and_private() {
        let (_t, p, ca) = ca();
        let der = parse(ca.cert_pem());
        let (_, cert) = x509_parser::parse_x509_certificate(&der).unwrap();
        assert!(cert.is_ca());
        let nc = cert.name_constraints().unwrap().unwrap();
        let permitted = format!("{:?}", nc.value.permitted_subtrees);
        assert!(permitted.contains("\"test\""), "{permitted}");
        let mode = std::fs::metadata(LocalCa::key_path(&p.certs_dir()))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        assert_eq!(ca.fingerprint().unwrap().len(), 64);
    }

    #[test]
    fn ca_is_reused_until_tld_changes() {
        let (_t, p, ca) = ca();
        let fp = ca.fingerprint().unwrap();
        assert_eq!(
            LocalCa::load_or_create(&p, "test")
                .unwrap()
                .fingerprint()
                .unwrap(),
            fp
        );
        ca.ensure_leaf("blog.test").unwrap();
        let other = LocalCa::load_or_create(&p, "localhost").unwrap();
        assert_ne!(other.fingerprint().unwrap(), fp);
        assert!(
            !ca.leaf_files("blog.test").cert.exists(),
            "old leaves are wiped"
        );
    }

    #[test]
    fn leaf_is_signed_by_ca_and_cached() {
        let (_t, _p, ca) = ca();
        let f = ca.ensure_leaf("blog.test").unwrap();
        let pem = std::fs::read_to_string(&f.cert).unwrap();
        let der = parse(&pem);
        let (_, leaf) = x509_parser::parse_x509_certificate(&der).unwrap();
        let ca_der = ca.cert_der().unwrap();
        let (_, ca_cert) = x509_parser::parse_x509_certificate(&ca_der).unwrap();
        leaf.verify_signature(Some(ca_cert.public_key())).unwrap();
        let (_, _, sans) = inspect(&pem).unwrap();
        assert_eq!(sans, ["blog.test", "*.blog.test"]);
        let days = (leaf.validity().not_after.timestamp() - leaf.validity().not_before.timestamp())
            / 86_400;
        assert!(days <= 398);

        let mtime = std::fs::metadata(&f.cert).unwrap().modified().unwrap();
        ca.ensure_leaf("blog.test").unwrap();
        assert_eq!(
            std::fs::metadata(&f.cert).unwrap().modified().unwrap(),
            mtime
        );
    }

    #[test]
    fn refuses_hosts_outside_tld_and_prunes() {
        let (_t, _p, ca) = ca();
        assert!(matches!(
            ca.ensure_leaf("google.com"),
            Err(TlsError::OutsideTld { .. })
        ));
        assert!(matches!(
            ca.ensure_leaf("eviltest"),
            Err(TlsError::OutsideTld { .. })
        ));
        ca.ensure_leaf("a.test").unwrap();
        ca.ensure_leaf("b.test").unwrap();
        assert_eq!(ca.prune_leaves(&["a.test"]).unwrap(), 2);
        assert!(ca.leaf_files("a.test").cert.exists());
        assert!(LocalCa::cert_path(&ca.dir).exists());
    }
}
