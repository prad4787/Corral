//! Downloading runtimes (Caddy, PHP) with integrity verification.
//!
//! Every artifact is downloaded to `downloads/`, hashed while streaming, and
//! verified **before** anything is extracted. Archives are never extracted
//! wholesale: only the named entry is read, which rules out path traversal.

use anyhow::{Context, bail};
use corral_core::paths::Paths;
use corral_core::php::PhpVersion;
use futures_util::StreamExt;
use sha2::{Digest, Sha256, Sha512};
use std::io::Read;
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

pub const CADDY_VERSION: &str = "2.11.9";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hash {
    Sha256,
    Sha512,
}

/// Expected digest of an artifact. `None` is only allowed with an explicit opt-in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expected {
    pub algo: Hash,
    pub hex: String,
}

pub fn arch() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "amd64"
    }
}

pub fn tools_dir(paths: &Paths) -> PathBuf {
    paths.root.join("tools")
}

pub fn caddy_bin(paths: &Paths) -> PathBuf {
    tools_dir(paths).join("caddy")
}

fn client() -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(concat!("corral/", env!("CARGO_PKG_VERSION")))
        .build()?)
}

/// Stream `url` to `dest`, returning (sha256, sha512) hex digests.
async fn download(url: &str, dest: &Path) -> anyhow::Result<(String, String)> {
    tracing::info!(%url, "downloading");
    let resp = client()?
        .get(url)
        .send()
        .await?
        .error_for_status()
        .with_context(|| format!("GET {url}"))?;
    if let Some(dir) = dest.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    let part = dest.with_extension("part");
    let mut file = tokio::fs::File::create(&part).await?;
    let (mut h256, mut h512) = (Sha256::new(), Sha512::new());
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        h256.update(&chunk);
        h512.update(&chunk);
        file.write_all(&chunk).await?;
    }
    file.sync_all().await?;
    tokio::fs::rename(&part, dest).await?;
    Ok((hex::encode(h256.finalize()), hex::encode(h512.finalize())))
}

pub fn verify(actual: (&str, &str), expected: Option<&Expected>, url: &str) -> anyhow::Result<()> {
    match expected {
        Some(e) => {
            let got = match e.algo {
                Hash::Sha256 => actual.0,
                Hash::Sha512 => actual.1,
            };
            if !got.eq_ignore_ascii_case(e.hex.trim()) {
                bail!("checksum mismatch for {url}: expected {}, got {got}", e.hex);
            }
            Ok(())
        }
        None => {
            tracing::warn!(%url, sha256 = actual.0, "installed WITHOUT checksum verification");
            Ok(())
        }
    }
}

/// Extract the single regular-file entry named `entry` (basename match) from a
/// `.tar.gz` into `dest` with mode 0755.
pub fn extract_entry(archive: &Path, entry: &str, dest: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let gz = flate2::read::GzDecoder::new(std::fs::File::open(archive)?);
    let mut ar = tar::Archive::new(gz);
    for e in ar.entries()? {
        let mut e = e?;
        let is_file = e.header().entry_type().is_file();
        let path = e.path()?.into_owned();
        if is_file && path.file_name().and_then(|n| n.to_str()) == Some(entry) {
            if let Some(dir) = dest.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let tmp = dest.with_extension("extracting");
            let mut buf = Vec::new();
            e.read_to_end(&mut buf)?;
            std::fs::write(&tmp, &buf)?;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
            std::fs::rename(&tmp, dest)?;
            return Ok(());
        }
    }
    bail!("{} does not contain {entry}", archive.display())
}

/// Parse a `checksums.txt` (`<hex>  <filename>` per line) and find `file`.
pub fn find_checksum(checksums: &str, file: &str) -> Option<String> {
    checksums.lines().find_map(|l| {
        let mut it = l.split_whitespace();
        let (h, f) = (it.next()?, it.next()?);
        (f.trim_start_matches('*') == file).then(|| h.to_string())
    })
}

/// Install Caddy from the official GitHub release, verified against the
/// release's SHA-512 checksum file.
pub async fn install_caddy(paths: &Paths) -> anyhow::Result<PathBuf> {
    let v = CADDY_VERSION;
    let file = format!("caddy_{v}_mac_{}.tar.gz", arch());
    let base = format!("https://github.com/caddyserver/caddy/releases/download/v{v}");
    let sums = client()?
        .get(format!("{base}/caddy_{v}_checksums.txt"))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let expected = Expected {
        algo: Hash::Sha512,
        hex: find_checksum(&sums, &file)
            .with_context(|| format!("{file} missing from checksums"))?,
    };
    let url = format!("{base}/{file}");
    let archive = paths.downloads_dir().join(&file);
    let (s256, s512) = download(&url, &archive).await?;
    verify((&s256, &s512), Some(&expected), &url)?;
    let bin = caddy_bin(paths);
    extract_entry(&archive, "caddy", &bin)?;
    std::fs::remove_file(&archive).ok();
    Ok(bin)
}

/// Where a PHP build comes from.
#[derive(Debug, Clone)]
pub struct PhpSource {
    pub cli_url: String,
    pub fpm_url: String,
    pub cli_sha256: Option<String>,
    pub fpm_sha256: Option<String>,
}

impl PhpSource {
    /// Upstream static-php-cli "common" builds. They publish no checksums, so
    /// installing from here requires `allow_unverified` until Corral hosts its
    /// own signed manifest (PLAN 0.5).
    pub fn static_php(full_version: &str) -> Self {
        let a = if cfg!(target_arch = "aarch64") {
            "aarch64"
        } else {
            "x86_64"
        };
        let base = "https://dl.static-php.dev/static-php-cli/common";
        Self {
            cli_url: format!("{base}/php-{full_version}-cli-macos-{a}.tar.gz"),
            fpm_url: format!("{base}/php-{full_version}-fpm-macos-{a}.tar.gz"),
            cli_sha256: None,
            fpm_sha256: None,
        }
    }
}

