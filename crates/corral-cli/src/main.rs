//! `corral`: command-line interface. Every command works on the same files the
//! desktop app uses, and a running `corral start` (or the app) picks up
//! changes automatically.

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use corral_core::php::{self, PhpVersion};
use corral_core::sites::Registry;
use corral_core::{Config, Paths};
use corral_engine::install;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "corral",
    version,
    about = "Free, open-source local PHP development environment"
)]
struct Cli {
    /// Print machine-readable JSON where supported.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Download the web server and write initial configuration.
    Setup {
        /// Serve on the internal high ports (no privileged helper needed).
        #[arg(long)]
        high_ports: bool,
    },
    /// Run DNS, PHP-FPM and the web server in the foreground.
    Start,
    /// Regenerate all configuration files and report problems.
    Render,
    /// List all sites.
    Sites,
    /// Serve every folder inside DIR as <folder>.<tld>.
    Park { dir: Option<PathBuf> },
    /// Stop serving the folders inside DIR.
    Unpark { dir: Option<PathBuf> },
    /// Serve DIR (default: current directory) as NAME.<tld>.
    Link {
        name: Option<String>,
        #[arg(long)]
        path: Option<PathBuf>,
    },
    /// Remove a linked site.
    Unlink { name: Option<String> },
    /// Pin a site to a PHP version.
    Isolate {
        version: String,
        #[arg(long)]
        site: Option<String>,
    },
    /// Remove a site's PHP pin.
    Unisolate {
        #[arg(long)]
        site: Option<String>,
    },
    /// Serve a site over HTTPS.
    Secure {
        #[arg(long)]
        site: Option<String>,
    },
    /// Serve a site over plain HTTP.
    Unsecure {
        #[arg(long)]
        site: Option<String>,
    },
    /// Set the global default PHP version.
    Use { version: String },
    /// Manage PHP versions.
    #[command(subcommand)]
    Php(PhpCmd),
    /// Show or change the local TLD.
    Tld { tld: Option<String> },
    /// Print the commands that install DNS and certificate trust manually
    /// (used until the privileged helper is installed).
    Trust,
    /// Check the environment for common problems.
    Doctor,
    /// Show what is installed and whether Corral is running.
    Status,
    /// Add Corral's bin directory to your shell PATH (or --remove it).
    ShellPath {
        #[arg(long)]
        remove: bool,
    },
    /// Install or update Composer (verified against its published SHA-256).
    Composer,
    /// Remove Corral: shims, PATH lines, generated files and (with
    /// --delete-data) runtimes and service data. Prints the sudo commands for
    /// system-level undo.
    Uninstall {
        /// Actually do it (without this, only shows what would be removed).
        #[arg(long)]
        yes: bool,
        /// Also delete databases and downloaded runtimes.
        #[arg(long)]
        delete_data: bool,
    },
    /// Manage Node.js versions.
    #[command(subcommand)]
    Node(NodeCmd),
    /// Create a new Laravel project in your first parked folder.
    New {
        name: String,
        /// none, react, vue or livewire
        #[arg(long, default_value = "none")]
        kit: String,
        /// Parent folder (default: first parked folder).
        #[arg(long)]
        path: Option<PathBuf>,
    },
    /// Show a Laravel site's log (storage/logs), newest first.
    Logs {
        /// Site name (default: the site for the current directory).
        #[arg(long)]
        site: Option<String>,
        #[arg(short = 'n', long, default_value_t = 20)]
        limit: usize,
        /// Minimum level: debug, info, notice, warning, error, critical…
        #[arg(long)]
        level: Option<String>,
        /// Include stack traces.
        #[arg(long)]
        trace: bool,
    },
    /// Show dump()/dd() output captured from PHP, newest first.
    Dumps {
        #[arg(short = 'n', long, default_value_t = 10)]
        limit: usize,
        /// Delete all captured dumps.
        #[arg(long)]
        clear: bool,
    },
    /// Laravel watchers (queries, HTTP calls, logs, views, cache, jobs).
    /// `enable` adds the corral/laravel package to the project as a --dev
    /// dependency from Corral's local copy; `disable` removes it again.
    Watch {
        /// enable or disable
        action: String,
        #[arg(long)]
        site: Option<String>,
    },
    /// macOS integration: `.test` DNS, HTTPS trust and ports 80/443.
    /// `setup` shows the standard macOS password dialog once.
    System {
        /// status, setup or remove
        action: String,
    },
    /// Show events from the Laravel watchers, newest first.
    Events {
        #[arg(short = 'n', long, default_value_t = 30)]
        limit: usize,
        /// Only one kind: query, http, log, view, cache, job
        #[arg(long)]
        kind: Option<String>,
        #[arg(long)]
        clear: bool,
    },
    /// Show mail caught by Corral (SMTP on 127.0.0.1:2525).
    #[command(subcommand)]
    Mail(MailCmd),
    /// Manage databases and other background services.
    #[command(subcommand)]
    Service(ServiceCmd),
    /// Manage databases inside a MySQL service.
    #[command(subcommand)]
    Db(DbCmd),
    /// Print where Corral keeps its files.
    Paths,
}

