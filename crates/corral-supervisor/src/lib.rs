//! Supervises long-running child processes (PHP-FPM pools, the web server,
//! databases).
//!
//! - Each child runs in its own process group, so stopping it also stops its
//!   workers.
//! - stdout and stderr are appended to the service's log file.
//! - Unexpected exits are restarted with exponential backoff. A service that
//!   crashes `max_restarts` times inside `restart_window` is marked `Failed`.
//! - Stop sends SIGTERM to the group, then SIGKILL after `stop_timeout`.

use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, Notify, watch};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    pub name: String,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: Option<PathBuf>,
    pub log_file: PathBuf,
    pub restart: RestartPolicy,
    pub stop_timeout: Duration,
    /// Signal sent to the process group to stop it (SIGKILL follows after
    /// `stop_timeout`). Defaults to SIGTERM.
    pub stop_signal: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartPolicy {
    pub enabled: bool,
    pub max_restarts: u32,
    pub restart_window: Duration,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
}

impl Default for RestartPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            max_restarts: 5,
            restart_window: Duration::from_secs(60),
            initial_backoff: Duration::from_millis(250),
            max_backoff: Duration::from_secs(10),
        }
    }
}

impl Spec {
    pub fn new(
        name: impl Into<String>,
        program: impl Into<PathBuf>,
        log_file: impl Into<PathBuf>,
    ) -> Self {
        Self {
            name: name.into(),
            program: program.into(),
            args: Vec::new(),
            env: Vec::new(),
            cwd: None,
            log_file: log_file.into(),
            restart: RestartPolicy::default(),
            stop_timeout: Duration::from_secs(5),
            stop_signal: libc::SIGTERM,
        }
    }
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }
    pub fn env(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        self.env.push((k.into(), v.into()));
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum State {
    Starting,
    Running { pid: u32 },
    Backoff { restarts: u32 },
    Stopped,
    Failed { reason: String },
}

#[derive(Debug, thiserror::Error)]
pub enum SupervisorError {
    #[error("service {0:?} is already running")]
    AlreadyRunning(String),
    #[error("service {0:?} is not managed")]
    Unknown(String),
}

struct Handle {
    state: watch::Receiver<State>,
    stop: Arc<Notify>,
    task: tokio::task::JoinHandle<()>,
}

#[derive(Clone, Default)]
pub struct Supervisor {
    inner: Arc<Mutex<HashMap<String, Handle>>>,
}

fn spawn(spec: &Spec) -> std::io::Result<Child> {
    if let Some(dir) = spec.log_file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&spec.log_file)?;
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args)
        .envs(spec.env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .process_group(0)
        .kill_on_drop(true);
    if let Some(cwd) = &spec.cwd {
        cmd.current_dir(cwd);
    }
    cmd.spawn()
}

fn signal_group(pid: u32, sig: libc::c_int) {
    // SAFETY: kill(2) with a negative pid targets the process group we created.
    unsafe {
        libc::kill(-(pid as libc::pid_t), sig);
    }
}

async fn terminate(child: &mut Child, timeout: Duration, sig: libc::c_int) {
    if let Some(pid) = child.id() {
        signal_group(pid, sig);
        if tokio::time::timeout(timeout, child.wait()).await.is_err() {
            signal_group(pid, libc::SIGKILL);
            let _ = child.wait().await;
        }
    }
}

