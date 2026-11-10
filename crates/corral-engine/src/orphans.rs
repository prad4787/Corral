//! Single-instance lock and cleanup of children left behind by a run that
//! died without stopping them (SIGKILL, crash, power loss).
//!
//! Children run in their own process groups, so they survive the engine's
//! death. While running we record their PIDs in `run/children.pids`. On the
//! next start, after taking the exclusive engine lock, any recorded PID that is
//! still alive **and** whose command line references Corral's data directory is
//! an orphan: its group gets SIGTERM, then SIGKILL.

use corral_core::paths::Paths;
use std::fs::File;
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub fn pid_file(paths: &Paths) -> PathBuf {
    paths.run_dir().join("children.pids")
}

/// Exclusive lock held for the engine's lifetime. Dropping it releases the lock.
pub struct EngineLock {
    _file: File,
}

impl EngineLock {
    /// Take the lock, or fail if another engine holds it.
    pub fn acquire(paths: &Paths) -> anyhow::Result<Self> {
        std::fs::create_dir_all(paths.run_dir())?;
        let file = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(paths.run_dir().join("engine.lock"))?;
        // SAFETY: flock on a valid, owned fd.
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc != 0 {
            anyhow::bail!(
                "Corral is already running (another `corral start` or the app holds the engine lock)"
            );
        }
        Ok(Self { _file: file })
    }
}

fn alive(pid: i32) -> bool {
    // SAFETY: signal 0 only tests for existence/permission.
    unsafe { libc::kill(pid, 0) == 0 }
}

fn command_line(pid: i32) -> Option<String> {
    let out = std::process::Command::new("/bin/ps")
        .args(["-o", "command=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Record the PIDs of currently supervised children.
pub fn record(paths: &Paths, pids: &[u32]) -> std::io::Result<()> {
    let body: String = pids.iter().map(|p| format!("{p}\n")).collect();
    corral_core::fsutil::write_atomic(&pid_file(paths), body.as_bytes())
}

/// Kill orphaned children from a previous run. Returns the PIDs that were reaped.
/// Must only be called while holding [`EngineLock`].
pub fn reap(paths: &Paths) -> Vec<i32> {
    let Ok(body) = std::fs::read_to_string(pid_file(paths)) else {
        return Vec::new();
    };
    let marker = paths.root.display().to_string();
    let targets: Vec<i32> = body
        .lines()
        .filter_map(|l| l.trim().parse::<i32>().ok())
        .filter(|&p| p > 1 && alive(p))
        .filter(|&p| command_line(p).is_some_and(|c| c.contains(&marker)))
        .collect();
    for &p in &targets {
        tracing::warn!(pid = p, "stopping orphaned process from a previous run");
        // SAFETY: negative pid targets the process group the child leads.
        unsafe { libc::kill(-p, libc::SIGTERM) };
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    while targets.iter().any(|&p| alive(p)) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    for &p in targets.iter().filter(|&&p| alive(p)) {
        // SAFETY: as above.
        unsafe { libc::kill(-p, libc::SIGKILL) };
    }
    let _ = std::fs::remove_file(pid_file(paths));
    targets
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::CommandExt;

    fn spawn_group(arg0_marker: &str) -> std::process::Child {
        std::process::Command::new("/bin/sh")
            .args(["-c", "sleep 30; :", arg0_marker])
            .process_group(0)
            .spawn()
            .unwrap()
    }

    #[test]
    fn lock_is_exclusive() {
        let t = tempfile::tempdir().unwrap();
        let p = Paths::at(t.path());
        let a = EngineLock::acquire(&p).unwrap();
        assert!(EngineLock::acquire(&p).is_err());
        drop(a);
        assert!(EngineLock::acquire(&p).is_ok());
    }

    #[test]
    fn reaps_only_processes_that_belong_to_corral() {
        let t = tempfile::tempdir().unwrap();
        let p = Paths::at(t.path().join("corral-home"));
        p.ensure().unwrap();
        let mut ours = spawn_group(&p.root.join("tools/caddy").display().to_string());
        let mut foreign = spawn_group("/somewhere/else");
        record(&p, &[ours.id(), foreign.id(), 999_999]).unwrap();

        let reaped = reap(&p);
        assert_eq!(reaped, [ours.id() as i32]);
        assert!(ours.wait().is_ok());
        assert!(
            alive(foreign.id() as i32),
            "foreign process must be left alone"
        );
        assert!(!pid_file(&p).exists());
        foreign.kill().unwrap();
        foreign.wait().unwrap();
    }
}
