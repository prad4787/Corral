//! Laravel watcher events (queries, HTTP calls, logs, views, cache, jobs)
//! from the `corral/laravel` package: plain JSON lines, nothing is unserialized.

use crate::{JsonlStore, Record};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::io::AsyncBufReadExt;

pub const MAX_EVENTS: usize = 5000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    #[serde(default)]
    pub id: u64,
    pub kind: String,
    pub time: f64,
    #[serde(default)]
    pub request_id: Option<String>,
    #[serde(default)]
    pub request: serde_json::Value,
    #[serde(default)]
    pub data: serde_json::Value,
}

impl Record for Event {
    fn id(&self) -> u64 {
        self.id
    }
    fn set_id(&mut self, id: u64) {
        self.id = id;
    }
}

pub type EventStore = JsonlStore<Event>;

pub fn socket_path(root: &Path) -> PathBuf {
    root.join("run/events.sock")
}
pub fn store_file(root: &Path) -> PathBuf {
    root.join("dumps/events.jsonl")
}
pub fn store(root: &Path) -> EventStore {
    JsonlStore::with_capacity(store_file(root), MAX_EVENTS)
}

/// The `corral/laravel` package sources, written to `php/corral-laravel/`
/// so projects can require it from a Composer path repository.
pub const LARAVEL_PACKAGE: &[(&str, &str)] = &[
    (
        "composer.json",
        include_str!("../../../php/corral-laravel/composer.json"),
    ),
    (
        "README.md",
        include_str!("../../../php/corral-laravel/README.md"),
    ),
    (
        "src/Client.php",
        include_str!("../../../php/corral-laravel/src/Client.php"),
    ),
    (
        "src/CorralServiceProvider.php",
        include_str!("../../../php/corral-laravel/src/CorralServiceProvider.php"),
    ),
];

pub fn laravel_package_dir(root: &Path) -> PathBuf {
    root.join("php/corral-laravel")
}

/// Write (or refresh) the package. Returns its directory.
pub fn write_laravel_package(root: &Path) -> std::io::Result<PathBuf> {
    let dir = laravel_package_dir(root);
    for (rel, body) in LARAVEL_PACKAGE {
        let p = dir.join(rel);
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        if std::fs::read_to_string(&p).ok().as_deref() != Some(*body) {
            std::fs::write(&p, body)?;
        }
    }
    Ok(dir)
}

const KINDS: &[&str] = &["query", "http", "log", "view", "cache", "job"];

pub async fn serve(listener: tokio::net::UnixListener, store: EventStore) -> std::io::Result<()> {
    loop {
        let (stream, _) = listener.accept().await?;
        let store = store.clone();
        tokio::spawn(async move {
            let mut lines = tokio::io::BufReader::new(stream).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if line.len() > 1 << 20 {
                    continue;
                }
                match serde_json::from_str::<Event>(&line) {
                    Ok(e) if KINDS.contains(&e.kind.as_str()) => {
                        if let Err(err) = store.push(e) {
                            tracing::error!(error = %err, "could not store event");
                        }
                    }
                    Ok(e) => tracing::debug!(kind = %e.kind, "unknown event kind"),
                    Err(err) => tracing::debug!(error = %err, "invalid event"),
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn receives_known_kinds_only() {
        let t = tempfile::tempdir().unwrap();
        let sock = t.path().join("run/events.sock");
        let store = store(t.path());
        tokio::spawn(serve(crate::bind(&sock).unwrap(), store.clone()));
        let mut c = tokio::net::UnixStream::connect(&sock).await.unwrap();
        c.write_all(
            br#"{"kind":"query","time":1.5,"request_id":"ab","request":{"uri":"/"},"data":{"sql":"select 1","time_ms":0.4}}
{"kind":"evil","time":1}
{"kind":"http","time":2,"data":{"url":"https://api.test","status":200}}
"#,
        )
        .await
        .unwrap();
        drop(c);
        for _ in 0..100 {
            if store.list(10).len() == 2 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        let got = store.list(10);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].kind, "http");
        assert_eq!(got[1].data["sql"], "select 1");
        assert_eq!(got[1].id, 1);
    }
}
