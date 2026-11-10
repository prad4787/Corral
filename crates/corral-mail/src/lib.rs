//! Local mail catcher. A minimal SMTP server on 127.0.0.1 accepts every
//! message (any sender, any recipient, any AUTH) and stores it instead of
//! delivering it. Point an app at it with `MAIL_HOST=127.0.0.1 MAIL_PORT=2525`.
//!
//! Storage: `mail/<id>.eml` (raw RFC 5322) + `mail/<id>.json` (summary), so
//! listing never re-parses messages. The newest [`MAX_MESSAGES`] are kept.

use mail_parser::MimeHeaders;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

pub const MAX_MESSAGES: usize = 1000;
pub const MAX_SIZE: usize = 30 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    pub id: String,
    /// Unix seconds when Corral received it.
    pub received: u64,
    pub from: String,
    pub to: Vec<String>,
    pub subject: String,
    pub size: usize,
    pub attachments: usize,
    /// SMTP envelope (may differ from headers, e.g. Bcc).
    pub envelope_from: String,
    pub envelope_to: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Attachment {
    pub index: usize,
    pub name: String,
    pub content_type: String,
    pub size: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Detail {
    #[serde(flatten)]
    pub summary: Summary,
    pub cc: Vec<String>,
    pub date: Option<String>,
    pub html: Option<String>,
    pub text: Option<String>,
    pub headers: Vec<(String, String)>,
    pub attachment_list: Vec<Attachment>,
}

#[derive(Debug, Clone)]
pub struct Store {
    dir: PathBuf,
}

fn fmt_mailbox(name: Option<&str>, addr: Option<&str>) -> String {
    match (name.filter(|n| !n.is_empty()), addr) {
        (Some(n), Some(a)) => format!("{n} <{a}>"),
        (None, Some(a)) => a.to_string(),
        (Some(n), None) => n.to_string(),
        (None, None) => String::new(),
    }
}

fn list_of(l: Option<&mail_parser::Address<'_>>) -> Vec<String> {
    l.map(|l| {
        l.iter()
            .map(|m| fmt_mailbox(m.name(), m.address()))
            .collect()
    })
    .unwrap_or_default()
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 40 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

impl Store {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn path(&self, id: &str, ext: &str) -> anyhow::Result<PathBuf> {
        anyhow::ensure!(valid_id(id), "invalid message id");
        Ok(self.dir.join(format!("{id}.{ext}")))
    }

    /// Store a raw message; returns its summary.
    pub fn save(
        &self,
        raw: &[u8],
        envelope_from: &str,
        envelope_to: &[String],
    ) -> anyhow::Result<Summary> {
        std::fs::create_dir_all(&self.dir)?;
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?;
        // Sortable, unique enough for one local server.
        let id = format!(
            "{:016x}-{:04x}",
            now.as_nanos() as u64,
            std::process::id() & 0xffff
        );
        let parsed = mail_parser::MessageParser::new().parse(raw);
        let (from, to, subject, attachments) = match &parsed {
            Some(m) => (
                list_of(m.from()).into_iter().next().unwrap_or_default(),
                list_of(m.to()),
                m.subject().unwrap_or("").to_string(),
                m.attachments().count(),
            ),
            None => (String::new(), Vec::new(), String::new(), 0),
        };
        let summary = Summary {
            id: id.clone(),
            received: now.as_secs(),
            from: if from.is_empty() {
                envelope_from.to_string()
            } else {
                from
            },
            to: if to.is_empty() {
                envelope_to.to_vec()
            } else {
                to
            },
            subject,
            size: raw.len(),
            attachments,
            envelope_from: envelope_from.to_string(),
            envelope_to: envelope_to.to_vec(),
        };
        std::fs::write(self.path(&id, "eml")?, raw)?;
        std::fs::write(self.path(&id, "json")?, serde_json::to_vec(&summary)?)?;
        self.prune(MAX_MESSAGES)?;
        Ok(summary)
    }

    fn ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = std::fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let n = e.file_name().into_string().ok()?;
                n.strip_suffix(".json")
                    .filter(|i| valid_id(i))
                    .map(str::to_owned)
            })
            .collect();
        ids.sort_unstable_by(|a, b| b.cmp(a));
        ids
    }

    /// Newest first.
    pub fn list(&self, limit: usize) -> Vec<Summary> {
        self.ids()
            .into_iter()
            .take(limit)
            .filter_map(|id| {
                let p = self.path(&id, "json").ok()?;
                serde_json::from_slice(&std::fs::read(p).ok()?).ok()
            })
            .collect()
    }

    pub fn count(&self) -> usize {
        self.ids().len()
    }

    pub fn raw(&self, id: &str) -> anyhow::Result<Vec<u8>> {
        Ok(std::fs::read(self.path(id, "eml")?)?)
    }

    pub fn detail(&self, id: &str) -> anyhow::Result<Detail> {
        let summary: Summary = serde_json::from_slice(&std::fs::read(self.path(id, "json")?)?)?;
        let raw = self.raw(id)?;
        let m = mail_parser::MessageParser::new()
            .parse(&raw)
            .ok_or_else(|| anyhow::anyhow!("message could not be parsed"))?;
        let headers = m
            .headers()
            .iter()
            .map(|h| {
                let v = raw
                    .get(h.offset_start as usize..h.offset_end as usize)
                    .unwrap_or_default();
                (
                    h.name.as_str().to_string(),
                    String::from_utf8_lossy(v).trim().to_string(),
                )
            })
            .collect();
        let attachment_list = m
            .attachments()
            .enumerate()
            .map(|(index, a)| Attachment {
                index,
                name: a.attachment_name().unwrap_or("attachment").to_string(),
                content_type: a
                    .content_type()
                    .map(|c| match c.subtype() {
                        Some(s) => format!("{}/{s}", c.ctype()),
                        None => c.ctype().to_string(),
                    })
                    .unwrap_or_else(|| "application/octet-stream".into()),
                size: a.contents().len(),
            })
            .collect();
        Ok(Detail {
            cc: list_of(m.cc()),
            date: m.date().map(|d| d.to_rfc3339()),
            html: m.body_html(0).map(|s| s.into_owned()),
            text: m.body_text(0).map(|s| s.into_owned()),
            headers,
            attachment_list,
            summary,
        })
    }

    /// (file name, bytes) of attachment `index`.
    pub fn attachment(&self, id: &str, index: usize) -> anyhow::Result<(String, Vec<u8>)> {
        let raw = self.raw(id)?;
        let m = mail_parser::MessageParser::new()
            .parse(&raw)
            .ok_or_else(|| anyhow::anyhow!("message could not be parsed"))?;
        let a = m
            .attachments()
            .nth(index)
            .ok_or_else(|| anyhow::anyhow!("no attachment {index}"))?;
        let name = a
            .attachment_name()
            .unwrap_or("attachment")
            .replace(['/', '\\'], "_");
        Ok((name, a.contents().to_vec()))
    }

    pub fn delete(&self, id: &str) -> anyhow::Result<()> {
        for ext in ["eml", "json"] {
            match std::fs::remove_file(self.path(id, ext)?) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
                _ => {}
            }
        }
        Ok(())
    }

    pub fn clear(&self) -> anyhow::Result<usize> {
        let ids = self.ids();
        for id in &ids {
            self.delete(id)?;
        }
        Ok(ids.len())
    }

    fn prune(&self, keep: usize) -> anyhow::Result<()> {
        for id in self.ids().into_iter().skip(keep) {
            self.delete(&id)?;
        }
        Ok(())
    }
}