async fn run(spec: Spec, state: watch::Sender<State>, stop: Arc<Notify>) {
    let mut crashes: Vec<Instant> = Vec::new();
    let mut backoff = spec.restart.initial_backoff;
    loop {
        let _ = state.send(State::Starting);
        let mut child = match spawn(&spec) {
            Ok(c) => c,
            Err(e) => {
                let _ = state.send(State::Failed {
                    reason: format!("spawn {}: {e}", spec.program.display()),
                });
                return;
            }
        };
        let pid = child.id().unwrap_or_default();
        let _ = state.send(State::Running { pid });
        tracing::info!(service = %spec.name, pid, "started");
        let started = Instant::now();

        let status = tokio::select! {
            s = child.wait() => s,
            _ = stop.notified() => {
                terminate(&mut child, spec.stop_timeout, spec.stop_signal).await;
                let _ = state.send(State::Stopped);
                tracing::info!(service = %spec.name, "stopped");
                return;
            }
        };
        let reason = match status {
            Ok(s) => format!("exited with {s}"),
            Err(e) => format!("wait failed: {e}"),
        };
        tracing::warn!(service = %spec.name, %reason, "exited unexpectedly");
        if !spec.restart.enabled {
            let _ = state.send(State::Failed { reason });
            return;
        }
        // A run that stayed up for a full window resets the backoff.
        if started.elapsed() >= spec.restart.restart_window {
            crashes.clear();
            backoff = spec.restart.initial_backoff;
        }
        let now = Instant::now();
        crashes.retain(|t| now.duration_since(*t) < spec.restart.restart_window);
        crashes.push(now);
        if crashes.len() as u32 > spec.restart.max_restarts {
            let _ = state.send(State::Failed {
                reason: format!(
                    "{reason}; gave up after {} restarts",
                    spec.restart.max_restarts
                ),
            });
            return;
        }
        let _ = state.send(State::Backoff {
            restarts: crashes.len() as u32,
        });
        tokio::select! {
            _ = tokio::time::sleep(backoff) => {}
            _ = stop.notified() => {
                let _ = state.send(State::Stopped);
                return;
            }
        }
        backoff = (backoff * 2).min(spec.restart.max_backoff);
    }
}

