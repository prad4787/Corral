//! macOS system integration, driven from the GUI.
//!
//! | Need            | How                                                         | Root?                         |
//! |-----------------|-------------------------------------------------------------|-------------------------------|
//! | `*.tld` DNS     | `/etc/resolver/<tld>` → Corral's resolver                   | one file copy (admin prompt)  |
//! | HTTPS trust     | Corral CA in the **user** trust settings (`security`)       | no (macOS asks for consent)   |
//! | ports 80/443    | launchd daemon with socket activation, job runs as the user | only launchd binds the ports  |
//!
//! All root work happens in a single `osascript … with administrator
//! privileges` call, so the user sees one standard macOS password dialog.
//! Nothing here ever stores or handles the password.

pub mod forward;

use corral_core::{Config, Paths};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const DAEMON_LABEL: &str = "dev.corral.ports";

pub fn daemon_plist_path() -> PathBuf {
    PathBuf::from("/Library/LaunchDaemons").join(format!("{DAEMON_LABEL}.plist"))
}

pub fn resolver_path(tld: &str) -> PathBuf {
    PathBuf::from("/etc/resolver").join(tld)
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Status {
    /// `/etc/resolver/<tld>` points at Corral's DNS.
    pub dns: bool,
    /// Another tool (e.g. Valet) owns the resolver file.
    pub dns_foreign: bool,
    /// Corral's CA is trusted for TLS.
    pub https: bool,
    /// launchd forwards 80/443 to Corral.
    pub ports: bool,
    /// Something other than Corral already listens on 127.0.0.1:80/443.
    pub ports_busy: bool,
}

impl Status {
    pub fn complete(&self) -> bool {
        self.dns && self.https && self.ports
    }
}

pub fn status(paths: &Paths, cfg: &Config) -> Status {
    let expected = corral_dns::resolver_file(cfg.dns_port);
    let current = std::fs::read_to_string(resolver_path(&cfg.tld)).ok();
    let dns = current.as_deref() == Some(expected.as_str());
    let ca = corral_tls::LocalCa::cert_path(&paths.certs_dir());
    // No -r anchor: succeeds only if macOS itself trusts the CA.
    let https = ca.exists()
        && Command::new("/usr/bin/security")
            .args(["verify-cert", "-q", "-p", "basic", "-c"])
            .arg(&ca)
            .output()
            .is_ok_and(|o| o.status.success());
    let plist = std::fs::read_to_string(daemon_plist_path()).unwrap_or_default();
    // Ours, and the forwarder binary it runs still exists (the app may have moved).
    let ours = plist.contains(DAEMON_LABEL) && plist_program(&plist).is_some_and(|p| p.is_file());
    let listening = std::net::TcpStream::connect_timeout(
        &"127.0.0.1:443".parse().unwrap(),
        std::time::Duration::from_millis(300),
    )
    .is_ok();
    Status {
        dns,
        dns_foreign: current.is_some() && !dns,
        https,
        ports: ours && listening,
        ports_busy: !ours && listening,
    }
}

/// The executable path in our daemon plist (first ProgramArguments string).
pub fn plist_program(plist: &str) -> Option<PathBuf> {
    let after = plist.split("<key>ProgramArguments</key>").nth(1)?;
    let start = after.find("<string>")? + "<string>".len();
    let end = after[start..].find("</string>")? + start;
    let raw = &after[start..end];
    Some(PathBuf::from(
        raw.replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&amp;", "&"),
    ))
}

/// POSIX shell single-quoting.
fn sq(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// AppleScript string literal.
fn applescript_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// launchd plist for the port daemon. The job runs as `user`; launchd binds
/// the sockets as root and hands them over.
pub fn daemon_plist(
    exe: &Path,
    user: &str,
    http_target: u16,
    https_target: u16,
    log: &Path,
) -> String {
    let x = |s: &str| {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    let sock = |port: u16| {
        format!(
            "<dict><key>SockNodeName</key><string>127.0.0.1</string>\
             <key>SockServiceName</key><string>{port}</string>\
             <key>SockType</key><string>stream</string>\
             <key>SockFamily</key><string>IPv4</string></dict>"
        )
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{label}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{exe}</string>
    <string>--port-forward</string>
    <string>{http_target}</string>
    <string>{https_target}</string>
  </array>
  <key>UserName</key><string>{user}</string>
  <key>Sockets</key>
  <dict>
    <key>http</key>{http}
    <key>https</key>{https}
  </dict>
  <key>StandardErrorPath</key><string>{log}</string>
</dict>
</plist>
"#,
        label = DAEMON_LABEL,
        exe = x(&exe.display().to_string()),
        user = x(user),
        log = x(&log.display().to_string()),
        http = sock(80),
        https = sock(443),
    )
}

/// The root part of setup as one shell script (run by osascript).
pub fn setup_script(tld: &str, resolver_src: &Path, plist_src: &Path) -> String {
    let r = resolver_path(tld);
    let p = daemon_plist_path();
    [
        "set -e".to_string(),
        "/bin/mkdir -p /etc/resolver".into(),
        format!(
            "/usr/bin/install -m 644 -o root -g wheel {} {}",
            sq(&resolver_src.display().to_string()),
            sq(&r.display().to_string())
        ),
        format!(
            "/usr/bin/plutil -lint {} >/dev/null",
            sq(&plist_src.display().to_string())
        ),
        format!("/bin/launchctl bootout system/{DAEMON_LABEL} 2>/dev/null || true"),
        format!(
            "/usr/bin/install -m 644 -o root -g wheel {} {}",
            sq(&plist_src.display().to_string()),
            sq(&p.display().to_string())
        ),
        format!(
            "/bin/launchctl bootstrap system {}",
            sq(&p.display().to_string())
        ),
    ]
    .join("; ")
}

/// The root part of undo. Leaves a resolver file alone unless Corral wrote it.
pub fn undo_script(tld: &str) -> String {
    let r = resolver_path(tld);
    [
        format!("/bin/launchctl bootout system/{DAEMON_LABEL} 2>/dev/null || true"),
        format!(
            "/bin/rm -f {}",
            sq(&daemon_plist_path().display().to_string())
        ),
        format!(
            "if /usr/bin/grep -q 'Generated by Corral' {r} 2>/dev/null; then /bin/rm -f {r}; fi",
            r = sq(&r.display().to_string())
        ),
    ]
    .join("; ")
}

/// Run `script` as root behind the standard macOS administrator dialog.
fn run_as_admin(script: &str, prompt: &str) -> anyhow::Result<()> {
    let apple = format!(
        "do shell script {} with prompt {} with administrator privileges",
        applescript_str(script),
        applescript_str(prompt)
    );
    let out = Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(apple)
        .output()?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        if err.contains("-128") {
            anyhow::bail!("cancelled");
        }
        anyhow::bail!("system setup failed: {}", err.trim());
    }
    Ok(())
}

/// Trust Corral's CA in the user's trust settings. macOS shows its own
/// confirmation dialog; no root needed.
fn trust_ca(ca: &Path) -> anyhow::Result<()> {
    let login = home().join("Library/Keychains/login.keychain-db");
    let out = Command::new("/usr/bin/security")
        .args([
            "add-trusted-cert",
            "-r",
            "trustRoot",
            "-p",
            "ssl",
            "-p",
            "basic",
            "-k",
        ])
        .arg(&login)
        .arg(ca)
        .output()?;
    anyhow::ensure!(
        out.status.success(),
        "trusting the certificate was not completed: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    );
    Ok(())
}

fn untrust_ca(ca: &Path) {
    let _ = Command::new("/usr/bin/security")
        .arg("remove-trusted-cert")
        .arg(ca)
        .output();
    let _ = Command::new("/usr/bin/security")
        .args(["delete-certificate", "-c", "Corral Local CA"])
        .output();
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

/// Do everything that is not done yet. `exe` is the binary launchd runs with
/// `--port-forward` (the app itself). Updates config to the public ports.
pub fn setup(paths: &Paths, exe: &Path) -> anyhow::Result<Status> {
    let mut cfg = Config::load(paths)?;
    // The CA must exist before it can be trusted.
    let ca = corral_tls::LocalCa::load_or_create(paths, &cfg.tld)?;
    let before = status(paths, &cfg);
    anyhow::ensure!(
        before.ports || !before.ports_busy,
        "another app is using ports 80/443 (quit Valet, MAMP or Docker first)"
    );
    if !(before.dns && before.ports) {
        let staging = paths.run_dir().join("system-setup");
        std::fs::create_dir_all(&staging)?;
        let resolver = staging.join("resolver");
        std::fs::write(&resolver, corral_dns::resolver_file(cfg.dns_port))?;
        let plist = staging.join("daemon.plist");
        let user = std::env::var("USER").unwrap_or_default();
        anyhow::ensure!(
            !user.is_empty() && user != "root",
            "cannot determine the current user"
        );
        std::fs::write(
            &plist,
            daemon_plist(
                exe,
                &user,
                cfg.internal_http_port,
                cfg.internal_https_port,
                &paths.logs_dir().join("port-forward.log"),
            ),
        )?;
        let result = run_as_admin(
            &setup_script(&cfg.tld, &resolver, &plist),
            "Corral wants to set up .test domains and ports 80/443 for local sites.",
        );
        let _ = std::fs::remove_dir_all(&staging);
        result?;
    }
    if !before.https {
        trust_ca(&corral_tls::LocalCa::cert_path(&paths.certs_dir()))?;
    }
    let _ = ca;
    cfg.http_port = 80;
    cfg.https_port = 443;
    cfg.save(paths)?;
    Ok(status(paths, &cfg))
}

/// Reverse [`setup`]. Switches Corral back to its high ports.
pub fn undo(paths: &Paths) -> anyhow::Result<Status> {
    let mut cfg = Config::load(paths)?;
    let st = status(paths, &cfg);
    let daemon_installed = daemon_plist_path().exists();
    if st.dns || daemon_installed {
        run_as_admin(
            &undo_script(&cfg.tld),
            "Corral wants to remove its .test domain and port settings.",
        )?;
    }
    untrust_ca(&corral_tls::LocalCa::cert_path(&paths.certs_dir()));
    cfg.http_port = cfg.internal_http_port;
    cfg.https_port = cfg.internal_https_port;
    cfg.save(paths)?;
    Ok(status(paths, &cfg))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting() {
        assert_eq!(sq("/a b/it's"), r"'/a b/it'\''s'");
        assert_eq!(applescript_str(r#"say "hi" \ ok"#), r#""say \"hi\" \\ ok""#);
    }

    #[test]
    fn plist_runs_as_user_with_socket_activation() {
        let p = daemon_plist(
            Path::new("/Applications/Corral.app/Contents/MacOS/corral-app"),
            "pradip",
            18080,
            18443,
            Path::new("/Users/pradip/Library/Application Support/Corral/logs/port-forward.log"),
        );
        assert!(p.contains("<key>UserName</key><string>pradip</string>"));
        assert!(p.contains("<string>--port-forward</string>"));
        assert_eq!(
            plist_program(&p).unwrap(),
            PathBuf::from("/Applications/Corral.app/Contents/MacOS/corral-app")
        );
        assert!(p.contains("<key>SockServiceName</key><string>80</string>"));
        assert!(p.contains("<key>SockServiceName</key><string>443</string>"));
        assert_eq!(
            p.matches("<string>127.0.0.1</string>").count(),
            2,
            "loopback only"
        );
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join("p.plist");
        std::fs::write(&f, &p).unwrap();
        let lint = Command::new("/usr/bin/plutil")
            .arg("-lint")
            .arg(&f)
            .output()
            .unwrap();
        assert!(
            lint.status.success(),
            "{}",
            String::from_utf8_lossy(&lint.stdout)
        );
    }

    #[test]
    fn scripts_are_scoped() {
        let s = setup_script(
            "test",
            Path::new("/tmp/a b/resolver"),
            Path::new("/tmp/a b/d.plist"),
        );
        assert!(s.contains("'/tmp/a b/resolver' '/etc/resolver/test'"));
        assert!(s.contains(
            "launchctl bootstrap system '/Library/LaunchDaemons/dev.corral.ports.plist'"
        ));
        let u = undo_script("test");
        assert!(
            u.contains("grep -q 'Generated by Corral' '/etc/resolver/test'"),
            "never removes foreign resolver files"
        );
    }
}
