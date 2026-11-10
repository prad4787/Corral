//! `corral mcp`: a Model Context Protocol server over stdio, so coding agents
//! can inspect the local environment. Read-only by design: sites, logs,
//! dumps, watcher events, caught mail, services and PHP versions. `.env`
//! contents are never exposed.

use corral_core::sites::Registry;
use corral_core::{Config, Paths};
use serde_json::{Value, json};
use std::io::{BufRead, Write};

const PROTOCOL: &str = "2025-06-18";

fn tool(name: &str, description: &str, props: Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": { "type": "object", "properties": props, "required": required },
        "annotations": { "readOnlyHint": true }
    })
}

pub fn tools() -> Value {
    let limit = json!({ "type": "integer", "minimum": 1, "maximum": 200, "description": "Max items (default 20)" });
    json!([
        tool(
            "status",
            "Corral status: whether it runs, PHP versions, services, TLD and ports.",
            json!({}),
            &[]
        ),
        tool(
            "list_sites",
            "All local sites with URL, framework, PHP version and path.",
            json!({}),
            &[]
        ),
        tool(
            "site_logs",
            "Newest Laravel log entries (storage/logs) for a site, with stack traces.",
            json!({
                "site": { "type": "string", "description": "Site name, e.g. 'shop' for shop.test" },
                "level": { "type": "string", "description": "Minimum level: debug, info, warning, error…" },
                "limit": limit
            }),
            &["site"],
        ),
        tool(
            "recent_dumps",
            "Newest dump()/dd() output captured from PHP, as plain text.",
            json!({ "limit": limit }),
            &[]
        ),
        tool(
            "recent_events",
            "Newest Laravel watcher events: queries (with timings), HTTP calls, logs, views, cache, jobs.",
            json!({ "kind": { "type": "string", "enum": ["query", "http", "log", "view", "cache", "job"] }, "limit": limit }),
            &[],
        ),
        tool(
            "recent_mail",
            "Newest mail caught by Corral's SMTP catcher (summaries).",
            json!({ "limit": limit }),
            &[]
        ),
        tool(
            "read_mail",
            "One caught mail: headers summary, text body and attachment names.",
            json!({ "id": { "type": "string" } }),
            &["id"]
        ),
        tool(
            "list_services",
            "Database and other services with kind, version, port and state.",
            json!({}),
            &[]
        ),
    ])
}

fn limit(args: &Value) -> usize {
    args["limit"]
        .as_u64()
        .map_or(20, |n| n.clamp(1, 200) as usize)
}

fn text(v: impl serde::Serialize) -> anyhow::Result<String> {
    Ok(serde_json::to_string_pretty(&v)?)
}

/// Run one tool. Returns JSON text for the agent.
pub fn call(paths: &Paths, name: &str, args: &Value) -> anyhow::Result<String> {
    let cfg = Config::load(paths)?;
    match name {
        "status" => {
            let php: Vec<String> = corral_core::php::installed(paths)?
                .iter()
                .map(|i| i.version.to_string())
                .collect();
            text(json!({
                "running": corral_engine::orphans::engine_running(paths),
                "tld": cfg.tld,
                "http_port": cfg.http_port,
                "https_port": cfg.https_port,
                "php_versions": php,
                "default_php": cfg.default_php,
                "mail_smtp": format!("127.0.0.1:{}", cfg.mail_port),
            }))
        }
        "list_sites" => {
            let sites = Registry::load(paths)?.resolve(&cfg)?;
            text(
                sites
                    .iter()
                    .map(|s| {
                        json!({
                            "name": s.name, "url": s.url(&cfg), "path": s.path,
                            "framework": s.driver.kind.label(), "php": s.php, "secure": s.secure,
                        })
                    })
                    .collect::<Vec<_>>(),
            )
        }
        "site_logs" => {
            let site = args["site"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("site is required"))?;
            let s = Registry::load(paths)?
                .resolve(&cfg)?
                .into_iter()
                .find(|x| x.name == site)
                .ok_or_else(|| anyhow::anyhow!("no site named {site:?}"))?;
            let min = args["level"]
                .as_str()
                .and_then(corral_core::logs::Level::parse);
            match corral_core::logs::log_files(&s.path).first() {
                Some(f) => text(corral_core::logs::tail(f, 4 << 20, limit(args), min)?),
                None => Ok("[]".into()),
            }
        }
        "recent_dumps" => {
            let store = corral_dumps::Store::new(corral_dumps::store_file(&paths.root));
            text(
                store
                    .list(limit(args))
                    .into_iter()
                    .map(|d| json!({ "id": d.id, "time": d.time, "file": d.file, "line": d.line, "text": d.text }))
                    .collect::<Vec<_>>(),
            )
        }
        "recent_events" => {
            let mut list = corral_dumps::events::store(&paths.root).list(5000);
            if let Some(k) = args["kind"].as_str() {
                list.retain(|e| e.kind == k);
            }
            list.truncate(limit(args));
            text(list)
        }
        "recent_mail" => {
            text(corral_mail::Store::new(corral_mail::store_dir(&paths.root)).list(limit(args)))
        }
        "read_mail" => {
            let id = args["id"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("id is required"))?;
            let d = corral_mail::Store::new(corral_mail::store_dir(&paths.root)).detail(id)?;
            text(json!({
                "from": d.summary.from, "to": d.summary.to, "cc": d.cc, "subject": d.summary.subject,
                "date": d.date, "text": d.text, "has_html": d.html.is_some(),
                "attachments": d.attachment_list.iter().map(|a| &a.name).collect::<Vec<_>>(),
            }))
        }
        "list_services" => {
            let f = corral_services::store::ServicesFile::load(paths)?;
            text(
                f.instances
                    .iter()
                    .map(|(n, i)| {
                        json!({
                            "name": n, "kind": i.kind.as_str(), "version": i.version,
                            "host": "127.0.0.1", "port": i.port, "enabled": i.enabled,
                            "listening": corral_services::port_in_use(i.port),
                        })
                    })
                    .collect::<Vec<_>>(),
            )
        }
        other => anyhow::bail!("unknown tool {other:?}"),
    }
}