impl Supervisor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start a service. Fails if one with the same name is active.
    pub async fn start(&self, spec: Spec) -> Result<(), SupervisorError> {
        let mut map = self.inner.lock().await;
        if let Some(h) = map.get(&spec.name)
            && !h.task.is_finished()
        {
            return Err(SupervisorError::AlreadyRunning(spec.name));
        }
        let (tx, rx) = watch::channel(State::Starting);
        let stop = Arc::new(Notify::new());
        let name = spec.name.clone();
        let task = tokio::spawn(run(spec, tx, stop.clone()));
        map.insert(
            name,
            Handle {
                state: rx,
                stop,
                task,
            },
        );
        Ok(())
    }

    /// Stop a service and wait until it has exited.
    pub async fn stop(&self, name: &str) -> Result<(), SupervisorError> {
        let handle = self
            .inner
            .lock()
            .await
            .remove(name)
            .ok_or_else(|| SupervisorError::Unknown(name.into()))?;
        handle.stop.notify_one();
        let _ = handle.task.await;
        Ok(())
    }

    pub async fn restart(&self, spec: Spec) -> Result<(), SupervisorError> {
        match self.stop(&spec.name).await {
            Ok(()) | Err(SupervisorError::Unknown(_)) => {}
            Err(e) => return Err(e),
        }
        self.start(spec).await
    }

    /// Stop everything (used on app quit).
    pub async fn stop_all(&self) {
        let names: Vec<String> = self.inner.lock().await.keys().cloned().collect();
        for n in names {
            let _ = self.stop(&n).await;
        }
    }

    pub async fn state(&self, name: &str) -> Option<State> {
        self.inner
            .lock()
            .await
            .get(name)
            .map(|h| h.state.borrow().clone())
    }

    pub async fn states(&self) -> Vec<(String, State)> {
        let mut v: Vec<_> = self
            .inner
            .lock()
            .await
            .iter()
            .map(|(k, h)| (k.clone(), h.state.borrow().clone()))
            .collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    }

    /// Wait until `pred` holds for the service's state, or the timeout elapses.
    pub async fn wait_for(
        &self,
        name: &str,
        timeout: Duration,
        pred: impl Fn(&State) -> bool,
    ) -> Option<State> {
        let mut rx = self.inner.lock().await.get(name)?.state.clone();
        tokio::time::timeout(timeout, async {
            loop {
                let s = rx.borrow_and_update().clone();
                if pred(&s) {
                    return Some(s);
                }
                rx.changed().await.ok()?;
            }
        })
        .await
        .ok()
        .flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(name: &str, dir: &std::path::Path, script: &str) -> Spec {
        Spec::new(name, "/bin/sh", dir.join(format!("{name}.log"))).args(["-c", script])
    }

    fn running(s: &State) -> bool {
        matches!(s, State::Running { .. })
    }

    fn pid_alive(pid: u32) -> bool {
        // SAFETY: signal 0 only checks for existence.
        unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
    }

    #[tokio::test]
    async fn start_logs_and_stop_kills_group() {
        let t = tempfile::tempdir().unwrap();
        let sup = Supervisor::new();
        // The child spawns a grandchild; stopping must take down both.
        let spec = sh(
            "svc",
            t.path(),
            "echo hello-$FOO; sleep 30 & echo $! > child.pid; wait",
        )
        .env("FOO", "bar");
        let spec = Spec {
            cwd: Some(t.path().into()),
            ..spec
        };
        sup.start(spec.clone()).await.unwrap();
        assert!(
            sup.wait_for("svc", Duration::from_secs(5), running)
                .await
                .is_some()
        );
        assert!(matches!(
            sup.start(spec).await,
            Err(SupervisorError::AlreadyRunning(_))
        ));

        let pid_file = t.path().join("child.pid");
        let mut grandchild = None;
        for _ in 0..50 {
            if let Some(p) = std::fs::read_to_string(&pid_file)
                .ok()
                .and_then(|s| s.trim().parse().ok())
            {
                grandchild = Some(p);
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let grandchild: u32 = grandchild.expect("grandchild pid");
        assert!(pid_alive(grandchild));

        sup.stop("svc").await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            !pid_alive(grandchild),
            "grandchild must be killed with the group"
        );
        let log = std::fs::read_to_string(t.path().join("svc.log")).unwrap();
        assert!(log.contains("hello-bar"));
        assert_eq!(sup.state("svc").await, None);
    }

    #[tokio::test]
    async fn crash_loop_gives_up() {
        let t = tempfile::tempdir().unwrap();
        let sup = Supervisor::new();
        let mut spec = sh("crashy", t.path(), "echo run >> runs.txt; exit 3");
        spec.cwd = Some(t.path().into());
        spec.restart.max_restarts = 2;
        spec.restart.initial_backoff = Duration::from_millis(10);
        sup.start(spec).await.unwrap();
        let s = sup
            .wait_for("crashy", Duration::from_secs(5), |s| {
                matches!(s, State::Failed { .. })
            })
            .await
            .expect("should fail");
        let State::Failed { reason } = s else {
            unreachable!()
        };
        assert!(reason.contains("gave up after 2 restarts"), "{reason}");
        let runs = std::fs::read_to_string(t.path().join("runs.txt")).unwrap();
        assert_eq!(runs.lines().count(), 3, "initial run + 2 restarts");
    }

    #[tokio::test]
    async fn sigterm_ignored_falls_back_to_sigkill() {
        let t = tempfile::tempdir().unwrap();
        let sup = Supervisor::new();
        let mut spec = sh(
            "stubborn",
            t.path(),
            "trap '' TERM; while true; do sleep 0.1; done",
        );
        spec.stop_timeout = Duration::from_millis(300);
        sup.start(spec).await.unwrap();
        let Some(State::Running { pid }) = sup
            .wait_for("stubborn", Duration::from_secs(5), running)
            .await
        else {
            panic!("not running");
        };
        let t0 = Instant::now();
        sup.stop("stubborn").await.unwrap();
        assert!(t0.elapsed() < Duration::from_secs(3));
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!pid_alive(pid));
    }

    #[tokio::test]
    async fn missing_binary_fails_and_restart_works() {
        let t = tempfile::tempdir().unwrap();
        let sup = Supervisor::new();
        sup.start(Spec::new("nope", "/does/not/exist", t.path().join("n.log")))
            .await
            .unwrap();
        let s = sup
            .wait_for("nope", Duration::from_secs(2), |s| {
                matches!(s, State::Failed { .. })
            })
            .await;
        assert!(s.is_some());
        // A failed service can be started again under the same name.
        sup.restart(sh("nope", t.path(), "sleep 30")).await.unwrap();
        assert!(
            sup.wait_for("nope", Duration::from_secs(5), running)
                .await
                .is_some()
        );
        assert_eq!(sup.states().await.len(), 1);
        sup.stop_all().await;
        assert!(sup.states().await.is_empty());
    }
}