/// Extract the address from `MAIL FROM:<a@b> SIZE=…` / `RCPT TO:<x@y>`.
fn path_arg(rest: &str) -> String {
    let r = rest.trim();
    match (r.find('<'), r.find('>')) {
        (Some(a), Some(b)) if b > a => r[a + 1..b].to_string(),
        _ => r.split_whitespace().next().unwrap_or("").to_string(),
    }
}

async fn session(stream: tokio::net::TcpStream, store: Store) -> anyhow::Result<()> {
    let (r, mut w) = stream.into_split();
    let mut r = BufReader::new(r);
    w.write_all(b"220 corral.local ESMTP Corral mail catcher\r\n")
        .await?;
    let (mut from, mut rcpts): (Option<String>, Vec<String>) = (None, Vec::new());
    let mut line = String::new();
    loop {
        line.clear();
        if r.read_line(&mut line).await? == 0 {
            return Ok(());
        }
        let cmd = line.trim_end_matches(['\r', '\n']);
        let upper = cmd.to_ascii_uppercase();
        let reply: &[u8] = if upper.starts_with("EHLO") {
            w.write_all(b"250-corral.local\r\n250-PIPELINING\r\n250-8BITMIME\r\n250-SMTPUTF8\r\n")
                .await?;
            w.write_all(format!("250-SIZE {MAX_SIZE}\r\n").as_bytes())
                .await?;
            b"250 AUTH PLAIN LOGIN\r\n"
        } else if upper.starts_with("HELO") {
            b"250 corral.local\r\n"
        } else if upper.starts_with("AUTH") {
            // Accept any credentials; LOGIN needs two challenge round-trips.
            if upper.starts_with("AUTH LOGIN") {
                let given_user = cmd.split_whitespace().nth(2).is_some();
                if !given_user {
                    w.write_all(b"334 VXNlcm5hbWU6\r\n").await?;
                    line.clear();
                    r.read_line(&mut line).await?;
                }
                w.write_all(b"334 UGFzc3dvcmQ6\r\n").await?;
                line.clear();
                r.read_line(&mut line).await?;
            } else if upper.trim() == "AUTH PLAIN" {
                w.write_all(b"334 \r\n").await?;
                line.clear();
                r.read_line(&mut line).await?;
            }
            b"235 2.7.0 Authentication accepted\r\n"
        } else if upper.starts_with("MAIL FROM:") {
            from = Some(path_arg(&cmd[10..]));
            rcpts.clear();
            b"250 2.1.0 OK\r\n"
        } else if upper.starts_with("RCPT TO:") {
            if from.is_none() {
                b"503 5.5.1 MAIL first\r\n"
            } else {
                rcpts.push(path_arg(&cmd[8..]));
                b"250 2.1.5 OK\r\n"
            }
        } else if upper == "DATA" {
            if from.is_none() || rcpts.is_empty() {
                b"503 5.5.1 MAIL and RCPT first\r\n"
            } else {
                w.write_all(b"354 End data with <CR><LF>.<CR><LF>\r\n")
                    .await?;
                let mut data: Vec<u8> = Vec::new();
                let mut too_big = false;
                let mut buf = Vec::new();
                loop {
                    buf.clear();
                    if r.read_until(b'\n', &mut buf).await? == 0 {
                        return Ok(());
                    }
                    if buf == b".\r\n" || buf == b".\n" {
                        break;
                    }
                    // Dot-unstuffing (RFC 5321 4.5.2).
                    let chunk = if buf.starts_with(b"..") {
                        &buf[1..]
                    } else {
                        &buf[..]
                    };
                    if data.len() + chunk.len() > MAX_SIZE {
                        too_big = true;
                    } else {
                        data.extend_from_slice(chunk);
                    }
                }
                let f = from.take().unwrap_or_default();
                let to = std::mem::take(&mut rcpts);
                if too_big {
                    b"552 5.3.4 Message too big\r\n"
                } else {
                    match store.save(&data, &f, &to) {
                        Ok(s) => {
                            tracing::info!(id = %s.id, subject = %s.subject, "mail caught");
                            b"250 2.0.0 OK: caught by Corral\r\n"
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "could not store mail");
                            b"451 4.3.0 Could not store message\r\n"
                        }
                    }
                }
            }
        } else if upper == "RSET" {
            from = None;
            rcpts.clear();
            b"250 2.0.0 OK\r\n"
        } else if upper == "NOOP" {
            b"250 2.0.0 OK\r\n"
        } else if upper == "QUIT" {
            w.write_all(b"221 2.0.0 Bye\r\n").await?;
            return Ok(());
        } else if upper.starts_with("STARTTLS") {
            b"454 4.7.0 TLS not available\r\n"
        } else {
            b"502 5.5.2 Command not implemented\r\n"
        };
        w.write_all(reply).await?;
    }
}

