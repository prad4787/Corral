use std::io::Write;
use std::path::Path;

/// Write a file atomically: temp file in the same directory, fsync, rename.
pub fn write_atomic(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let tmp = dir.join(format!(".{name}.tmp-{}", std::process::id()));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(contents)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

/// Like [`write_atomic`] but leaves the file (and its mtime) untouched when
/// the contents are identical. Returns whether the file was written.
pub fn write_if_changed(path: &Path, contents: &[u8]) -> std::io::Result<bool> {
    if std::fs::read(path).is_ok_and(|old| old == contents) {
        return Ok(false);
    }
    write_atomic(path, contents)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    #[test]
    fn write_if_changed_skips_identical() {
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join("x");
        assert!(super::write_if_changed(&f, b"a").unwrap());
        assert!(!super::write_if_changed(&f, b"a").unwrap());
        assert!(super::write_if_changed(&f, b"b").unwrap());
    }

    #[test]
    fn writes_and_replaces() {
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join("a/b.txt");
        super::write_atomic(&f, b"one").unwrap();
        super::write_atomic(&f, b"two").unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "two");
        assert_eq!(std::fs::read_dir(f.parent().unwrap()).unwrap().count(), 1);
    }
}
