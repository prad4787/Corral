//! Minimal `.env` access: read one key, or replace one line. Values are
//! never logged or printed by Corral.

use anyhow::Context;

/// Read one key from a dotenv file without printing anything else from it.
pub fn value(file: &std::path::Path, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(file).ok()?;
    text.lines().find_map(|l| {
        let l = l.trim();
        let rest = l.strip_prefix(key)?.trim_start().strip_prefix('=')?;
        let v = rest.split(" #").next().unwrap_or("").trim();
        let v = v.trim_matches(|c| c == '"' || c == '\'');
        (!v.is_empty()).then(|| v.to_string())
    })
}

/// Replace (or append) one `KEY=value` line in a dotenv file, keeping every
/// other byte as-is. The first change writes `<file>.corral-backup`.
pub fn set_value(file: &std::path::Path, key: &str, value: &str) -> anyhow::Result<()> {
    let text = std::fs::read_to_string(file).with_context(|| format!("read {}", file.display()))?;
    let backup = file.with_file_name(format!(
        "{}.corral-backup",
        file.file_name().and_then(|n| n.to_str()).unwrap_or(".env")
    ));
    if !backup.exists() {
        std::fs::copy(file, &backup)?;
    }
    let mut found = false;
    let mut out: Vec<String> = text
        .lines()
        .map(|l| {
            let is_key = l
                .trim_start()
                .strip_prefix(key)
                .is_some_and(|r| r.trim_start().starts_with('='));
            if is_key && !found {
                found = true;
                format!("{key}={value}")
            } else {
                l.to_string()
            }
        })
        .collect();
    if !found {
        out.push(format!("{key}={value}"));
    }
    let mut body = out.join("\n");
    if text.ends_with('\n') || !found {
        body.push('\n');
    }
    let perms = std::fs::metadata(file)?.permissions();
    crate::fsutil::write_atomic(file, body.as_bytes())?;
    std::fs::set_permissions(file, perms)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{set_value, value};

    #[test]
    fn set_dotenv_value_replaces_only_that_line() {
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join(".env");
        let orig = "APP_KEY=k\nDB_USERNAME=root\nDB_USERNAME_X=keep\nDB_PASSWORD=p\n";
        std::fs::write(&f, orig).unwrap();
        set_value(&f, "DB_USERNAME", "elbilling").unwrap();
        assert_eq!(
            std::fs::read_to_string(&f).unwrap(),
            "APP_KEY=k\nDB_USERNAME=elbilling\nDB_USERNAME_X=keep\nDB_PASSWORD=p\n"
        );
        let backup = t.path().join(".env.corral-backup");
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), orig);
        // A second change keeps the original backup.
        set_value(&f, "DB_USERNAME", "other").unwrap();
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), orig);
        set_value(&f, "NEW_KEY", "1").unwrap();
        assert!(
            std::fs::read_to_string(&f)
                .unwrap()
                .ends_with("NEW_KEY=1\n")
        );
    }

    #[test]
    fn reads_single_dotenv_key() {
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join(".env");
        std::fs::write(
            &f,
            "APP_KEY=base64:secret\nDB_DATABASE=\"el_billing\" # comment\nDB_DATABASE_TEST=x\n",
        )
        .unwrap();
        assert_eq!(value(&f, "DB_DATABASE").as_deref(), Some("el_billing"));
        assert_eq!(value(&f, "DB_DATABASE_TEST").as_deref(), Some("x"));
        assert_eq!(value(&f, "MISSING"), None);
    }
}