/// Accept connections forever.
pub async fn serve(listener: TcpListener, store: Store) -> std::io::Result<()> {
    loop {
        let (stream, peer) = listener.accept().await?;
        let store = store.clone();
        tokio::spawn(async move {
            if let Err(e) = session(stream, store).await {
                tracing::debug!(%peer, error = %e, "smtp session ended");
            }
        });
    }
}

pub fn store_dir(root: &Path) -> PathBuf {
    root.join("mail")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const MSG: &str = "From: Shop <shop@example.test>\r\nTo: Ann <ann@example.test>\r\n\
Subject: Your order\r\nMIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=\"b\"\r\n\r\n\
--b\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<p>Hello <b>Ann</b></p>\r\n\
..leading dot line\r\n\
--b\r\nContent-Type: text/plain; name=\"invoice.txt\"\r\n\
Content-Disposition: attachment; filename=\"invoice.txt\"\r\n\r\nTOTAL 42\r\n--b--\r\n";

    async fn client(addr: std::net::SocketAddr, script: &[&str]) -> String {
        let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
        let mut out = String::new();
        let mut buf = [0u8; 4096];
        let n = s.read(&mut buf).await.unwrap();
        out.push_str(&String::from_utf8_lossy(&buf[..n]));
        for cmd in script {
            s.write_all(cmd.as_bytes()).await.unwrap();
            let n = s.read(&mut buf).await.unwrap();
            out.push_str(&String::from_utf8_lossy(&buf[..n]));
        }
        out
    }

    #[tokio::test]
    async fn catches_mail_over_smtp_with_auth() {
        let t = tempfile::tempdir().unwrap();
        let store = Store::new(t.path());
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(serve(l, store.clone()));
        let data = format!("{MSG}.\r\n");
        let out = client(
            addr,
            &[
                "EHLO app\r\n",
                "AUTH PLAIN dXNlcgB1c2VyAHBhc3M=\r\n",
                "MAIL FROM:<bounce@example.test> SIZE=100\r\n",
                "RCPT TO:<ann@example.test>\r\n",
                "RCPT TO:<hidden@example.test>\r\n",
                "DATA\r\n",
                &data,
                "QUIT\r\n",
            ],
        )
        .await;
        assert!(out.contains("235"), "{out}");
        assert!(out.contains("caught by Corral"), "{out}");

        let list = store.list(10);
        assert_eq!(list.len(), 1);
        let s = &list[0];
        assert_eq!(s.subject, "Your order");
        assert_eq!(s.from, "Shop <shop@example.test>");
        assert_eq!(s.to, ["Ann <ann@example.test>"]);
        assert_eq!(s.envelope_to, ["ann@example.test", "hidden@example.test"]);
        assert_eq!(s.attachments, 1);

        let d = store.detail(&s.id).unwrap();
        assert!(d.html.as_deref().unwrap().contains("<b>Ann</b>"));
        assert!(
            d.html.as_deref().unwrap().contains("\r\n.leading dot line"),
            "dot-unstuffed"
        );
        assert_eq!(d.attachment_list[0].name, "invoice.txt");
        let (name, bytes) = store.attachment(&s.id, 0).unwrap();
        assert_eq!(
            (name.as_str(), bytes.as_slice()),
            ("invoice.txt", b"TOTAL 42".as_slice())
        );
        assert!(
            d.headers
                .iter()
                .any(|(k, v)| k == "Subject" && v == "Your order")
        );
    }

    #[tokio::test]
    async fn rejects_out_of_order_commands_and_login_auth_works() {
        let t = tempfile::tempdir().unwrap();
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(serve(l, Store::new(t.path())));
        let out = client(
            addr,
            &[
                "HELO x\r\n",
                "RCPT TO:<a@b>\r\n",
                "DATA\r\n",
                "AUTH LOGIN\r\n",
                "dXNlcg==\r\n",
                "cGFzcw==\r\n",
                "FOO\r\n",
            ],
        )
        .await;
        assert!(out.matches("503").count() >= 2, "{out}");
        assert!(
            out.contains("334 VXNlcm5hbWU6") && out.contains("235"),
            "{out}"
        );
        assert!(out.contains("502"), "{out}");
    }

    #[test]
    fn prune_delete_clear_and_ids() {
        let t = tempfile::tempdir().unwrap();
        let store = Store::new(t.path());
        for i in 0..5 {
            store
                .save(
                    format!("Subject: m{i}\r\n\r\nx").as_bytes(),
                    "a@b",
                    &["c@d".into()],
                )
                .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        store.prune(3).unwrap();
        let l = store.list(10);
        assert_eq!(
            l.iter().map(|s| s.subject.as_str()).collect::<Vec<_>>(),
            ["m4", "m3", "m2"]
        );
        store.delete(&l[0].id).unwrap();
        assert_eq!(store.count(), 2);
        assert!(
            store.detail("../../etc/passwd").is_err(),
            "ids are validated"
        );
        assert_eq!(store.clear().unwrap(), 2);
        assert_eq!(store.count(), 0);
    }
}
