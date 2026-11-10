//! Dump viewer backend.
//!
//! Corral's prepend file (`prepend.php`) switches Symfony VarDumper into its
//! built-in dump-server mode pointed at a 0600 unix socket. Each dump arrives
//! as one line, `base64(serialize([Data, context]))`; `render.php` (run with
//! Corral's PHP and the project's own VarDumper) turns it into HTML + text.
//! The newest [`MAX_DUMPS`] are kept in `dumps/dumps.jsonl`.

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::io::AsyncBufReadExt;

pub const MAX_DUMPS: usize = 500;
pub mod events;
const PREPEND: &str = include_str!("prepend.php");
const RENDER: &str = include_str!("render.php");

pub fn render_php() -> &'static str {
    RENDER
}
pub fn render_path(root: &Path) -> PathBuf {
    root.join("php/corral-dump-render.php")
}

/// How payloads are rendered.
#[derive(Debug, Clone)]
pub struct Renderer {
    pub php: PathBuf,
    pub script: PathBuf,
}

impl Renderer {
    /// Render one dump-server payload line into a [`Dump`].
    pub async fn render(&self, line: &str) -> anyhow::Result<Dump> {
        use tokio::io::AsyncWriteExt;
        let mut child = tokio::process::Command::new(&self.php)
            .args(["-d", "auto_prepend_file=", "-d", "display_errors=stderr"])
            .arg(&self.script)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let mut stdin = child.stdin.take().expect("piped");
        stdin.write_all(line.as_bytes()).await?;
        drop(stdin);
        let out =
            tokio::time::timeout(std::time::Duration::from_secs(10), child.wait_with_output())
                .await
                .map_err(|_| anyhow::anyhow!("renderer timed out"))??;
        if !out.status.success() {
            anyhow::bail!(
                "renderer failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(serde_json::from_slice(&out.stdout)?)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Dump {
    #[serde(default)]
    pub id: u64,
    pub time: f64,
    pub label: Option<String>,
    pub html: String,
    pub text: String,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub sapi: Option<String>,
    pub host: Option<String>,
    pub method: Option<String>,
    pub uri: Option<String>,
}

/// The prepend file for `socket`. Single quotes in the path are escaped for
/// the PHP string literal.
pub fn prepend_php(socket: &Path) -> String {
    let lit = |p: &Path| {
        p.display()
            .to_string()
            .replace('\\', "\\\\")
            .replace('\'', "\\'")
    };
    let events = socket.with_file_name("events.sock");
    PREPEND
        .replace("__CORRAL_DUMP_SOCKET__", &lit(socket))
        .replace("__CORRAL_EVENTS_SOCKET__", &lit(&events))
}

pub fn prepend_path(root: &Path) -> PathBuf {
    root.join("php/corral-prepend.php")
}
pub fn socket_path(root: &Path) -> PathBuf {
    root.join("run/dumps.sock")
}
pub fn store_file(root: &Path) -> PathBuf {
    root.join("dumps/dumps.jsonl")
}

/// Something stored in a [`JsonlStore`]: carries a sequential id.
pub trait Record: Serialize + serde::de::DeserializeOwned {
    fn id(&self) -> u64;
    fn set_id(&mut self, id: u64);
}

impl Record for Dump {
    fn id(&self) -> u64 {
        self.id
    }
    fn set_id(&mut self, id: u64) {
        self.id = id;
    }
}

/// Append-only JSON-lines ring buffer keeping roughly the newest `max`
/// records (compacted when it reaches twice that, so pushes are O(1) amortized).
pub struct JsonlStore<T> {
    file: PathBuf,
    max: usize,
    /// (lines on disk, last id), loaded lazily on first push.
    state: Arc<Mutex<Option<(usize, u64)>>>,
    _t: std::marker::PhantomData<fn() -> T>,
}

impl<T> Clone for JsonlStore<T> {
    fn clone(&self) -> Self {
        Self {
            file: self.file.clone(),
            max: self.max,
            state: self.state.clone(),
            _t: std::marker::PhantomData,
        }
    }
}

/// The dump store.
pub type Store = JsonlStore<Dump>;

impl Store {
    pub fn new(file: impl Into<PathBuf>) -> Self {
        JsonlStore::with_capacity(file, MAX_DUMPS)
    }
}

impl<T: Record> JsonlStore<T> {
    pub fn with_capacity(file: impl Into<PathBuf>, max: usize) -> Self {
        Self {
            file: file.into(),
            max,
            state: Arc::new(Mutex::new(None)),
            _t: std::marker::PhantomData,
        }
    }

    fn read_all(&self) -> Vec<T> {
        std::fs::read_to_string(&self.file)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }

    /// Newest first.
    pub fn list(&self, limit: usize) -> Vec<T> {
        let mut v = self.read_all();
        v.reverse();
        v.truncate(limit);
        v
    }

    pub fn push(&self, mut d: T) -> anyhow::Result<T> {
        let mut st = self.state.lock().unwrap();
        if let Some(dir) = self.file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let (count, last) = *st.get_or_insert_with(|| {
            let all = self.read_all();
            (all.len(), all.last().map_or(0, Record::id))
        });
        d.set_id(last + 1);
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file)?;
        writeln!(f, "{}", serde_json::to_string(&d)?)?;
        let mut count = count + 1;
        if count >= 2 * self.max {
            let mut all = self.read_all();
            all.drain(..all.len().saturating_sub(self.max));
            let body: String = all
                .iter()
                .filter_map(|x| serde_json::to_string(x).ok())
                .map(|l| l + "\n")
                .collect();
            corral_write(&self.file, body.as_bytes())?;
            count = all.len();
        }
        *st = Some((count, d.id()));
        Ok(d)
    }

    pub fn clear(&self) -> std::io::Result<()> {
        let mut st = self.state.lock().unwrap();
        *st = Some((0, st.map_or(0, |s| s.1)));
        match std::fs::remove_file(&self.file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}

fn corral_write(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, data)?;
    std::fs::rename(tmp, path)
}

/// Bind the receiver socket (replacing a stale one) with mode 0600.
pub fn bind(socket: &Path) -> std::io::Result<tokio::net::UnixListener> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(d) = socket.parent() {
        std::fs::create_dir_all(d)?;
    }
    let _ = std::fs::remove_file(socket);
    let l = tokio::net::UnixListener::bind(socket)?;
    std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o600))?;
    Ok(l)
}