#[derive(Subcommand)]
enum ServiceCmd {
    /// List service instances and the catalog.
    List,
    /// Install and add a service instance (e.g. `corral service add mysql`).
    Add {
        kind: String,
        /// Version prefix, e.g. 8.4 (default: newest in the catalog).
        #[arg(long)]
        version: Option<String>,
        /// Instance name (default: the kind, e.g. "mysql").
        #[arg(long)]
        name: Option<String>,
        /// Port on 127.0.0.1 (default: the standard port, or the next free one).
        #[arg(long)]
        port: Option<u16>,
    },
    /// Enable an instance; a running engine starts it within ~1s.
    Start { name: String },
    /// Disable an instance; a running engine stops it within ~1s.
    Stop { name: String },
    /// Remove an instance. Data is kept unless --delete-data is given.
    Remove {
        name: String,
        #[arg(long)]
        delete_data: bool,
    },
}

#[derive(Subcommand)]
enum DbCmd {
    /// Create a database. Without NAME, uses DB_DATABASE from ./.env.
    Create {
        name: Option<String>,
        #[arg(long, default_value = "mysql")]
        service: String,
        /// Switch ./.env to this dedicated DB_USERNAME (a backup of .env is
        /// kept as .env.corral-backup). The existing DB_PASSWORD is reused.
        #[arg(long)]
        user: Option<String>,
    },
    /// List databases.
    List {
        #[arg(long, default_value = "mysql")]
        service: String,
    },
}

use corral_core::dotenv::{set_value as set_dotenv_value, value as dotenv_value};

#[derive(Subcommand)]
enum MailCmd {
    /// List caught messages, newest first.
    List {
        #[arg(short = 'n', long, default_value_t = 20)]
        limit: usize,
    },
    /// Show one message (text body, or HTML if there is no text part).
    Show { id: String },
    /// Delete all caught messages.
    Clear,
}

#[derive(Subcommand)]
enum NodeCmd {
    /// List installed versions.
    List,
    /// Install a major version (default: newest LTS), verified via SHASUMS256.
    Install { major: Option<u32> },
    /// Make a major version the default for node/npm/npx.
    Use { major: u32 },
    /// Remove a major version.
    Remove { major: u32 },
}

/// Map a starter-kit name to its Composer package.
fn starter_kit(kit: &str) -> Option<&'static str> {
    Some(match kit {
        "none" | "" => "laravel/laravel",
        "react" => "laravel/react-starter-kit",
        "vue" => "laravel/vue-starter-kit",
        "livewire" => "laravel/livewire-starter-kit",
        _ => return None,
    })
}

#[derive(Subcommand)]
enum PhpCmd {
    /// List installed versions.
    List,
    /// Install a PHP version (e.g. 8.4).
    Install {
        version: String,
        /// Install without checksum verification (upstream publishes none yet).
        #[arg(long)]
        allow_unverified: bool,
        /// Corral build manifest (https URL or file) with SHA-256 for every
        /// artifact; includes Xdebug when the build has it.
        #[arg(long)]
        manifest: Option<String>,
    },
    /// Remove an installed version.
    Remove { version: String },
    /// Show or change php.ini overrides for a version (90-user.ini).
    /// Examples: `corral php ini 8.4`, `corral php ini 8.4 memory_limit=1G`,
    /// `corral php ini 8.4 memory_limit=` (removes the override).
    Ini {
        version: String,
        settings: Vec<String>,
    },
}

fn cwd() -> anyhow::Result<PathBuf> {
    Ok(std::env::current_dir()?)
}

fn parse_php(v: &str) -> anyhow::Result<PhpVersion> {
    PhpVersion::parse(v).with_context(|| format!("invalid PHP version {v:?} (expected e.g. 8.4)"))
}

/// Resolve `--site` or the site serving the current directory.
fn site_name(paths: &Paths, cfg: &Config, explicit: Option<String>) -> anyhow::Result<String> {
    if let Some(s) = explicit {
        return Ok(s);
    }
    let reg = Registry::load(paths)?;
    match reg.find_by_path(cfg, &cwd()?)? {
        Some(site) => Ok(site.name),
        None => bail!("the current directory is not a Corral site; pass a site name"),
    }
}

fn print_sites(paths: &Paths, cfg: &Config, json: bool) -> anyhow::Result<()> {
    let sites = Registry::load(paths)?.resolve(cfg)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&sites)?);
        return Ok(());
    }
    if sites.is_empty() {
        println!("No sites yet. Try `corral park ~/Sites` or `corral link` inside a project.");
        return Ok(());
    }
    let w = sites.iter().map(|s| s.url(cfg).len()).max().unwrap_or(0);
    for s in &sites {
        let php = match (&s.php, s.isolated) {
            (Some(v), true) => format!("PHP {v} (isolated)"),
            (Some(v), false) => format!("PHP {v}"),
            (None, _) => "PHP -".into(),
        };
        println!(
            "{:w$}  {:<18} {:<14} {:?}  {}",
            s.url(cfg),
            php,
            s.driver.kind.label(),
            s.origin,
            s.path.display()
        );
    }
    Ok(())
}