/// Handle one JSON-RPC message. `None` for notifications.
pub fn handle(paths: &Paths, msg: &Value) -> Option<Value> {
    let id = msg.get("id").cloned();
    let method = msg["method"].as_str().unwrap_or("");
    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": PROTOCOL,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "corral", "version": env!("CARGO_PKG_VERSION") },
            "instructions": "Corral is the local PHP environment on this Mac. Use these read-only tools to inspect sites, logs, dumps, queries, caught mail and services."
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools() })),
        "tools/call" => {
            let name = msg["params"]["name"].as_str().unwrap_or("");
            let args = msg["params"].get("arguments").cloned().unwrap_or(json!({}));
            Ok(match call(paths, name, &args) {
                Ok(t) => json!({ "content": [{ "type": "text", "text": t }] }),
                Err(e) => {
                    json!({ "content": [{ "type": "text", "text": format!("Error: {e:#}") }], "isError": true })
                }
            })
        }
        _ if id.is_none() => return None, // notifications/* need no answer
        _ => Err(json!({ "code": -32601, "message": format!("method not found: {method}") })),
    };
    let id = id?;
    Some(match result {
        Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
        Err(e) => json!({ "jsonrpc": "2.0", "id": id, "error": e }),
    })
}

/// Serve newline-delimited JSON-RPC on stdin/stdout until EOF.
pub fn serve(paths: &Paths) -> anyhow::Result<()> {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => handle(paths, &msg),
            Err(e) => Some(
                json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": e.to_string() } }),
            ),
        };
        if let Some(r) = reply {
            writeln!(out, "{r}")?;
            out.flush()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshake_list_and_call() {
        let t = tempfile::tempdir().unwrap();
        let p = Paths::at(t.path());
        p.ensure().unwrap();
        let init = handle(
            &p,
            &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        )
        .unwrap();
        assert_eq!(init["result"]["serverInfo"]["name"], "corral");
        assert!(
            handle(
                &p,
                &json!({"jsonrpc":"2.0","method":"notifications/initialized"})
            )
            .is_none()
        );
        let list = handle(&p, &json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})).unwrap();
        let names: Vec<&str> = list["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"list_sites") && names.contains(&"recent_events"));
        let call = handle(&p, &json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"status","arguments":{}}})).unwrap();
        let body: Value =
            serde_json::from_str(call["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(body["tld"], "test");
        let bad = handle(
            &p,
            &json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"nope"}}),
        )
        .unwrap();
        assert_eq!(bad["result"]["isError"], true);
        let missing = handle(&p, &json!({"jsonrpc":"2.0","id":5,"method":"bogus"})).unwrap();
        assert_eq!(missing["error"]["code"], -32601);
    }
}