pub async fn serve(
    listener: tokio::net::UnixListener,
    store: Store,
    renderer: Renderer,
) -> std::io::Result<()> {
    loop {
        let (stream, _) = listener.accept().await?;
        let (store, renderer) = (store.clone(), renderer.clone());
        tokio::spawn(async move {
            let mut lines = tokio::io::BufReader::new(stream).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                // 16 MB cap keeps a runaway dump from filling the disk.
                if line.len() > 16 << 20 {
                    tracing::warn!(bytes = line.len(), "dump too large; dropped");
                    continue;
                }
                match renderer.render(&line).await {
                    Ok(d) => {
                        if let Err(e) = store.push(d) {
                            tracing::error!(error = %e, "could not store dump");
                        }
                    }
                    Err(e) => tracing::warn!(error = %e, "could not render dump"),
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    fn dump(text: &str) -> Dump {
        Dump {
            id: 0,
            time: 1.0,
            label: None,
            html: format!("<pre>{text}</pre>"),
            text: text.into(),
            file: Some("/app/routes/web.php".into()),
            line: Some(12),
            sapi: Some("fpm-fcgi".into()),
            host: Some("shop.test".into()),
            method: Some("GET".into()),
            uri: Some("/".into()),
        }
    }

    #[test]
    fn prepend_embeds_escaped_socket_path() {
        let p = prepend_php(Path::new(
            "/Users/o'neil/Library/Application Support/Corral/run/dumps.sock",
        ));
        assert!(p.contains(
            r"$sock = '/Users/o\'neil/Library/Application Support/Corral/run/dumps.sock';"
        ));
        assert!(!p.contains("__CORRAL_DUMP_SOCKET__"));
        assert!(p.starts_with("<?php"));
    }

    #[test]
    fn store_caps_and_orders() {
        let t = tempfile::tempdir().unwrap();
        let s = Store::new(t.path().join("d/dumps.jsonl"));
        for i in 0..(MAX_DUMPS + 5) {
            s.push(dump(&i.to_string())).unwrap();
        }
        let l = s.list(3);
        assert_eq!(
            l.iter().map(|d| d.text.as_str()).collect::<Vec<_>>(),
            ["504", "503", "502"]
        );
        assert!(l[0].id > l[1].id);
        for i in 0..MAX_DUMPS {
            s.push(dump(&format!("b{i}"))).unwrap();
        }
        let n = s.list(10_000).len();
        assert!(
            (MAX_DUMPS..2 * MAX_DUMPS).contains(&n),
            "compacted to newest, got {n}"
        );
        assert_eq!(s.list(1)[0].text, format!("b{}", MAX_DUMPS - 1));
        s.clear().unwrap();
        assert!(s.list(10).is_empty());
    }

    #[tokio::test]
    async fn receives_over_unix_socket_and_renders() {
        use std::os::unix::fs::PermissionsExt;
        let t = tempfile::tempdir().unwrap();
        let sock = t.path().join("run/dumps.sock");
        // Stand-in for PHP: echoes a Dump JSON whose text is the payload.
        let fake = t.path().join("fake-php");
        std::fs::write(
            &fake,
            "#!/bin/sh\nread p\nprintf '{\"time\":1,\"label\":null,\"html\":\"<b>%s</b>\",\"text\":\"%s\",\"file\":null,\"line\":null,\"sapi\":\"fpm-fcgi\",\"host\":null,\"method\":null,\"uri\":null}' \"$p\" \"$p\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let store = Store::new(t.path().join("dumps.jsonl"));
        let renderer = Renderer {
            php: fake,
            script: t.path().join("unused.php"),
        };
        let l = bind(&sock).unwrap();
        let mode = std::fs::metadata(&sock).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "only the owner may send dumps");
        tokio::spawn(serve(l, store.clone(), renderer));
        let mut c = tokio::net::UnixStream::connect(&sock).await.unwrap();
        c.write_all(b"aGVsbG8=\n").await.unwrap();
        drop(c);
        for _ in 0..100 {
            if !store.list(1).is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        let got = store.list(5);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].text, "aGVsbG8=");
    }
}