async fn run(cli: Cli) -> anyhow::Result<()> {
    let paths = Paths::discover()?;
    paths.ensure()?;
    let mut cfg = Config::load(&paths)?;

    match cli.cmd {
        Cmd::Setup { high_ports } => {
            if high_ports {
                cfg.http_port = cfg.internal_http_port;
                cfg.https_port = cfg.internal_https_port;
                cfg.save(&paths)?;
            }
            if install::caddy_bin(&paths).exists() {
                println!("Web server already installed.");
            } else {
                let bin = install::install_caddy(&paths).await?;
                println!(
                    "Installed Caddy {} to {}",
                    install::CADDY_VERSION,
                    bin.display()
                );
            }
            if !install::composer_phar(&paths).exists() {
                match install::install_composer(&paths).await {
                    Ok(_) => println!("Installed Composer."),
                    Err(e) => println!(
                        "warning: Composer not installed ({e:#}); retry with `corral composer`"
                    ),
                }
            }
            let r = corral_engine::render(&paths)?;
            println!("Configuration written ({} sites).", r.sites.len());
            println!(
                "\nNext steps:\n  corral php install 8.4 --allow-unverified\n  corral park ~/Sites\n  corral trust   # one-time DNS + HTTPS trust\n  corral start"
            );
        }
        Cmd::Start => {
            use tokio::signal::unix::{SignalKind, signal};
            let mut term = signal(SignalKind::terminate())?;
            let mut hup = signal(SignalKind::hangup())?;
            let shutdown = async move {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = term.recv() => {}
                    _ = hup.recv() => {}
                }
                tracing::info!("shutting down");
            };
            println!(
                "Corral running: DNS on {}:{}, HTTP {}, HTTPS {}. Press Ctrl-C to stop.",
                cfg.loopback, cfg.dns_port, cfg.internal_http_port, cfg.internal_https_port
            );
            corral_engine::run(paths, shutdown).await?;
        }
        Cmd::Render => {
            let r = corral_engine::render(&paths)?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&r)?);
            } else {
                println!(
                    "Rendered {} sites; FPM pools: {:?}",
                    r.sites.len(),
                    r.fpm_versions
                );
                for (site, v) in &r.missing_php {
                    println!("warning: {site} wants PHP {v}, which is not installed");
                }
            }
        }
        Cmd::Sites => print_sites(&paths, &cfg, cli.json)?,
        Cmd::Park { dir } => {
            let dir = dir.map_or_else(cwd, Ok)?.canonicalize()?;
            if !cfg.parked.contains(&dir) {
                cfg.parked.push(dir.clone());
                cfg.save(&paths)?;
            }
            println!("Parked {}", dir.display());
            print_sites(&paths, &cfg, false)?;
        }
        Cmd::Unpark { dir } => {
            let dir = dir.map_or_else(cwd, Ok)?.canonicalize()?;
            let before = cfg.parked.len();
            cfg.parked.retain(|p| p != &dir);
            if cfg.parked.len() == before {
                bail!("{} is not parked", dir.display());
            }
            cfg.save(&paths)?;
            println!("Unparked {}", dir.display());
        }
        Cmd::Link { name, path } => {
            let path = path.map_or_else(cwd, Ok)?;
            let mut reg = Registry::load(&paths)?;
            let name = reg.link(&path, name.as_deref())?;
            reg.save()?;
            let site = reg
                .resolve(&cfg)?
                .into_iter()
                .find(|s| s.name == name)
                .context("linked site vanished")?;
            println!(
                "Linked {} → {} ({})",
                site.url(&cfg),
                site.path.display(),
                site.driver.kind.label()
            );
        }
        Cmd::Unlink { name } => {
            let name = site_name(&paths, &cfg, name)?;
            let mut reg = Registry::load(&paths)?;
            reg.unlink(&name)?;
            reg.save()?;
            println!("Unlinked {name}");
        }
        Cmd::Isolate { version, site } => {
            let v = parse_php(&version)?;
            let name = site_name(&paths, &cfg, site)?;
            let mut reg = Registry::load(&paths)?;
            reg.isolate(&cfg, &name, Some(v.to_string()))?;
            reg.save()?;
            if php::find(&paths, v)?.is_none() {
                println!("note: PHP {v} is not installed yet; run `corral php install {v}`");
            }
            println!("{name} now uses PHP {v}");
        }
        Cmd::Unisolate { site } => {
            let name = site_name(&paths, &cfg, site)?;
            let mut reg = Registry::load(&paths)?;
            reg.isolate(&cfg, &name, None)?;
            reg.save()?;
            println!("{name} now uses the default PHP version");
        }
        Cmd::Secure { site } => {
            let name = site_name(&paths, &cfg, site)?;
            let mut reg = Registry::load(&paths)?;
            let val = (!cfg.secure_by_default).then_some(true);
            reg.set_secure(&cfg, &name, val)?;
            reg.save()?;
            println!("{name} is served over HTTPS");
        }
        Cmd::Unsecure { site } => {
            let name = site_name(&paths, &cfg, site)?;
            let mut reg = Registry::load(&paths)?;
            reg.set_secure(&cfg, &name, Some(false))?;
            reg.save()?;
            println!("{name} is served over HTTP");
        }
        Cmd::Use { version } => {
            let v = parse_php(&version)?;
            if php::find(&paths, v)?.is_none() {
                bail!("PHP {v} is not installed; run `corral php install {v}`");
            }
            cfg.default_php = Some(v.to_string());
            cfg.save(&paths)?;
            php::write_shims(&paths, Some(v))?;
            println!("Default PHP is now {v}");
        }
        Cmd::Php(PhpCmd::List) => {
            let installed = php::installed(&paths)?;
            if installed.is_empty() {
                println!("No PHP versions installed. Try `corral php install 8.4`.");
            }
            for i in installed {
                let mark = if cfg.default_php.as_deref() == Some(&i.version.to_string()) {
                    "*"
                } else {
                    " "
                };
                println!("{mark} {}  {}", i.version, i.dir.display());
            }
        }
        Cmd::Php(PhpCmd::Install {
            version,
            allow_unverified,
            manifest,
        }) => {
            let v = parse_php(&version)?;
            let full = if let Some(src) = manifest.as_deref() {
                let m = install::load_manifest(src).await?;
                let b = m
                    .pick(v)
                    .with_context(|| format!("manifest has no PHP {v} build for this Mac"))?
                    .clone();
                install::install_php_from_manifest(&paths, &b).await?;
                println!(
                    "Verified all artifacts{}",
                    if b.xdebug.is_some() {
                        " (including Xdebug)"
                    } else {
                        ""
                    }
                );
                b.version
            } else {
                let full = install::latest_static_php(v).await?;
                let src = install::PhpSource::static_php(&full);
                install::install_php(&paths, v, &src, allow_unverified).await?;
                full
            };
            if cfg.default_php.is_none() {
                cfg.default_php = Some(v.to_string());
                cfg.save(&paths)?;
            }
            corral_engine::render(&paths)?;
            println!("Installed PHP {full}");
        }
        Cmd::Php(PhpCmd::Ini { version, settings }) => {
            let v = parse_php(&version)?;
            let inst =
                php::find(&paths, v)?.with_context(|| format!("PHP {v} is not installed"))?;
            for kv in &settings {
                let (k, val) = kv
                    .split_once('=')
                    .with_context(|| format!("expected key=value, got {kv:?}"))?;
                let val = val.trim();
                php::set_user_ini(&inst, k.trim(), (!val.is_empty()).then_some(val))?;
            }
            let file = php::user_ini(&inst);
            println!("# {}", file.display());
            match std::fs::read_to_string(&file) {
                Ok(s) => print!("{s}"),
                Err(_) => println!("; no overrides yet"),
            }
            if !settings.is_empty() {
                println!("# A running Corral restarts PHP {v} within ~1s.");
            }
        }
        Cmd::Logs {
            site,
            limit,
            level,
            trace,
        } => {
            use corral_core::logs;
            let name = site_name(&paths, &cfg, site)?;
            let s = Registry::load(&paths)?
                .resolve(&cfg)?
                .into_iter()
                .find(|x| x.name == name)
                .with_context(|| format!("no site named {name:?}"))?;
            let min = match level.as_deref() {
                Some(l) => {
                    Some(logs::Level::parse(l).with_context(|| format!("unknown level {l:?}"))?)
                }
                None => None,
            };
            let file = logs::log_files(&s.path)
                .into_iter()
                .next()
                .with_context(|| format!("no log files in {}/storage/logs", s.path.display()))?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&logs::tail(&file, 4 << 20, limit, min)?)?
                );
            } else {
                println!("# {}", file.display());
                for e in logs::tail(&file, 4 << 20, limit, min)? {
                    println!(
                        "{} {:>9} {}",
                        e.time,
                        format!("{:?}", e.level).to_uppercase(),
                        e.message
                    );
                    if trace && !e.details.is_empty() {
                        println!("{}", e.details);
                    }
                }
            }
        }
        Cmd::Dumps { limit, clear } => {
            let store = corral_dumps::Store::new(corral_dumps::store_file(&paths.root));
            if clear {
                store.clear()?;
                println!("Cleared dumps");
            } else {
                let list = store.list(limit);
                if list.is_empty() {
                    println!(
                        "No dumps yet{}.",
                        if cfg.dumps {
                            ""
                        } else {
                            " (dumps are disabled: set dumps = true)"
                        }
                    );
                }
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&list)?);
                } else {
                    for d in list {
                        let at = match (&d.file, d.line) {
                            (Some(f), Some(l)) => format!("{f}:{l}"),
                            (Some(f), None) => f.clone(),
                            _ => "?".into(),
                        };
                        let req = match (&d.method, &d.host, &d.uri) {
                            (Some(m), Some(h), Some(u)) => format!("{m} {h}{u}"),
                            _ => d.sapi.clone().unwrap_or_default(),
                        };
                        println!("#{} {at}  [{req}]", d.id);
                        println!("{}", d.text.trim_end());
                    }
                }
            }
        }
        Cmd::Watch { action, site } => {
            let name = site_name(&paths, &cfg, site)?;
            let s = Registry::load(&paths)?
                .resolve(&cfg)?
                .into_iter()
                .find(|x| x.name == name)
                .with_context(|| format!("no site named {name:?}"))?;
            let enable = match action.as_str() {
                "enable" => true,
                "disable" => false,
                other => bail!("unknown action {other:?}: use enable or disable"),
            };
            corral_engine::project::set_watchers(&paths, &s.path, enable)?;
            println!(
                "Watchers {} for {name}.",
                if enable { "enabled" } else { "removed" }
            );
        }
        Cmd::System { action } => {
            let exe = std::env::current_exe()?;
            let st = match action.as_str() {
                "status" => corral_system::status(&paths, &cfg),
                "setup" => corral_system::setup(&paths, &exe)?,
                "remove" => corral_system::undo(&paths)?,
                other => bail!("unknown action {other:?}: use status, setup or remove"),
            };
            let mark = |b: bool| if b { "ok  " } else { "--  " };
            println!("{}.{} domains -> Corral DNS", mark(st.dns), cfg.tld);
            println!("{}HTTPS certificates trusted", mark(st.https));
            println!("{}ports 80/443 -> Corral", mark(st.ports));
            if st.dns_foreign {
                println!("note: /etc/resolver/{} belongs to another tool", cfg.tld);
            }
            if st.ports_busy {
                println!("note: another app is using ports 80/443");
            }
        }
        Cmd::Events { limit, kind, clear } => {
            let store = corral_dumps::events::store(&paths.root);
            if clear {
                store.clear()?;
                println!("Cleared events");
            } else {
                let mut list = store.list(if kind.is_some() { 5000 } else { limit });
                if let Some(k) = &kind {
                    list.retain(|e| &e.kind == k);
                    list.truncate(limit);
                }
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&list)?);
                } else {
                    for e in list {
                        let d = &e.data;
                        let summary = match e.kind.as_str() {
                            "query" => format!(
                                "{:>8.2}ms  {}",
                                d["time_ms"].as_f64().unwrap_or(0.0),
                                d["sql"].as_str().unwrap_or("")
                            ),
                            "http" => format!(
                                "{} {} -> {}",
                                d["method"].as_str().unwrap_or(""),
                                d["url"].as_str().unwrap_or(""),
                                d["status"]
                            ),
                            "log" => format!(
                                "{}: {}",
                                d["level"].as_str().unwrap_or(""),
                                d["message"].as_str().unwrap_or("")
                            ),
                            "view" => d["name"].as_str().unwrap_or("").to_string(),
                            "cache" => format!(
                                "{} {}",
                                d["op"].as_str().unwrap_or(""),
                                d["key"].as_str().unwrap_or("")
                            ),
                            "job" => format!(
                                "{} {}",
                                d["name"].as_str().unwrap_or(""),
                                d["status"].as_str().unwrap_or("")
                            ),
                            _ => d.to_string(),
                        };
                        println!(
                            "#{:<5} {:<6} {}",
                            e.id,
                            e.kind,
                            summary.chars().take(200).collect::<String>()
                        );
                    }
                }
            }
        }
        Cmd::Mail(cmd) => {
            let store = corral_mail::Store::new(corral_mail::store_dir(&paths.root));
            match cmd {
                MailCmd::List { limit } => {
                    let list = store.list(limit);
                    if list.is_empty() {
                        println!(
                            "No mail yet. Point your app at it:\n  MAIL_MAILER=smtp\n  MAIL_HOST=127.0.0.1\n  MAIL_PORT={}",
                            cfg.mail_port
                        );
                    }
                    for m in list {
                        println!(
                            "{}  {:<28} {:<30} {}{}",
                            m.id,
                            m.from.chars().take(28).collect::<String>(),
                            m.to.join(", ").chars().take(30).collect::<String>(),
                            m.subject,
                            if m.attachments > 0 {
                                format!("  [{} attachment(s)]", m.attachments)
                            } else {
                                String::new()
                            }
                        );
                    }
                }
                MailCmd::Show { id } => {
                    let d = store.detail(&id)?;
                    println!(
                        "From:    {}\nTo:      {}\nSubject: {}",
                        d.summary.from,
                        d.summary.to.join(", "),
                        d.summary.subject
                    );
                    if !d.cc.is_empty() {
                        println!("Cc:      {}", d.cc.join(", "));
                    }
                    for a in &d.attachment_list {
                        println!("Attach:  {} ({}, {} bytes)", a.name, a.content_type, a.size);
                    }
                    println!();
                    println!("{}", d.text.or(d.html).unwrap_or_default());
                }
                MailCmd::Clear => println!("Deleted {} message(s)", store.clear()?),
            }
        }
        Cmd::Node(NodeCmd::List) => {
            let inst = corral_engine::node::installed(&paths);
            if inst.is_empty() {
                println!("No Node versions installed. Try `corral node install` (newest LTS).");
            }
            for (m, ver) in inst {
                let mark = if cfg.default_node == Some(m) {
                    "*"
                } else {
                    " "
                };
                println!("{mark} {m}  {ver}");
            }
        }
        Cmd::Node(NodeCmd::Install { major }) => {
            let (m, ver) = corral_engine::node::install(&paths, major).await?;
            if cfg.default_node.is_none() {
                cfg.default_node = Some(m);
                cfg.save(&paths)?;
            }
            corral_engine::node::write_shims(&paths, cfg.default_node)?;
            println!("Installed Node {ver} (verified against SHASUMS256.txt)");
        }
        Cmd::Node(NodeCmd::Use { major }) => {
            corral_engine::node::write_shims(&paths, Some(major))?;
            cfg.default_node = Some(major);
            cfg.save(&paths)?;
            println!("Default Node is now {major}");
        }
        Cmd::Node(NodeCmd::Remove { major }) => {
            corral_engine::node::remove(&paths, major)?;
            if cfg.default_node == Some(major) {
                cfg.default_node = corral_engine::node::installed(&paths)
                    .first()
                    .map(|(m, _)| *m);
                cfg.save(&paths)?;
            }
            corral_engine::node::write_shims(&paths, cfg.default_node)?;
            println!("Removed Node {major}");
        }
        Cmd::New { name, kit, path } => {
            let package = starter_kit(&kit).with_context(|| {
                format!("unknown kit {kit:?}: use none, react, vue or livewire")
            })?;
            let slug = corral_core::sites::slugify(&name)
                .filter(|s| s == &name)
                .with_context(|| {
                    format!("project name {name:?} must be lowercase letters, digits and '-'")
                })?;
            let parent =
                match path {
                    Some(p) => p,
                    None => cfg.parked.first().cloned().context(
                        "no parked folder; pass --path or run `corral park <dir>` first",
                    )?,
                };
            let dir = parent.join(&slug);
            anyhow::ensure!(!dir.exists(), "{} already exists", dir.display());
            anyhow::ensure!(
                install::composer_phar(&paths).exists(),
                "Composer missing: run `corral composer`"
            );
            anyhow::ensure!(
                paths.bin.join("php").exists(),
                "no PHP installed: run `corral php install 8.4`"
            );
            println!("Creating {} from {package}…", dir.display());
            let status = tokio::process::Command::new(paths.bin.join("composer"))
                .args(["create-project", package])
                .arg(&dir)
                .args(["--no-interaction", "--prefer-dist"])
                // Composer scripts call `php`; make sure it is Corral's.
                .env(
                    "PATH",
                    format!(
                        "{}:{}",
                        paths.bin.display(),
                        std::env::var("PATH").unwrap_or_default()
                    ),
                )
                .status()
                .await?;
            anyhow::ensure!(
                status.success(),
                "composer create-project failed ({status})"
            );
            if !cfg.parked.contains(&parent) {
                let mut reg = Registry::load(&paths)?;
                reg.link(&dir, Some(&slug))?;
                reg.save()?;
            }
            let site = Registry::load(&paths)?
                .resolve(&cfg)?
                .into_iter()
                .find(|s| s.name == slug)
                .context("new project was not picked up as a site")?;
            println!("\nReady: {}", site.url(&cfg));
            if kit != "none" {
                println!(
                    "Front-end assets: cd {} && npm install && npm run build",
                    dir.display()
                );
            }
        }
        Cmd::Php(PhpCmd::Remove { version }) => {
            let v = parse_php(&version)?;
            let inst =
                php::find(&paths, v)?.with_context(|| format!("PHP {v} is not installed"))?;
            anyhow::ensure!(
                paths.owns(&inst.dir),
                "refusing to delete {}",
                inst.dir.display()
            );
            std::fs::remove_dir_all(&inst.dir)?;
            if cfg.default_php.as_deref() == Some(&v.to_string()) {
                cfg.default_php = php::installed(&paths)?
                    .first()
                    .map(|i| i.version.to_string());
                cfg.save(&paths)?;
            }
            corral_engine::render(&paths)?;
            println!("Removed PHP {v}");
        }
        Cmd::Tld { tld: None } => println!("{}", cfg.tld),
        Cmd::Tld { tld: Some(t) } => {
            let old = std::mem::replace(&mut cfg.tld, t);
            cfg.save(&paths)?;
            corral_engine::render(&paths)?;
            println!(
                "TLD changed from .{old} to .{}. Run `corral trust` again to update DNS and HTTPS trust.",
                cfg.tld
            );
        }
        Cmd::Trust => {
            let r = corral_engine::render(&paths)?;
            let ca = corral_tls::LocalCa::cert_path(&paths.certs_dir());
            let resolver = corral_dns::resolver_file(cfg.dns_port);
            println!(
                "Until the privileged helper ships, run these once (they need your password):\n"
            );
            println!("  sudo mkdir -p /etc/resolver");
            println!(
                "  printf '{}' | sudo tee /etc/resolver/{} >/dev/null",
                resolver.replace('\n', "\\n"),
                cfg.tld
            );
            println!(
                "  sudo security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain \"{}\"",
                ca.display()
            );
            println!("\nCA SHA-256 fingerprint: {}", r.ca_fingerprint);
            println!(
                "\nTo undo:\n  sudo rm /etc/resolver/{}\n  sudo security remove-trusted-cert -d \"{}\"",
                cfg.tld,
                ca.display()
            );
        }
        Cmd::Doctor => doctor(&paths, &cfg)?,
        Cmd::Status => {
            let running = corral_engine::orphans::engine_running(&paths);
            println!("engine:   {}", if running { "running" } else { "stopped" });
            println!(
                "caddy:    {}",
                if install::caddy_bin(&paths).exists() {
                    "installed"
                } else {
                    "missing (corral setup)"
                }
            );
            println!(
                "composer: {}",
                if install::composer_phar(&paths).exists() {
                    "installed"
                } else {
                    "missing (corral composer)"
                }
            );
            let php: Vec<String> = php::installed(&paths)?
                .iter()
                .map(|i| i.version.to_string())
                .collect();
            println!(
                "php:      {} (default {})",
                if php.is_empty() {
                    "none".into()
                } else {
                    php.join(", ")
                },
                cfg.default_php.as_deref().unwrap_or("-")
            );
            let svc = corral_services::store::ServicesFile::load(&paths)?;
            for (n, i) in &svc.instances {
                println!(
                    "service:  {n} {} {} :{} {}",
                    i.kind.label(),
                    i.version,
                    i.port,
                    if corral_services::port_in_use(i.port) {
                        "listening"
                    } else {
                        "down"
                    }
                );
            }
            let sites = Registry::load(&paths)?.resolve(&cfg)?;
            println!(
                "sites:    {} on .{} (https :{}, http :{})",
                sites.len(),
                cfg.tld,
                cfg.https_port,
                cfg.http_port
            );
            let home = dirs_home()?;
            let on_path = corral_core::shell::rc_files(&home)
                .iter()
                .any(|f| corral_core::shell::has_block(f));
            println!(
                "PATH:     {}",
                if on_path {
                    "configured"
                } else {
                    "not configured (corral shell-path)"
                }
            );
        }
        Cmd::ShellPath { remove } => {
            let home = dirs_home()?;
            for rc in corral_core::shell::rc_files(&home) {
                let changed = if remove {
                    corral_core::shell::uninstall(&rc)?
                } else {
                    corral_core::shell::install(&rc, &paths.bin)?
                };
                if changed {
                    println!(
                        "{} {}",
                        if remove { "Removed from" } else { "Updated" },
                        rc.display()
                    );
                }
            }
            if !remove {
                println!(
                    "Open a new terminal (or `source ~/.zshrc`); `which php` should then show {}",
                    paths.bin.join("php").display()
                );
            }
        }
        Cmd::Composer => {
            let phar = install::install_composer(&paths).await?;
            println!(
                "Installed Composer to {} (shim: {})",
                phar.display(),
                paths.bin.join("composer").display()
            );
        }
        Cmd::Uninstall { yes, delete_data } => uninstall(&paths, &cfg, yes, delete_data)?,
        Cmd::Service(ServiceCmd::List) => {
            use corral_services::store::ServicesFile;
            let file = ServicesFile::load(&paths)?;
            if file.instances.is_empty() {
                println!("No services yet. Try `corral service add mysql`.");
            }
            for (name, i) in &file.instances {
                let up = corral_services::port_in_use(i.port);
                println!(
                    "{name:<14} {} {:<8} 127.0.0.1:{:<6} {:<9} {}",
                    i.kind.label(),
                    i.version,
                    i.port,
                    if i.enabled { "enabled" } else { "disabled" },
                    if up { "listening" } else { "not running" }
                );
            }
            println!("\nCatalog:");
            for r in corral_services::manifest::RELEASES {
                println!(
                    "  {} {}{}  license: {}",
                    r.kind.as_str(),
                    r.version,
                    if r.artifact().is_some() {
                        ""
                    } else {
                        " (not available for this CPU)"
                    },
                    r.kind.license()
                );
            }
        }
        Cmd::Service(ServiceCmd::Add {
            kind,
            version,
            name,
            port,
        }) => {
            let k = corral_services::manifest::Kind::parse(&kind)
                .with_context(|| format!("unknown service {kind:?}; see `corral service list`"))?;
            println!("Installing {} (license: {})…", k.label(), k.license());
            let (name, inst) =
                corral_services::add(&paths, k, version.as_deref(), name.as_deref(), port).await?;
            corral_services::write_shims(
                &paths,
                &corral_services::store::ServicesFile::load(&paths)?,
            )?;
            println!(
                "Added {name}: {} {} on 127.0.0.1:{}",
                k.label(),
                inst.version,
                inst.port
            );
            println!("It starts with `corral start` (or within ~1s if Corral is already running).");
            println!(
                "\nLaravel .env:\n  DB_CONNECTION=mysql\n  DB_HOST=127.0.0.1\n  DB_PORT={}\n  DB_USERNAME=root\n  DB_PASSWORD=",
                inst.port
            );
        }
        Cmd::Service(ServiceCmd::Start { name }) => {
            let mut f = corral_services::store::ServicesFile::load(&paths)?;
            f.set_enabled(&name, true)?;
            f.save(&paths)?;
            println!("{name} enabled");
        }
        Cmd::Service(ServiceCmd::Stop { name }) => {
            let mut f = corral_services::store::ServicesFile::load(&paths)?;
            f.set_enabled(&name, false)?;
            f.save(&paths)?;
            println!("{name} disabled");
        }
        Cmd::Service(ServiceCmd::Remove { name, delete_data }) => {
            corral_services::remove(&paths, &name, delete_data)?;
            if delete_data {
                println!("Removed {name} and deleted its data");
            } else {
                println!(
                    "Removed {name}; data kept in {}",
                    corral_services::store::data_dir(&paths, &name).display()
                );
            }
        }
        Cmd::Db(DbCmd::Create {
            name,
            service,
            user,
        }) => {
            if let Some(u) = &user {
                anyhow::ensure!(
                    corral_services::mysql::valid_user_name(u) && u != "root",
                    "invalid --user {u:?}"
                );
                let env = cwd()?.join(".env");
                set_dotenv_value(&env, "DB_USERNAME", u)?;
                println!("Set DB_USERNAME={u} in .env (backup: .env.corral-backup)");
            }
            let f = corral_services::store::ServicesFile::load(&paths)?;
            let inst = f.get(&service)?;
            let db = match name {
                Some(n) => n,
                None => dotenv_value(&cwd()?.join(".env"), "DB_DATABASE")
                    .context("no NAME given and no DB_DATABASE in ./.env")?,
            };
            anyhow::ensure!(
                corral_services::port_in_use(inst.port),
                "{service} is not running; start Corral first"
            );
            match inst.kind {
                corral_services::manifest::Kind::Mysql => {
                    corral_services::mysql::create_database(&paths, &service, inst, &db).await?
                }
                corral_services::manifest::Kind::Postgres => {
                    corral_services::postgres::create_database(&paths, inst, &db).await?
                }
                other => bail!("{} has no databases", other.label()),
            }
            println!(
                "Database `{db}` ready on {service} (127.0.0.1:{})",
                inst.port
            );
            // Provision the app's own account from .env (values never printed).
            let env = cwd()?.join(".env");
            let builtin = |u: &str| u == "root" || u == "postgres";
            if let Some(user) = dotenv_value(&env, "DB_USERNAME").filter(|u| !builtin(u)) {
                let password = dotenv_value(&env, "DB_PASSWORD").unwrap_or_default();
                if inst.kind == corral_services::manifest::Kind::Postgres {
                    corral_services::postgres::ensure_app_user(&paths, inst, &db, &user, &password)
                        .await?;
                } else {
                    corral_services::mysql::ensure_app_user(
                        &paths, &service, inst, &db, &user, &password,
                    )
                    .await?;
                }
                println!("Account `{user}` (password from .env) has full access to `{db}`");
            } else if dotenv_value(&env, "DB_PASSWORD").is_some()
                && inst.kind == corral_services::manifest::Kind::Mysql
            {
                println!(
                    "note: .env uses root with a password, but Corral's root has none. \
                     Set DB_PASSWORD= (empty) or use a dedicated DB_USERNAME."
                );
            }
        }
        Cmd::Db(DbCmd::List { service }) => {
            let f = corral_services::store::ServicesFile::load(&paths)?;
            let inst = f.get(&service)?;
            let dbs = match inst.kind {
                corral_services::manifest::Kind::Postgres => {
                    corral_services::postgres::list_databases(&paths, inst).await?
                }
                _ => corral_services::mysql::list_databases(&paths, &service, inst).await?,
            };
            for db in dbs {
                println!("{db}");
            }
        }
        Cmd::Paths => {
            println!("data:   {}", paths.root.display());
            println!("bin:    {}", paths.bin.display());
            println!("config: {}", paths.config_file().display());
            println!("sites:  {}", paths.sites_file().display());
            println!("logs:   {}", paths.logs_dir().display());
        }
    }
    Ok(())
}

