//! Laravel / Monolog log reading. Parses the default line format
//! `[2026-11-10 15:55:59] local.ERROR: message {"context"}` and attaches
//! following lines (stack traces) to the entry above them.
//!
//! Only the tail of a file is read (`max_bytes`), so multi-GB logs stay fast.

use serde::Serialize;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Debug,
    Info,
    Notice,
    Warning,
    Error,
    Critical,
    Alert,
    Emergency,
}

impl Level {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.to_ascii_lowercase().as_str() {
            "debug" => Self::Debug,
            "info" => Self::Info,
            "notice" => Self::Notice,
            "warning" | "warn" => Self::Warning,
            "error" => Self::Error,
            "critical" => Self::Critical,
            "alert" => Self::Alert,
            "emergency" => Self::Emergency,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    pub time: String,
    pub env: String,
    pub level: Level,
    pub message: String,
    /// Continuation lines (usually a stack trace), joined with '\n'.
    pub details: String,
}

/// `storage/logs/*.log` of a Laravel project, newest first.
pub fn log_files(project: &Path) -> Vec<PathBuf> {
    let mut files: Vec<(std::time::SystemTime, PathBuf)> =
        std::fs::read_dir(project.join("storage/logs"))
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "log"))
            .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
            .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0));
    files.into_iter().map(|(_, p)| p).collect()
}

/// Parse `[time] env.LEVEL: message` → Entry (details empty).
fn parse_head(line: &str) -> Option<Entry> {
    let rest = line.strip_prefix('[')?;
    let close = rest.find(']')?;
    let time = &rest[..close];
    // Cheap sanity check: starts with a 4-digit year.
    if time.len() < 10 || !time.as_bytes()[..4].iter().all(u8::is_ascii_digit) {
        return None;
    }
    let after = rest[close + 1..].strip_prefix(' ')?;
    let colon = after.find(": ")?;
    let (chan, message) = (&after[..colon], &after[colon + 2..]);
    let (env, lvl) = chan.rsplit_once('.')?;
    if env.contains(' ') {
        return None;
    }
    Some(Entry {
        time: time.to_string(),
        env: env.to_string(),
        level: Level::parse(lvl)?,
        message: message.to_string(),
        details: String::new(),
    })
}

pub fn parse(text: &str) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    for line in text.lines() {
        if let Some(e) = parse_head(line) {
            out.push(e);
        } else if let Some(last) = out.last_mut() {
            if !last.details.is_empty() {
                last.details.push('\n');
            }
            last.details.push_str(line);
        }
    }
    out
}

/// Read the last `max_bytes` of `file` and return up to `limit` entries at or
/// above `min_level`, newest first.
pub fn tail(
    file: &Path,
    max_bytes: u64,
    limit: usize,
    min_level: Option<Level>,
) -> std::io::Result<Vec<Entry>> {
    let mut f = std::fs::File::open(file)?;
    let len = f.metadata()?.len();
    let start = len.saturating_sub(max_bytes);
    f.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::with_capacity((len - start) as usize);
    f.read_to_end(&mut buf)?;
    let mut text = String::from_utf8_lossy(&buf).into_owned();
    if start > 0 {
        // Drop the partial first line.
        match text.find('\n') {
            Some(i) => text.drain(..=i),
            None => text.drain(..),
        };
    }
    let mut entries = parse(&text);
    if let Some(min) = min_level {
        entries.retain(|e| e.level >= min);
    }
    entries.reverse();
    entries.truncate(limit);
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "[2026-11-10 15:41:35] local.ERROR: SQLSTATE[HY000] [2002] Connection refused {\"exception\":\"x\"}\n\
[stacktrace]\n#0 /app/vendor/a.php(12): foo()\n#1 {main}\n\
[2026-11-10 15:42:00] local.INFO: User logged in {\"id\":1}\n\
[2026-11-10 15:43:00] production.WARNING: Slow query\n";

    #[test]
    fn parses_entries_and_stack_traces() {
        let e = parse(LOG);
        assert_eq!(e.len(), 3);
        assert_eq!(e[0].level, Level::Error);
        assert_eq!(e[0].env, "local");
        assert!(e[0].message.starts_with("SQLSTATE[HY000] [2002]"));
        assert_eq!(
            e[0].details,
            "[stacktrace]\n#0 /app/vendor/a.php(12): foo()\n#1 {main}"
        );
        assert_eq!(e[2].env, "production");
        assert_eq!(e[2].level, Level::Warning);
    }

    #[test]
    fn tail_filters_orders_and_skips_partial_line() {
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join("laravel.log");
        std::fs::write(&f, LOG).unwrap();
        let all = tail(&f, 1 << 20, 10, None).unwrap();
        assert_eq!(all[0].message, "Slow query", "newest first");
        let errs = tail(&f, 1 << 20, 10, Some(Level::Warning)).unwrap();
        assert_eq!(errs.len(), 2);
        // Only the last ~60 bytes: the first entry is cut and must not appear half-parsed.
        let small = tail(&f, 60, 10, None).unwrap();
        assert_eq!(small.len(), 1);
        assert_eq!(small[0].message, "Slow query");
    }

    #[test]
    fn finds_log_files_newest_first() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path().join("storage/logs");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("old.log"), "").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(d.join("laravel.log"), "").unwrap();
        std::fs::write(d.join(".gitignore"), "").unwrap();
        let f = log_files(t.path());
        assert_eq!(f.len(), 2);
        assert!(f[0].ends_with("laravel.log"));
    }
}
