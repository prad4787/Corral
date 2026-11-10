//! Built-in catalog of service releases with **pinned SHA-256 digests**.
//!
//! Every artifact here was downloaded from its official upstream URL and its
//! digest cross-checked against the vendor's published checksum before being
//! pinned. Installing anything not listed requires editing this file, which
//! goes through code review.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Mysql,
}

impl Kind {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "mysql" => Some(Self::Mysql),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mysql => "mysql",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Mysql => "MySQL",
        }
    }
    pub fn default_port(self) -> u16 {
        match self {
            Self::Mysql => 3306,
        }
    }
    pub fn license(self) -> &'static str {
        match self {
            Self::Mysql => "GPL-2.0 (with FOSS exception), Oracle MySQL Community Server",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Artifact {
    pub url: &'static str,
    pub sha256: &'static str,
    /// Leading path components to strip when extracting.
    pub strip: usize,
    /// How the digest was cross-checked when pinned.
    pub provenance: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Release {
    pub kind: Kind,
    pub version: &'static str,
    pub arm64: Option<Artifact>,
    pub x86_64: Option<Artifact>,
}

impl Release {
    /// `8.4.11` → `8.4`
    pub fn minor(&self) -> &str {
        let mut dots = self.version.match_indices('.');
        match dots.nth(1) {
            Some((i, _)) => &self.version[..i],
            None => self.version,
        }
    }

    pub fn artifact(&self) -> Option<&Artifact> {
        if cfg!(target_arch = "aarch64") {
            self.arm64.as_ref()
        } else {
            self.x86_64.as_ref()
        }
    }
}

pub const RELEASES: &[Release] = &[Release {
    kind: Kind::Mysql,
    version: "8.4.11",
    arm64: Some(Artifact {
        url: "https://cdn.mysql.com/Downloads/MySQL-8.4/mysql-8.4.11-macos15-arm64.tar.gz",
        sha256: "b96e00493bc3499b9ffd7f08d65c5d64933af0383a8287d9873b64f94c2d6009",
        strip: 1,
        provenance: "2026-11-10: MD5 6e89113f04f2af85d0a164573493db3a matched Oracle's published .md5",
    }),
    // Not pinned yet: needs the same download + cross-check on the x86_64 artifact.
    x86_64: None,
}];

/// Newest release of `kind` whose version starts with `want` (e.g. "8.4" or
/// "8.4.11"); `None` for `want` picks the newest overall.
pub fn find(kind: Kind, want: Option<&str>) -> Option<&'static Release> {
    RELEASES
        .iter()
        .filter(|r| r.kind == kind)
        .filter(|r| want.is_none_or(|w| r.version == w || r.version.starts_with(&format!("{w}."))))
        .max_by(|a, b| version_key(a.version).cmp(&version_key(b.version)))
}

fn version_key(v: &str) -> Vec<u32> {
    v.split('.').filter_map(|p| p.parse().ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup() {
        let r = find(Kind::Mysql, Some("8.4")).unwrap();
        assert_eq!((r.version, r.minor()), ("8.4.11", "8.4"));
        assert!(find(Kind::Mysql, Some("8.4.11")).is_some());
        assert!(
            find(Kind::Mysql, Some("8.41")).is_none(),
            "prefix must end at a dot"
        );
        assert!(find(Kind::Mysql, Some("5.7")).is_none());
        assert_eq!(find(Kind::Mysql, None).unwrap().version, "8.4.11");
        assert_eq!(Kind::parse("MySQL"), Some(Kind::Mysql));
    }

    #[test]
    fn pinned_digests_are_well_formed() {
        for r in RELEASES {
            for a in [r.arm64, r.x86_64].into_iter().flatten() {
                assert_eq!(a.sha256.len(), 64, "{}", a.url);
                assert!(a.sha256.bytes().all(|b| b.is_ascii_hexdigit()));
                assert!(a.url.starts_with("https://"));
            }
        }
    }
}