fn dirs_home() -> anyhow::Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set")
}

fn uninstall(paths: &Paths, cfg: &Config, yes: bool, delete_data: bool) -> anyhow::Result<()> {
    if corral_engine::orphans::engine_running(paths) {
        bail!("Corral is running; stop it first (Ctrl-C `corral start`, or quit the app)");
    }
    let plan = corral_engine::uninstall::plan(paths, &dirs_home()?, delete_data)?;
    println!(
        "{}",
        if yes {
            "Removing:"
        } else {
            "Would remove (re-run with --yes):"
        }
    );
    for p in &plan.remove {
        println!("  {}", p.display());
    }
    for rc in &plan.shell_files {
        println!("  Corral PATH block in {}", rc.display());
    }
    if let Some(k) = &plan.kept {
        println!(
            "Keeping service data: {} (use --delete-data to remove)",
            k.display()
        );
    }
    let sys = corral_system::status(paths, cfg);
    let system_changes =
        sys.dns || sys.ports || sys.https || corral_system::daemon_plist_path().exists();
    if system_changes {
        println!(
            "System integration (.{} DNS, HTTPS trust, ports 80/443) will be removed too.",
            cfg.tld
        );
    }
    if yes {
        if system_changes {
            corral_system::undo(paths)?;
        }
        corral_engine::uninstall::apply(paths, &plan)?;
    }
    Ok(())
}

