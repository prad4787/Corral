//! Verified downloads and safe archive extraction.
//!
//! Every artifact is streamed to disk, hashed while downloading, and verified
//! **before** anything is extracted. Extraction never writes outside the
//! destination: entries with absolute paths or `..` components are rejected,
//! and so are symlinks whose target would escape the destination.

use anyhow::{Context, bail};
use futures_util::StreamExt;
use sha2::{Digest, Sha256, Sha512};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use tokio::io::AsyncWriteExt;

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

pub fn client() -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(concat!("corral/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(std::time::Duration::from_secs(15))
        // Per-read timeout (not total), so large downloads on slow links work.
        .read_timeout(std::time::Duration::from_secs(60))
        .build()?)
}

/// Stream `url` to `dest`, returning (sha256, sha512) hex digests.
pub async fn download(url: &str, dest: &Path) -> anyhow::Result<(String, String)> {
    const ATTEMPTS: u32 = 3;
    let mut last = None;
    for attempt in 1..=ATTEMPTS {
        match download_once(url, dest).await {
            Ok(v) => return Ok(v),
            Err(e) => {
                tracing::warn!(%url, attempt, error = %e, "download failed");
                last = Some(e);
                if attempt < ATTEMPTS {
                    tokio::time::sleep(std::time::Duration::from_secs(u64::from(attempt))).await;
                }
            }
        }
    }
    Err(last.expect("at least one attempt"))
}