/// Find the newest patch release of `minor` in the static-php index page.
pub async fn latest_static_php(minor: PhpVersion) -> anyhow::Result<String> {
    let html = client()?
        .get("https://dl.static-php.dev/static-php-cli/common/")
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    newest_patch(&html, minor).with_context(|| format!("no static build of PHP {minor} found"))
}

/// Pure helper for [`latest_static_php`].
pub fn newest_patch(index: &str, minor: PhpVersion) -> Option<String> {
    let prefix = format!("php-{minor}.");
    let mut best: Option<(u32, String)> = None;
    for part in index.split(&prefix).skip(1) {
        let patch: String = part.chars().take_while(char::is_ascii_digit).collect();
        if patch.is_empty() || !part[patch.len()..].starts_with("-fpm-macos-") {
            continue;
        }
        let n: u32 = patch.parse().ok()?;
        if best.as_ref().is_none_or(|(b, _)| n > *b) {
            best = Some((n, format!("{minor}.{n}")));
        }
    }
    best.map(|(_, v)| v)
}

pub async fn install_php(
    paths: &Paths,
    version: PhpVersion,
    src: &PhpSource,
    allow_unverified: bool,
) -> anyhow::Result<PathBuf> {
    if !allow_unverified && (src.cli_sha256.is_none() || src.fpm_sha256.is_none()) {
        bail!(
            "refusing to install PHP {version} without checksums (pass --allow-unverified to override)"
        );
    }
    let dir = paths.php_version_dir(&version.to_string());
    for (url, sha, entry, dest) in [
        (&src.cli_url, &src.cli_sha256, "php", dir.join("bin/php")),
        (
            &src.fpm_url,
            &src.fpm_sha256,
            "php-fpm",
            dir.join("sbin/php-fpm"),
        ),
    ] {
        let name = url.rsplit('/').next().unwrap_or("php.tar.gz");
        let archive = paths.downloads_dir().join(name);
        let (s256, s512) = download(url, &archive).await?;
        let exp = sha.as_ref().map(|h| Expected {
            algo: Hash::Sha256,
            hex: h.clone(),
        });
        verify((&s256, &s512), exp.as_ref(), url)?;
        extract_entry(&archive, entry, &dest)?;
        std::fs::remove_file(&archive).ok();
    }
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tarball(dir: &Path, entries: &[(&str, &[u8])]) -> PathBuf {
        let p = dir.join("a.tar.gz");
        let gz = flate2::write::GzEncoder::new(
            std::fs::File::create(&p).unwrap(),
            flate2::Compression::fast(),
        );
        let mut b = tar::Builder::new(gz);
        for (name, data) in entries {
            let mut h = tar::Header::new_gnu();
            h.set_size(data.len() as u64);
            h.set_mode(0o644);
            h.set_cksum();
            b.append_data(&mut h, name, *data).unwrap();
        }
        b.into_inner().unwrap().finish().unwrap();
        p
    }

    #[test]
    fn extracts_only_named_entry() {
        use std::os::unix::fs::PermissionsExt;
        let t = tempfile::tempdir().unwrap();
        let a = tarball(t.path(), &[("LICENSE", b"x"), ("pkg/caddy", b"BIN")]);
        let dest = t.path().join("out/caddy");
        extract_entry(&a, "caddy", &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"BIN");
        assert_eq!(
            std::fs::metadata(&dest).unwrap().permissions().mode() & 0o777,
            0o755
        );
        assert!(!t.path().join("out/LICENSE").exists());
        assert!(extract_entry(&a, "php", &t.path().join("php")).is_err());
    }

    #[test]
    fn checksum_lookup_and_verify() {
        let sums = "aa11  caddy_2_mac_arm64.tar.gz\nbb22 *caddy_2_linux.tar.gz\n";
        assert_eq!(
            find_checksum(sums, "caddy_2_mac_arm64.tar.gz").as_deref(),
            Some("aa11")
        );
        assert_eq!(
            find_checksum(sums, "caddy_2_linux.tar.gz").as_deref(),
            Some("bb22")
        );
        assert_eq!(find_checksum(sums, "nope"), None);
        let e = Expected {
            algo: Hash::Sha512,
            hex: "ABCD".into(),
        };
        assert!(verify(("x", "abcd"), Some(&e), "u").is_ok());
        assert!(verify(("abcd", "ffff"), Some(&e), "u").is_err());
        assert!(verify(("x", "y"), None, "u").is_ok());
    }

    #[test]
    fn finds_newest_patch() {
        let idx = r#"<a href="php-8.4.9-fpm-macos-aarch64.tar.gz"> <a href="php-8.4.23-cli-macos-aarch64.tar.gz">
            <a href="php-8.4.23-fpm-macos-aarch64.tar.gz"> <a href="php-8.4.100-cli-linux.tar.gz">
            <a href="php-8.5.1-fpm-macos-aarch64.tar.gz">"#;
        let v = PhpVersion::parse("8.4").unwrap();
        assert_eq!(newest_patch(idx, v).as_deref(), Some("8.4.23"));
        assert_eq!(newest_patch(idx, PhpVersion::parse("7.4").unwrap()), None);
    }

    #[tokio::test]
    async fn php_install_requires_checksums_unless_opted_out() {
        let t = tempfile::tempdir().unwrap();
        let p = Paths::at(t.path());
        let src = PhpSource::static_php("8.4.23");
        let err = install_php(&p, PhpVersion::parse("8.4").unwrap(), &src, false)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("without checksums"));
    }
}