fn doctor(paths: &Paths, cfg: &Config) -> anyhow::Result<()> {
    let mut problems = 0;
    let mut check = |ok: bool, good: &str, bad: String| {
        if ok {
            println!("ok    {good}");
        } else {
            problems += 1;
            println!("FAIL  {bad}");
        }
    };
    check(
        install::caddy_bin(paths).exists(),
        "web server installed",
        "web server missing: run `corral setup`".into(),
    );
    let installed = php::installed(paths)?;
    check(
        !installed.is_empty(),
        &format!("{} PHP version(s) installed", installed.len()),
        "no PHP installed: run `corral php install 8.4`".into(),
    );
    let resolver = PathBuf::from("/etc/resolver").join(&cfg.tld);
    let expected = corral_dns::resolver_file(cfg.dns_port);
    check(
        std::fs::read_to_string(&resolver).is_ok_and(|s| s == expected),
        &format!("{} points .{} at Corral", resolver.display(), cfg.tld),
        format!(
            "{} missing or outdated: run `corral trust`",
            resolver.display()
        ),
    );
    let path_var = std::env::var("PATH").unwrap_or_default();
    let first_php = path_var
        .split(':')
        .map(|d| PathBuf::from(d).join("php"))
        .find(|p| p.is_file());
    let shim = paths.bin.join("php");
    check(
        first_php.as_deref() == Some(shim.as_path()),
        &format!("`php` on PATH is Corral's ({})", shim.display()),
        format!(
            "`php` on PATH is {}; add `export PATH=\"{}:$PATH\"` to your shell profile",
            first_php.map_or("not found".into(), |p| p.display().to_string()),
            paths.bin.display()
        ),
    );
    for port in [cfg.internal_http_port, cfg.internal_https_port] {
        let free = std::net::TcpListener::bind((cfg.loopback, port)).is_ok();
        check(
            free,
            &format!("port {port} is free"),
            format!("port {port} is in use (is `corral start` already running?)"),
        );
    }
    if problems > 0 {
        bail!("{problems} problem(s) found");
    }
    Ok(())
}

fn main() {
    // Run by launchd when `corral system setup` registered the CLI binary.
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--port-forward") {
        let port = |i: usize, d: u16| args.get(i).and_then(|a| a.parse().ok()).unwrap_or(d);
        if let Err(e) = corral_system::forward::main_from_launchd(port(2, 18080), port(3, 18443)) {
            eprintln!("corral port forwarder: {e:#}");
            std::process::exit(1);
        }
        return;
    }
    real_main();
}

#[tokio::main]
async fn real_main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("CORRAL_LOG")
                .unwrap_or_else(|_| "info".into()),
        )
        .with_target(false)
        .init();
    if let Err(e) = run(Cli::parse()).await {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
