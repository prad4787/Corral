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

#[cfg(test)]
mod tests {
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