async fn download_once(url: &str, dest: &Path) -> anyhow::Result<(String, String)> {
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

/// Extract the regular-file entry whose basename is `entry` from a `.zip`
/// into `dest` with mode 0755 (only that entry is read).
pub fn extract_zip_entry(archive: &Path, entry: &str, dest: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut zip = zip::ZipArchive::new(std::fs::File::open(archive)?)?;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i)?;
        let name = f.name().to_string();
        let base = name.rsplit('/').next().unwrap_or(&name);
        if f.is_file() && base == entry {
            if let Some(dir) = dest.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let tmp = dest.with_extension("extracting");
            let mut buf = Vec::new();
            f.read_to_end(&mut buf)?;
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

fn safe_relative(p: &Path) -> bool {
    p.components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

/// Extract a whole `.tar.gz` into `dest`, dropping the first `strip`
/// leading path components (like `tar --strip-components`).
///
/// Extraction goes to a sibling temp dir that is renamed into place, so a
/// failed extraction never leaves a half-installed tree at `dest`.
pub fn extract_tree(archive: &Path, dest: &Path, strip: usize) -> anyhow::Result<()> {
    let parent = dest.parent().context("destination has no parent")?;
    std::fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(
        ".{}.extracting-{}",
        dest.file_name().and_then(|n| n.to_str()).unwrap_or("tree"),
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp)?;
    let result = (|| -> anyhow::Result<()> {
        let gz = flate2::read::GzDecoder::new(std::fs::File::open(archive)?);
        let mut ar = tar::Archive::new(gz);
        ar.set_preserve_permissions(true);
        for e in ar.entries()? {
            let mut e = e?;
            let path = e.path()?.into_owned();
            if !safe_relative(&path) {
                bail!("unsafe path in archive: {}", path.display());
            }
            let rel: PathBuf = path.components().skip(strip).collect();
            if rel.as_os_str().is_empty() {
                continue;
            }
            let kind = e.header().entry_type();
            if kind.is_symlink() || kind.is_hard_link() {
                let target = e.link_name()?.context("link without target")?.into_owned();
                // Resolve relative to the link's own directory; must stay inside.
                let base = rel.parent().unwrap_or(Path::new(""));
                let mut depth: i64 = base.components().count() as i64;
                let escapes = target.is_absolute()
                    || target.components().any(|c| {
                        match c {
                            Component::ParentDir => depth -= 1,
                            Component::Normal(_) => depth += 1,
                            _ => {}
                        }
                        depth < 0
                    });
                if escapes {
                    bail!(
                        "link escapes destination: {} -> {}",
                        rel.display(),
                        target.display()
                    );
                }
                if kind.is_hard_link() {
                    // Hard-link targets are archive paths; strip them the same way.
                    let t: PathBuf = target.components().skip(strip).collect();
                    let out = tmp.join(&rel);
                    if let Some(d) = out.parent() {
                        std::fs::create_dir_all(d)?;
                    }
                    std::fs::hard_link(tmp.join(t), out)?;
                    continue;
                }
            } else if !(kind.is_file() || kind.is_dir()) {
                continue; // skip devices, fifos, pax headers etc.
            }
            let out = tmp.join(&rel);
            if let Some(d) = out.parent() {
                std::fs::create_dir_all(d)?;
            }
            e.unpack(&out)?;
        }
        Ok(())
    })();
    if let Err(err) = result {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(err);
    }
    if dest.exists() {
        std::fs::remove_dir_all(dest)?;
    }
    std::fs::rename(&tmp, dest)?;
    Ok(())
}

/// Download `url` to `dir/<file name>`, verify it against `expected`, and
/// return the path. The file is deleted if verification fails.
pub async fn fetch_verified(
    url: &str,
    dir: &Path,
    expected: Option<&Expected>,
) -> anyhow::Result<PathBuf> {
    let name = url
        .rsplit('/')
        .next()
        .filter(|n| !n.is_empty())
        .unwrap_or("download");
    let dest = dir.join(name);
    let (s256, s512) = download(url, &dest).await?;
    if let Err(e) = verify((&s256, &s512), expected, url) {
        let _ = std::fs::remove_file(&dest);
        return Err(e);
    }
    Ok(dest)
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

    fn tarball_with(
        dir: &Path,
        build: impl FnOnce(&mut tar::Builder<flate2::write::GzEncoder<std::fs::File>>),
    ) -> PathBuf {
        let p = dir.join("t.tar.gz");
        let gz = flate2::write::GzEncoder::new(
            std::fs::File::create(&p).unwrap(),
            flate2::Compression::fast(),
        );
        let mut b = tar::Builder::new(gz);
        build(&mut b);
        b.into_inner().unwrap().finish().unwrap();
        p
    }

    fn file(
        b: &mut tar::Builder<flate2::write::GzEncoder<std::fs::File>>,
        name: &str,
        data: &[u8],
        mode: u32,
    ) {
        let mut h = tar::Header::new_gnu();
        h.set_size(data.len() as u64);
        h.set_mode(mode);
        h.set_cksum();
        b.append_data(&mut h, name, data).unwrap();
    }

    fn symlink(
        b: &mut tar::Builder<flate2::write::GzEncoder<std::fs::File>>,
        name: &str,
        target: &str,
    ) {
        let mut h = tar::Header::new_gnu();
        h.set_entry_type(tar::EntryType::Symlink);
        h.set_size(0);
        h.set_mode(0o777);
        b.append_link(&mut h, name, target).unwrap();
    }

    #[test]
    fn extracts_named_zip_entry() {
        use std::io::Write;
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("a.zip");
        let mut w = zip::ZipWriter::new(std::fs::File::create(&p).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        w.start_file("README.md", opts).unwrap();
        w.write_all(b"readme").unwrap();
        w.start_file("pgweb_darwin_arm64", opts).unwrap();
        w.write_all(b"BIN").unwrap();
        w.finish().unwrap();
        let dest = t.path().join("tools/pgweb");
        extract_zip_entry(&p, "pgweb_darwin_arm64", &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"BIN");
        assert!(extract_zip_entry(&p, "nope", &t.path().join("x")).is_err());
    }

    #[test]
    fn extract_tree_strips_and_keeps_modes_and_links() {
        use std::os::unix::fs::PermissionsExt;
        let t = tempfile::tempdir().unwrap();
        let a = tarball_with(t.path(), |b| {
            file(b, "mysql-8/bin/mysqld", b"BIN", 0o755);
            file(b, "mysql-8/lib/libx.1.dylib", b"LIB", 0o644);
            symlink(b, "mysql-8/lib/libx.dylib", "libx.1.dylib");
        });
        let dest = t.path().join("dist/mysql");
        extract_tree(&a, &dest, 1).unwrap();
        assert_eq!(std::fs::read(dest.join("bin/mysqld")).unwrap(), b"BIN");
        let mode = std::fs::metadata(dest.join("bin/mysqld"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111, "exec bit kept");
        assert_eq!(std::fs::read(dest.join("lib/libx.dylib")).unwrap(), b"LIB");
        // Re-extracting replaces the tree atomically.
        extract_tree(&a, &dest, 1).unwrap();
        assert!(dest.join("bin/mysqld").exists());
    }

    #[test]
    fn extract_tree_rejects_escaping_links_and_leaves_nothing() {
        let t = tempfile::tempdir().unwrap();
        for target in ["../../etc/passwd", "/etc/passwd", "../../../x"] {
            let a = tarball_with(t.path(), |b| {
                file(b, "pkg/ok", b"x", 0o644);
                symlink(b, "pkg/lib/evil", target);
            });
            let dest = t.path().join("out");
            let err = extract_tree(&a, &dest, 1).unwrap_err().to_string();
            assert!(err.contains("escapes"), "{target}: {err}");
            assert!(!dest.exists(), "nothing installed on failure");
        }
        // A link that climbs but stays inside is fine: lib/a -> ../bin/b
        let a = tarball_with(t.path(), |b| {
            file(b, "pkg/bin/b", b"B", 0o755);
            symlink(b, "pkg/lib/a", "../bin/b");
        });
        extract_tree(&a, &t.path().join("ok"), 1).unwrap();
        assert_eq!(std::fs::read(t.path().join("ok/lib/a")).unwrap(), b"B");
    }
}
