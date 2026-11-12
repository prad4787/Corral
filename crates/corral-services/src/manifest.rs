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
    Valkey,
    Postgres,
    Mongodb,
}

impl Kind {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "mysql" => Some(Self::Mysql),
            // Valkey is the BSD-licensed Redis fork; `redis` is accepted as an alias.
            "valkey" | "redis" => Some(Self::Valkey),
            "postgres" | "postgresql" | "pgsql" => Some(Self::Postgres),
            "mongodb" | "mongo" => Some(Self::Mongodb),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mysql => "mysql",
            Self::Valkey => "valkey",
            Self::Postgres => "postgres",
            Self::Mongodb => "mongodb",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Mysql => "MySQL",
            Self::Valkey => "Valkey (Redis-compatible)",
            Self::Postgres => "PostgreSQL",
            Self::Mongodb => "MongoDB",
        }
    }
    pub fn default_port(self) -> u16 {
        match self {
            Self::Mysql => 3306,
            Self::Valkey => 6379,
            Self::Postgres => 5432,
            Self::Mongodb => 27017,
        }
    }
    pub fn license(self) -> &'static str {
        match self {
            Self::Mysql => "GPL-2.0 (with FOSS exception), Oracle MySQL Community Server",
            Self::Valkey => "BSD-3-Clause, Valkey (Linux Foundation)",
            Self::Postgres => "PostgreSQL License (permissive, BSD-like)",
            Self::Mongodb => {
                "SSPL-1.0, MongoDB Community Server (downloaded from MongoDB, never bundled)"
            }
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
    /// Source tarball that must be compiled (`make`) after extraction.
    pub build: Option<Build>,
}

/// A source build. With `configure`, runs `./configure --prefix=<dist> …`,
/// `make` and `make install`; otherwise runs `make` and copies `binaries`
/// (paths relative to the source tree) into `bin/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Build {
    pub configure: Option<&'static [&'static str]>,
    pub make_args: &'static [&'static str],
    pub binaries: &'static [&'static str],
    /// Subdirectories built and installed after the main tree (e.g.
    /// PostgreSQL's `contrib` extensions).
    pub extras: &'static [&'static str],
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

const VALKEY_SRC: Artifact = Artifact {
    url: "https://github.com/valkey-io/valkey/archive/refs/tags/9.1.3.tar.gz",
    sha256: "184e34ed2d9b51879b682b5b15205e2b9d677c60280e3cb453be8f520d04e293",
    strip: 1,
    provenance: "2026-11-10: from github.com/valkey-io/valkey-hashes README",
    build: Some(Build {
        configure: None,
        make_args: &["BUILD_TLS=no", "MALLOC=libc"],
        binaries: &["src/valkey-server", "src/valkey-cli"],
        extras: &[],
    }),
};

const POSTGRES_SRC: Artifact = Artifact {
    url: "https://ftp.postgresql.org/pub/source/v18.6/postgresql-18.6.tar.gz",
    sha256: "983ee554ec53dbeb9b70797bef9fcf4e67e117e7e48ca1463cc80b3ff8e8ff3f",
    strip: 1,
    provenance: "2026-11-10: from postgresql-18.6.tar.gz.sha256 on ftp.postgresql.org",
    build: Some(Build {
        // ICU and readline are optional; skipping them avoids Homebrew deps.
        // macOS libc has the e2fs-style uuid API that uuid-ossp needs.
        configure: Some(&["--without-icu", "--without-readline", "--with-uuid=e2fs"]),
        make_args: &[],
        binaries: &[],
        // dblink, hstore, pg_trgm, uuid-ossp, postgres_fdw, citext, …
        // (pgcrypto and sslinfo need OpenSSL and are skipped by contrib's Makefile.)
        extras: &["contrib"],
    }),
};

pub const RELEASES: &[Release] = &[
    Release {
        kind: Kind::Mongodb,
        version: "8.2.12",
        arm64: Some(Artifact {
            url: "https://fastdl.mongodb.org/osx/mongodb-macos-arm64-8.2.12.tgz",
            sha256: "8b1140dd745c23acafb812012fac3ff559c593065db631a5bec3dbafaf8dd1c2",
            strip: 1,
            provenance: "2026-11-11: sha256 from MongoDB's downloads.mongodb.org/full.json",
            build: None,
        }),
        x86_64: None,
    },
    Release {
        kind: Kind::Postgres,
        version: "18.6",
        arm64: Some(POSTGRES_SRC),
        x86_64: Some(POSTGRES_SRC),
    },
    Release {
        kind: Kind::Valkey,
        version: "9.1.3",
        // Same source for both CPUs; compiled locally.
        arm64: Some(VALKEY_SRC),
        x86_64: Some(VALKEY_SRC),
    },
    Release {
        kind: Kind::Mysql,
        version: "8.4.11",
        arm64: Some(Artifact {
            url: "https://cdn.mysql.com/Downloads/MySQL-8.4/mysql-8.4.11-macos15-arm64.tar.gz",
            sha256: "b96e00493bc3499b9ffd7f08d65c5d64933af0383a8287d9873b64f94c2d6009",
            strip: 1,
            provenance: "2026-11-10: MD5 6e89113f04f2af85d0a164573493db3a matched Oracle's published .md5",
            build: None,
        }),
        // Not pinned yet: needs the same download + cross-check on the x86_64 artifact.
        x86_64: None,
    },
];

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
