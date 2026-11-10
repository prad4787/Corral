//! Shell PATH integration. Corral adds one clearly marked block to the user's
//! shell startup files and removes exactly that block on uninstall.

use crate::fsutil::write_atomic;
use std::path::{Path, PathBuf};

pub const BEGIN: &str = "# >>> corral >>>";
pub const END: &str = "# <<< corral <<<";

/// Startup files we manage, for the user's home directory. zsh is the macOS
/// default; bash and fish files are only touched if they already exist.
pub fn rc_files(home: &Path) -> Vec<PathBuf> {
    let mut v = vec![home.join(".zshrc")];
    for f in [".bash_profile", ".bashrc", ".config/fish/config.fish"] {
        let p = home.join(f);
        if p.exists() {
            v.push(p);
        }
    }
    v
}

fn block(rc: &Path, bin: &Path) -> String {
    let fish = rc.extension().is_some_and(|e| e == "fish");
    let line = if fish {
        format!("fish_add_path --prepend --global \"{}\"", bin.display())
    } else {
        format!("export PATH=\"{}:$PATH\"", bin.display())
    };
    format!(
        "{BEGIN}\n# Added by Corral. Remove with `corral uninstall` or delete this block.\n{line}\n{END}\n"
    )
}

/// Text with any Corral block removed (and the blank line we added before it).
pub fn strip_block(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut inside = false;
    for line in text.split_inclusive('\n') {
        let t = line.trim_end();
        if t == BEGIN {
            inside = true;
            if out.ends_with("\n\n") {
                out.pop();
            }
            continue;
        }
        if inside {
            if t == END {
                inside = false;
            }
            continue;
        }
        out.push_str(line);
    }
    out
}

pub fn has_block(rc: &Path) -> bool {
    std::fs::read_to_string(rc).is_ok_and(|s| s.lines().any(|l| l.trim_end() == BEGIN))
}

/// Add (or refresh) the block at the end of `rc` so Corral's bin wins over
/// earlier PATH entries. Returns true if the file changed.
pub fn install(rc: &Path, bin: &Path) -> std::io::Result<bool> {
    let old = std::fs::read_to_string(rc).unwrap_or_default();
    let mut new = strip_block(&old);
    if !new.is_empty() && !new.ends_with('\n') {
        new.push('\n');
    }
    if !new.is_empty() {
        new.push('\n');
    }
    new.push_str(&block(rc, bin));
    if new == old {
        return Ok(false);
    }
    write_atomic(rc, new.as_bytes())?;
    Ok(true)
}

/// Remove the block. Returns true if the file changed.
pub fn uninstall(rc: &Path) -> std::io::Result<bool> {
    let Ok(old) = std::fs::read_to_string(rc) else {
        return Ok(false);
    };
    let new = strip_block(&old);
    if new == old {
        return Ok(false);
    }
    write_atomic(rc, new.as_bytes())?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_is_idempotent_and_uninstall_restores() {
        let t = tempfile::tempdir().unwrap();
        let rc = t.path().join(".zshrc");
        let original =
            "export FOO=1\n# Another tool's PHP binary.\nexport PATH=\"/other/bin\":$PATH\n";
        std::fs::write(&rc, original).unwrap();
        let bin = Path::new("/Users/me/.corral/bin");
        assert!(install(&rc, bin).unwrap());
        assert!(!install(&rc, bin).unwrap(), "second install is a no-op");
        let text = std::fs::read_to_string(&rc).unwrap();
        assert!(text.ends_with(&format!(
            "export PATH=\"/Users/me/.corral/bin:$PATH\"\n{END}\n"
        )));
        assert_eq!(text.matches(BEGIN).count(), 1);
        assert!(has_block(&rc));
        assert!(uninstall(&rc).unwrap());
        assert_eq!(std::fs::read_to_string(&rc).unwrap(), original);
        assert!(!uninstall(&rc).unwrap());
    }

    #[test]
    fn fish_syntax_and_missing_file() {
        let t = tempfile::tempdir().unwrap();
        let fish = t.path().join("config.fish");
        install(&fish, Path::new("/b")).unwrap();
        assert!(
            std::fs::read_to_string(&fish)
                .unwrap()
                .contains("fish_add_path --prepend --global \"/b\"")
        );
        assert!(!uninstall(&t.path().join("nope")).unwrap());
    }

    #[test]
    fn strip_keeps_surrounding_text() {
        let s = format!("a\n\n{BEGIN}\nx\n{END}\nb\n");
        assert_eq!(strip_block(&s), "a\nb\n");
    }
}
