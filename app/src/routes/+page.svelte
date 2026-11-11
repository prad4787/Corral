<script lang="ts">
  import { onMount } from "svelte";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
  import { api, type Overview, type MailSummary, type MailDetail, type Dump, type LogEntry, type WatchEvent, type ProjectInfo } from "$lib/api";

  type View = "overview" | "site" | "services" | "mail" | "dumps" | "settings";

  let data = $state<Overview | null>(null);
  let view = $state<View>("overview");
  let selected = $state<string | null>(null);
  let query = $state("");
  let error = $state<string | null>(null);
  let busy = $state<string | null>(null);
  let skipSetup = $state(false);
  let notice = $state<string | null>(null);
  // .env editor for the selected site
  let envText = $state<string | null>(null);
  let envFor = $state<string | null>(null);
  // php.ini editor
  let iniVersion = $state<string | null>(null);
  let iniText = $state("");
  // new project form
  let showNew = $state(false);
  let newName = $state("");
  let newKit = $state("none");
  // mail
  let mails = $state<MailSummary[]>([]);
  let mail = $state<MailDetail | null>(null);
  let mailPane = $state<"html" | "text" | "headers">("html");
  // dumps
  let dumps = $state<Dump[]>([]);
  let events = $state<WatchEvent[]>([]);
  let dumpPane = $state<"dumps" | "activity">("dumps");
  let eventKind = $state("");
  const shownEvents = $derived(eventKind ? events.filter((e) => e.kind === eventKind) : events);
  // logs for the selected site
  let logs = $state<LogEntry[] | null>(null);
  let logLevel = $state("");
  let openTrace = $state<number | null>(null);
  // project (Laravel) info for the selected site
  let project = $state<ProjectInfo | null>(null);
  let dbUser = $state("");
  // uninstall confirmation
  let confirmUninstall = $state(false);
  let deleteData = $state(false);
  let update = $state<{ current: string; latest: string | null; newer: boolean; url: string | null } | null>(null);

  const site = $derived(data?.sites.find((s) => s.name === selected) ?? null);
  const filtered = $derived(
    (data?.sites ?? []).filter((s) => s.host.includes(query.toLowerCase().trim())),
  );
  const needsSetup = $derived(
    !!data && !skipSetup && (!data.caddy_installed || data.php.length === 0 || data.parked.length + data.sites.length === 0 || !(data.system.dns && data.system.https && data.system.ports)),
  );
  const statusText = $derived(
    !data ? "" : data.running ? "Running" : data.running_elsewhere ? "Running in terminal" : "Stopped",
  );

  async function run(label: string, op: () => Promise<Overview>) {
    busy = label;
    error = null;
    try {
      data = await op();
      if (selected && !data.sites.some((s) => s.name === selected)) {
        selected = null;
        if (view === "site") view = "overview";
      }
    } catch (e) {
      error = String(e);
    } finally {
      busy = null;
    }
  }

  async function act(label: string, op: () => Promise<void>) {
    busy = label;
    error = null;
    try {
      await op();
    } catch (e) {
      error = String(e);
    } finally {
      busy = null;
    }
  }

  async function pickDir(): Promise<string | null> {
    const dir = await openDialog({ directory: true, multiple: false });
    return typeof dir === "string" ? dir : null;
  }

  async function addParked() {
    const dir = await pickDir();
    if (dir) await run("park", () => api.park(dir));
  }

  async function linkFolder() {
    const dir = await pickDir();
    if (dir) await run("link", () => api.link(dir));
  }

  function selectSite(name: string) {
    selected = name;
    view = "site";
    envText = null;
    envFor = null;
    logs = null;
    project = null;
    api.projectInfo(name).then((p) => {
      project = p;
      dbUser = p.db?.database?.replace(/[^A-Za-z0-9_]/g, "_").slice(0, 32) ?? "";
    }).catch(() => {});
  }

  async function toggleWatchers(name: string, enable: boolean) {
    await act("watch", async () => {
      await api.setWatchers(name, enable);
      project = await api.projectInfo(name);
      notice = enable ? "Watchers enabled: see Dumps → Activity." : "Watchers removed.";
    });
  }

  async function setProcess(name: string, kind: string, on: boolean) {
    await act("proc", async () => {
      await api.setSiteProcess(name, kind, on);
      project = await api.projectInfo(name);
    });
  }

  const shareUrl = $derived(
    site && data ? (data.shares.find(([n]) => n === site.name)?.[1] ?? null) : null,
  );

  async function createDb(name: string, dedicated: boolean) {
    await act("db", async () => {
      notice = await api.createDatabase(name, dedicated ? dbUser : undefined);
      project = await api.projectInfo(name);
    });
  }

  async function refreshInbox() {
    try {
      if (view === "mail") mails = await api.mailList();
      if (view === "dumps") {
        dumps = await api.dumpsList();
        events = await api.eventsList();
      }
    } catch {
      /* ignore while polling */
    }
  }

  function show(v: View) {
    view = v;
    mail = null;
    refreshInbox();
  }

  async function openMail(id: string) {
    await act("mail", async () => {
      mail = await api.mailDetail(id);
      mailPane = mail.html ? "html" : "text";
    });
  }

  async function loadLogs(site: string) {
    await act("logs", async () => {
      logs = await api.siteLogs(site, logLevel || undefined);
      openTrace = null;
    });
  }

  function fmtTime(secs: number) {
    return new Date(secs * 1000).toLocaleString();
  }

  function eventSummary(e: WatchEvent): string {
    const d = e.data;
    switch (e.kind) {
      case "query": return `${Number(d.time_ms ?? 0).toFixed(2)} ms  ${d.sql}`;
      case "http": return `${d.method} ${d.url} → ${d.status ?? d.error ?? "?"}`;
      case "log": return `${d.level}: ${d.message}`;
      case "view": return String(d.name);
      case "cache": return `${d.op} ${d.key}`;
      case "job": return `${d.name} ${d.status}${d.error ? ": " + d.error : ""}`;
    }
    return JSON.stringify(d);
  }

  function fmtSize(n: number) {
    return n < 1024 ? `${n} B` : n < 1048576 ? `${(n / 1024).toFixed(1)} KB` : `${(n / 1048576).toFixed(1)} MB`;
  }

  async function loadEnv(name: string) {
    await act("env", async () => {
      envText = (await api.readEnv(name)) ?? "";
      envFor = name;
    });
  }

  async function saveEnv() {
    if (envFor === null || envText === null) return;
    const [n, t] = [envFor, envText];
    await act("saveenv", async () => {
      await api.writeEnv(n, t);
      notice = ".env saved (original kept as .env.corral-backup)";
    });
  }

  async function editIni(version: string) {
    await act("ini", async () => {
      iniText = await api.readPhpIni(version);
      iniVersion = version;
    });
  }

  async function saveIni() {
    if (!iniVersion) return;
    const [v, t] = [iniVersion, iniText];
    await act("saveini", async () => {
      await api.writePhpIni(v, t);
      notice = `Saved PHP ${v} overrides; a running Corral restarts that PHP within ~1s.`;
      iniVersion = null;
    });
  }

  async function createProject() {
    const name = newName.trim();
    await run("new", () => api.newProject(name, newKit));
    if (!error) {
      showNew = false;
      newName = "";
      selectSite(name);
    }
  }

  onMount(() => {
    run("load", api.overview);
    // corral://site/<name> links from the OS.
    let unlisten: (() => void) | undefined;
    import("@tauri-apps/api/event").then(({ listen }) =>
      listen<string>("corral://site", (e) => selectSite(e.payload)).then((u) => (unlisten = u)),
    );
    // One quiet check per launch; failures (offline) are ignored.
    api.checkUpdate().then((u) => (update = u)).catch(() => {});
    // Pick up changes made from the CLI.
    const t = setInterval(() => {
      if (busy) return;
      api.overview().then((d) => (data = d)).catch(() => {});
      refreshInbox();
    }, 3000);
    return () => {
      clearInterval(t);
      unlisten?.();
    };
  });
</script>

{#if needsSetup && data}
  <main class="setup">
    <h1>Set up Corral</h1>
    <p class="sub">Free, local PHP development. Everything here can be undone later in Settings.</p>

    {#if error}<div class="error" role="alert">{error}</div>{/if}

    <ol class="steps">
      <li class:done={data.caddy_installed}>
        <div>
          <strong>Web server and Composer</strong>
          <p>Caddy and Composer, downloaded from their official releases and checksum-verified.</p>
        </div>
        {#if data.caddy_installed}<span class="badge ok">Installed</span>{:else}
          <button class="btn primary" disabled={!!busy} onclick={() => run("web", () => api.install("web"))}>
            {busy === "web" ? "Installing…" : "Install"}
          </button>
        {/if}
      </li>
      <li class:done={data.php.length > 0}>
        <div>
          <strong>PHP 8.4</strong>
          <p>Static build from static-php.dev. Upstream publishes no checksums yet, so this download is not verified.</p>
        </div>
        {#if data.php.length > 0}<span class="badge ok">PHP {data.php.map((p) => p.version).join(", ")}</span>{:else}
          <button class="btn primary" disabled={!!busy} onclick={() => run("php", () => api.install("php", "8.4"))}>
            {busy === "php" ? "Installing…" : "Install"}
          </button>
        {/if}
      </li>
      <li class:done={data.parked.length + data.sites.length > 0}>
        <div>
          <strong>Your projects</strong>
          <p>Pick a folder: every project inside becomes <code>name.{data.tld}</code>.</p>
        </div>
        {#if data.parked.length > 0}<span class="badge ok">{data.sites.length} sites</span>{:else}
          <button class="btn primary" disabled={!!busy} onclick={addParked}>Choose folder…</button>
        {/if}
      </li>
      <li class:done={data.system.dns && data.system.https && data.system.ports}>
        <div>
          <strong>.{data.tld} domains and HTTPS</strong>
          <p>Lets your browser open <code>https://site.{data.tld}</code> with a trusted certificate on the normal ports. macOS asks for your password once, then to confirm the certificate.</p>
          {#if data.system.ports_busy}<p class="warn">Another app (Valet, MAMP or Docker?) is using ports 80/443. Quit it first.</p>{/if}
          {#if data.system.dns_foreign}<p class="warn">Another tool set up .{data.tld} before. Corral will take it over.</p>{/if}
        </div>
        {#if data.system.dns && data.system.https && data.system.ports}<span class="badge ok">Done</span>{:else}
          <button class="btn primary" disabled={!!busy || !data.caddy_installed || data.system.ports_busy} onclick={() => run("system", api.systemSetup)}>
            {busy === "system" ? "Waiting for macOS…" : "Set up"}
          </button>
        {/if}
      </li>
      <li class:done={data.path_configured}>
        <div>
          <strong>Terminal (optional)</strong>
          <p>Add <code>~/.corral/bin</code> to your PATH so <code>php</code>, <code>composer</code> and <code>mysql</code> use Corral. This takes precedence over Homebrew in new terminals.</p>
        </div>
        {#if data.path_configured}<span class="badge ok">Configured</span>{:else}
          <button class="btn" disabled={!!busy} onclick={() => run("path", () => api.setShellPath(true))}>Add to PATH</button>
        {/if}
      </li>
    </ol>
    <button class="btn" onclick={() => (skipSetup = true)}>
      {data.caddy_installed && data.php.length > 0 ? "Continue" : "Skip for now"}
    </button>
  </main>
{:else}
  <div class="shell">
    <aside class="sidebar">
      <header class="brand">
        <span class="dot" class:on={data?.running || data?.running_elsewhere}></span>
        <div class="title">
          <strong>Corral</strong>
          <small>{statusText}</small>
        </div>
        <button
          class="btn small"
          disabled={!!busy || !data?.caddy_installed || data?.running_elsewhere}
          title={data?.running_elsewhere ? "Started from the terminal; stop it there" : ""}
          onclick={() => run("engine", data?.running ? api.stop : api.start)}
        >
          {data?.running ? "Stop" : "Start"}
        </button>
      </header>

      <nav class="tabs" aria-label="Sections">
        <button class:active={view === "overview" || view === "site"} onclick={() => (view = "overview")}>Sites</button>
        <button class:active={view === "services"} onclick={() => show("services")}>Services</button>
        <button class:active={view === "mail"} onclick={() => show("mail")}>Mail</button>
        <button class:active={view === "dumps"} onclick={() => show("dumps")}>Dumps</button>
        <button class:active={view === "settings"} onclick={() => show("settings")}>Settings</button>
      </nav>

      <input class="search" placeholder="Search sites" bind:value={query} aria-label="Search sites" />

      <nav class="sites" aria-label="Sites">
        {#each filtered as s (s.name)}
          <button class="site-row" class:active={view === "site" && s.name === selected} onclick={() => selectSite(s.name)}>
            <span class="host">{s.host}</span>
            <span class="meta">{s.driver_label} · PHP {s.php ?? "–"}</span>
          </button>
        {:else}
          <p class="empty">{data ? "No sites yet." : "Loading…"}</p>
        {/each}
      </nav>

      {#if showNew}
        <form class="newform" onsubmit={(e) => { e.preventDefault(); createProject(); }}>
          <input placeholder="project-name" bind:value={newName} aria-label="Project name" />
          <select bind:value={newKit} aria-label="Starter kit">
            <option value="none">Laravel (blank)</option>
            <option value="livewire">Livewire starter kit</option>
            <option value="react">React starter kit</option>
            <option value="vue">Vue starter kit</option>
          </select>
          <div class="row">
            <button class="btn primary" type="submit" disabled={!!busy || !newName.trim()}>
              {busy === "new" ? "Creating…" : "Create"}
            </button>
            <button class="btn" type="button" onclick={() => (showNew = false)}>Cancel</button>
          </div>
        </form>
      {/if}
      {#if update?.newer && update.url}
        <button class="update" onclick={() => openUrl(update!.url!)}>Corral {update.latest} is available — Download</button>
      {/if}
      <footer class="actions">
        <button class="btn" onclick={() => (showNew = !showNew)} disabled={!!busy || !data?.parked.length}
          title={data?.parked.length ? "" : "Park a folder first"}>New…</button>
        <button class="btn" onclick={addParked} disabled={!!busy}>Park…</button>
        <button class="btn" onclick={linkFolder} disabled={!!busy}>Link…</button>
      </footer>
    </aside>

    <main class="detail">
      {#if error}
        <div class="error" role="alert">{error}</div>
      {/if}
      {#if notice}
        <div class="notice" role="status">{notice} <button class="link" onclick={() => (notice = null)}>Dismiss</button></div>
      {/if}

      {#if view === "site" && site}
        <h1>{site.host}</h1>
        <p class="sub">{site.driver_label} · {site.origin === "linked" ? "Linked" : "Parked"}</p>

        <div class="buttons">
          <button class="btn primary" onclick={() => openUrl(site.url)}>Open {site.url}</button>
          <button class="btn" onclick={() => act("editor", () => api.openIn(site.path, "editor"))}>Open in editor</button>
          <button class="btn" onclick={() => act("terminal", () => api.openIn(site.path, "terminal"))}>Terminal</button>
          {#if site.driver.kind === "laravel" || site.driver.kind === "statamic"}
            <button class="btn" onclick={() => act("tinker", () => api.openIn(site.path, "tinker"))}>Tinker</button>
          {/if}
          <button class="btn" onclick={() => revealItemInDir(site.path)}>Finder</button>
          {#if site.origin === "linked"}
            <button class="btn danger" onclick={() => run("unlink", () => api.unlink(site.name))}>Unlink</button>
          {/if}
        </div>

        <dl class="grid">
          <dt>Path</dt>
          <dd class="mono">{site.path}</dd>
          <dt>Document root</dt>
          <dd class="mono">{site.driver.docroot}</dd>
          <dt>PHP</dt>
          <dd>
            <select
              value={site.isolated ? site.php : ""}
              onchange={(e) => run("php", () => api.setSitePhp(site.name, e.currentTarget.value || null))}
              aria-label="PHP version for this site"
            >
              <option value="">Default ({data?.default_php ?? "none"})</option>
              {#each data?.php ?? [] as p (p.version)}
                <option value={p.version}>PHP {p.version}</option>
              {/each}
              {#if site.isolated && site.php && !data?.php.some((p) => p.version === site.php)}
                <option value={site.php}>PHP {site.php} (not installed)</option>
              {/if}
            </select>
          </dd>
          <dt>HTTPS</dt>
          <dd>
            <label class="toggle">
              <input
                type="checkbox"
                checked={site.secure}
                onchange={(e) => run("secure", () => api.setSiteSecure(site.name, e.currentTarget.checked))}
              />
              {site.secure ? "Enabled" : "Disabled"}
            </label>
          </dd>
        </dl>

        {#if project?.laravel}
          <section>
            <h2>Laravel</h2>
            <ul class="list">
              <li>
                <div><strong>Watchers</strong><small>Show queries, HTTP calls, logs, views, cache and jobs in Dumps → Activity. Adds a dev-only package to composer.json.</small></div>
                <label class="toggle">
                  <input type="checkbox" checked={project.watchers} disabled={!!busy}
                    onchange={(e) => toggleWatchers(site.name, e.currentTarget.checked)} />
                  {busy === "watch" ? "Working…" : project.watchers ? "On" : "Off"}
                </label>
              </li>
              {#each [["queue", "Queue worker", "php artisan queue:work"], ["schedule", "Scheduler", "php artisan schedule:work"], ["reverb", "Reverb", "WebSocket server on 127.0.0.1"]] as [kind, label, hint] (kind)}
                {#if kind !== "reverb" || project.reverb_available}
                  <li>
                    <div><strong>{label}</strong><small>{hint}. Runs while Corral runs and restarts if it crashes.</small></div>
                    <label class="toggle">
                      <input type="checkbox" checked={project.processes.includes(kind)} disabled={!!busy}
                        onchange={(e) => setProcess(site.name, kind, e.currentTarget.checked)} />
                      {project.processes.includes(kind) ? "On" : "Off"}
                    </label>
                  </li>
                {/if}
              {/each}
              {#if project.db && ["mysql", "mariadb", "pgsql"].includes(project.db.connection)}
                <li>
                  <div>
                    <strong>Database</strong>
                    <small>{project.db.connection} · {project.db.database ?? "no DB_DATABASE"}{project.db.username ? ` · user ${project.db.username}` : ""}{project.db.service ? ` · on ${project.db.service}` : " · no matching service yet"}</small>
                    {#if project.db.needs_dedicated_user}
                      <small class="warn">.env uses root with a password. Corral will switch it to its own user (your .env is backed up first):</small>
                      <input class="inline" bind:value={dbUser} aria-label="Database user" />
                    {/if}
                  </div>
                  <button class="btn small" disabled={!!busy || !project.db.service || !project.db.database}
                    onclick={() => createDb(site.name, !!project?.db?.needs_dedicated_user)}>
                    {busy === "db" ? "Creating…" : "Create database"}
                  </button>
                </li>
              {/if}
            </ul>
          </section>
        {/if}

        <section>
          <h2>Share</h2>
          {#if shareUrl}
            <p>Public at <button class="link mono" onclick={() => openUrl(shareUrl!)}>{shareUrl}</button></p>
            <p class="warn">Anyone with this link can open the site until you stop sharing.</p>
            <div class="buttons">
              <button class="btn" onclick={() => navigator.clipboard.writeText(shareUrl!)}>Copy link</button>
              <button class="btn danger" disabled={!!busy} onclick={() => run("share", () => api.shareStop(site.name))}>Stop sharing</button>
            </div>
          {:else}
            <p class="empty">Get a temporary public https link (Cloudflare quick tunnel, no account needed). Links your app builds from APP_URL still point to .{data?.tld}.</p>
            <button class="btn" disabled={!!busy || !(data?.running || data?.running_elsewhere)} onclick={() => run("share", () => api.shareStart(site.name))}>
              {busy === "share" ? "Opening tunnel…" : "Share publicly"}
            </button>
          {/if}
        </section>

        <section>
          <h2>Environment (.env)</h2>
          {#if envFor === site.name && envText !== null}
            <textarea class="code" spellcheck="false" bind:value={envText} aria-label=".env contents"></textarea>
            <div class="buttons">
              <button class="btn primary" onclick={saveEnv} disabled={!!busy}>Save .env</button>
              <button class="btn" onclick={() => { envFor = null; envText = null; }}>Close</button>
            </div>
          {:else}
            <p class="empty">Contains secrets, so it stays hidden until you open it.</p>
            <button class="btn" onclick={() => loadEnv(site.name)} disabled={!!busy}>Show and edit .env</button>
          {/if}
        </section>

        <section>
          <div class="head">
            <h2>Logs</h2>
            <span class="row">
              <select bind:value={logLevel} onchange={() => logs && loadLogs(site.name)} aria-label="Minimum level">
                <option value="">All levels</option>
                <option value="warning">Warning+</option>
                <option value="error">Error+</option>
              </select>
              <button class="btn small" onclick={() => loadLogs(site.name)} disabled={!!busy}>{logs ? "Refresh" : "Show logs"}</button>
            </span>
          </div>
          {#if logs}
            {#each logs as e, i (i)}
              <div class="log lvl-{e.level}">
                <button class="rowbtn" onclick={() => (openTrace = openTrace === i ? null : i)}>
                  <span class="badge">{e.level}</span>
                  <small>{e.time}</small>
                  <span class="msg">{e.message.length > 240 ? e.message.slice(0, 240) + "…" : e.message}</span>
                </button>
                {#if openTrace === i}
                  <pre class="mono box">{e.message}{e.details ? "\n\n" + e.details : ""}</pre>
                {/if}
              </div>
            {:else}
              <p class="empty">No log entries{logLevel ? " at this level" : ""}.</p>
            {/each}
          {/if}
        </section>
      {:else if view === "mail"}
        <div class="head">
          <h1>Mail</h1>
          <button class="btn small" disabled={!mails.length} onclick={() => act("clear", async () => { await api.mailClear(); mail = null; mails = []; })}>Clear all</button>
        </div>
        <p class="sub">Every mail sent to SMTP 127.0.0.1:2525 is caught here and never delivered. In Laravel: <code>MAIL_MAILER=smtp MAIL_HOST=127.0.0.1 MAIL_PORT=2525</code></p>
        {#if mail}
          <button class="link" onclick={() => (mail = null)}>← All mail</button>
          <h2 class="subject">{mail.subject || "(no subject)"}</h2>
          <dl class="grid tight">
            <dt>From</dt><dd>{mail.from}</dd>
            <dt>To</dt><dd>{mail.to.join(", ")}</dd>
            {#if mail.cc.length}<dt>Cc</dt><dd>{mail.cc.join(", ")}</dd>{/if}
            {#if mail.envelope_to.join() !== mail.to.join()}<dt>Envelope</dt><dd>{mail.envelope_to.join(", ")}</dd>{/if}
            <dt>Received</dt><dd>{fmtTime(mail.received)} · {fmtSize(mail.size)}</dd>
          </dl>
          {#if mail.attachment_list.length}
            <div class="buttons">
              {#each mail.attachment_list as a (a.index)}
                <button class="btn small" onclick={() => act("att", async () => {
                  const p = await api.mailSaveAttachment(mail!.id, a.index);
                  notice = `Saved to ${p}`;
                  revealItemInDir(p);
                })}>📎 {a.name} ({fmtSize(a.size)})</button>
              {/each}
            </div>
          {/if}
          <div class="seg">
            <button class:active={mailPane === "html"} disabled={!mail.html} onclick={() => (mailPane = "html")}>HTML</button>
            <button class:active={mailPane === "text"} disabled={!mail.text} onclick={() => (mailPane = "text")}>Text</button>
            <button class:active={mailPane === "headers"} onclick={() => (mailPane = "headers")}>Headers</button>
            <button class="danger" onclick={() => act("del", async () => { await api.mailDelete(mail!.id); mail = null; await refreshInbox(); })}>Delete</button>
          </div>
          {#if mailPane === "html" && mail.html}
            <!-- No sandbox permissions: scripts, forms and navigation in mail are blocked. -->
            <iframe class="mailframe" title="Mail HTML" sandbox="" srcdoc={mail.html}></iframe>
          {:else if mailPane === "text"}
            <pre class="mono box">{mail.text ?? ""}</pre>
          {:else}
            <table class="headers">
              <tbody>
                {#each mail.headers as [k, v], i (i)}<tr><th>{k}</th><td>{v}</td></tr>{/each}
              </tbody>
            </table>
          {/if}
        {:else}
          <ul class="list">
            {#each mails as m (m.id)}
              <li class="clickable">
                <button class="rowbtn" onclick={() => openMail(m.id)}>
                  <strong>{m.subject || "(no subject)"}</strong>
                  <small>{m.from} → {m.to.join(", ")} · {fmtTime(m.received)}{m.attachments ? ` · 📎 ${m.attachments}` : ""}</small>
                </button>
              </li>
            {:else}
              <p class="empty">No mail caught yet.</p>
            {/each}
          </ul>
        {/if}
      {:else if view === "dumps"}
        <div class="head">
          <h1>{dumpPane === "dumps" ? "Dumps" : "Activity"}</h1>
          {#if dumpPane === "dumps"}
            <button class="btn small" disabled={!dumps.length} onclick={() => act("clear", async () => { await api.dumpsClear(); dumps = []; })}>Clear</button>
          {:else}
            <button class="btn small" disabled={!events.length} onclick={() => act("clear", async () => { await api.eventsClear(); events = []; })}>Clear</button>
          {/if}
        </div>
        <div class="seg">
          <button class:active={dumpPane === "dumps"} onclick={() => (dumpPane = "dumps")}>dump() / dd()</button>
          <button class:active={dumpPane === "activity"} onclick={() => (dumpPane = "activity")}>Queries, HTTP, logs…</button>
        </div>
        {#if dumpPane === "activity"}
          <p class="sub">From Laravel sites with watchers turned on (open a site → Laravel → Watchers).</p>
          <div class="seg">
            {#each ["", "query", "http", "log", "view", "cache", "job"] as k (k)}
              <button class:active={eventKind === k} onclick={() => (eventKind = k)}>{k || "all"}</button>
            {/each}
          </div>
          {#each shownEvents as e (e.id)}
            <div class="log">
              <div class="rowbtn">
                <span class="badge">{e.kind}</span>
                <small>{new Date(e.time * 1000).toLocaleTimeString()}</small>
                <span class="msg mono">{eventSummary(e)}</span>
              </div>
            </div>
          {:else}
            <p class="empty">No activity yet.</p>
          {/each}
        {:else}
        <p class="sub">Output of <code>dump()</code> and <code>dd()</code> from your sites while Corral is running. Your pages stay clean.</p>
        {#each dumps as d (d.id)}
          <article class="dump">
            <header>
              <span class="mono">{d.file ? `${d.file.split("/").slice(-3).join("/")}:${d.line ?? ""}` : "unknown source"}</span>
              <small>{new Date(d.time * 1000).toLocaleTimeString()}{d.uri ? ` · ${d.uri}` : ""}</small>
            </header>
            <pre class="mono">{d.text}</pre>
          </article>
        {:else}
          <p class="empty">No dumps yet. Add <code>dump($something)</code> to a controller and reload the page.</p>
        {/each}
        {/if}
      {:else if view === "services" && data}
        <h1>Services</h1>
        <p class="sub">Run on 127.0.0.1 only. Data stays in Corral's folder until you remove it.</p>
        <ul class="list">
          {#each data.services as svc (svc.name)}
            <li>
              <div class="svc">
                <span class="dot" class:on={svc.listening}></span>
                <div>
                  <strong>{svc.name}</strong>
                  <small>{svc.label} {svc.version} · 127.0.0.1:{svc.port} · {svc.listening ? "listening" : "not running"}</small>
                </div>
              </div>
              <label class="toggle">
                <input
                  type="checkbox"
                  checked={svc.enabled}
                  onchange={(e) => run("svc", () => api.setServiceEnabled(svc.name, e.currentTarget.checked))}
                />
                {svc.enabled ? "Enabled" : "Disabled"}
              </label>
            </li>
          {:else}
            <p class="empty">No services yet.</p>
          {/each}
        </ul>
        <div class="buttons">
          {#if !data.services.some((s) => s.kind === "mysql")}
            <button class="btn primary" disabled={!!busy} onclick={() => run("mysql", () => api.install("mysql"))}>
              {busy === "mysql" ? "Installing MySQL (≈170 MB)…" : "Add MySQL 8.4"}
            </button>
          {/if}
          {#if !data.services.some((s) => s.kind === "postgres")}
            <button class="btn" disabled={!!busy} onclick={() => run("postgres", () => api.install("postgres"))}
              title="Built from verified source; needs Xcode command line tools">
              {busy === "postgres" ? "Building PostgreSQL (a few minutes)…" : "Add PostgreSQL 18"}
            </button>
          {/if}
          {#if !data.services.some((s) => s.kind === "mongodb")}
            <button class="btn" disabled={!!busy} onclick={() => run("mongodb", () => api.install("mongodb"))}
              title="SSPL licensed; downloaded from MongoDB">
              {busy === "mongodb" ? "Installing MongoDB…" : "Add MongoDB 8.2"}
            </button>
          {/if}
          {#if !data.services.some((s) => s.kind === "valkey")}
            <button class="btn" disabled={!!busy} onclick={() => run("valkey", () => api.install("valkey"))}
              title="Redis-compatible; built from verified source; needs Xcode command line tools">
              {busy === "valkey" ? "Building Valkey (about a minute)…" : "Add Valkey (Redis)"}
            </button>
          {/if}
        </div>
        <h2>Database GUI</h2>
        <ul class="list">
          <li>
            <div><strong>Adminer</strong><small>Browse and edit MySQL, PostgreSQL and SQLite at https://adminer.{data.tld}. Only reachable from this Mac.</small></div>
            {#if data.adminer}
              <span class="row">
                <button class="btn small primary" onclick={() => openUrl(`https://adminer.${data!.tld}${data!.https_port === 443 ? "" : ":" + data!.https_port}`)}>Open</button>
                <button class="btn small" disabled={!!busy} onclick={() => run("adminer", api.adminerRemove)}>Remove</button>
              </span>
            {:else}
              <button class="btn small" disabled={!!busy} onclick={() => run("adminer", () => api.install("adminer"))}>
                {busy === "adminer" ? "Installing…" : "Install"}
              </button>
            {/if}
          </li>
        </ul>
        {#if !data.running && !data.running_elsewhere && data.services.length}
          <p class="empty">Services start together with Corral.</p>
        {/if}
      {:else if view === "settings" && data}
        <h1>Settings</h1>
        <section>
          <h2>General</h2>
          <ul class="list">
            <li>
              <div><strong>Launch at login</strong><small>Start Corral in the menu bar and serve sites after login.</small></div>
              <label class="toggle"><input type="checkbox" checked={data.autostart}
                onchange={(e) => run("auto", () => api.setAutostart(e.currentTarget.checked))} /></label>
            </li>
            <li>
              <div><strong>Use Corral in the terminal</strong><small>Adds ~/.corral/bin to your PATH in new terminals.</small></div>
              <label class="toggle"><input type="checkbox" checked={data.path_configured}
                onchange={(e) => run("path", () => api.setShellPath(e.currentTarget.checked))} /></label>
            </li>
          </ul>
        </section>
        <section>
          <h2>PHP versions</h2>
          <ul class="list">
            {#each data.php as p (p.version)}
              <li>
                <span>PHP {p.version}</span>
                <span class="row">
                  <button class="btn small" onclick={() => editIni(p.version)}>php.ini…</button>
                  {#if p.is_default}<span class="badge">default</span>{:else}
                    <button class="btn small" onclick={() => run("defphp", () => api.setDefaultPhp(p.version))}>Make default</button>
                  {/if}
                </span>
              </li>
            {/each}
          </ul>
          {#if iniVersion}
            <h2>PHP {iniVersion} overrides (90-user.ini)</h2>
            <textarea class="code" spellcheck="false" bind:value={iniText} aria-label="php.ini overrides"></textarea>
            <div class="buttons">
              <button class="btn primary" onclick={saveIni} disabled={!!busy}>Save</button>
              <button class="btn" onclick={() => (iniVersion = null)}>Cancel</button>
            </div>
          {/if}
          <div class="buttons">
            {#each ["8.3", "8.2"] as v (v)}
              {#if !data.php.some((p) => p.version === v)}
                <button class="btn small" disabled={!!busy} onclick={() => run("php" + v, () => api.install("php", v))}>
                  {busy === "php" + v ? "Installing…" : `Install PHP ${v}`}
                </button>
              {/if}
            {/each}
          </div>
        </section>
        <section>
          <h2>Node.js</h2>
          <ul class="list">
            {#each data.node as n (n.major)}
              <li>
                <span>Node {n.version || n.major}</span>
                {#if n.is_default}<span class="badge">default</span>{:else}
                  <button class="btn small" onclick={() => run("nodeuse", () => api.nodeUse(n.major))}>Make default</button>
                {/if}
              </li>
            {:else}
              <p class="empty">No Node versions installed.</p>
            {/each}
          </ul>
          <div class="buttons">
            <button class="btn small" disabled={!!busy} onclick={() => run("node", () => api.nodeInstall())}>
              {busy === "node" ? "Installing…" : "Install latest LTS"}
            </button>
            {#each [24, 22] as m (m)}
              {#if !data.node.some((n) => n.major === m)}
                <button class="btn small" disabled={!!busy} onclick={() => run("node" + m, () => api.nodeInstall(m))}>
                  {busy === "node" + m ? "Installing…" : `Install Node ${m}`}
                </button>
              {/if}
            {/each}
          </div>
        </section>
        <section>
          <h2>Parked folders</h2>
          <ul class="list">
            {#each data.parked as dir (dir)}
              <li>
                <span class="mono">{dir}</span>
                <button class="btn small" onclick={() => run("unpark", () => api.unpark(dir))}>Remove</button>
              </li>
            {:else}
              <p class="empty">No parked folders.</p>
            {/each}
          </ul>
        </section>
        <section>
          <h2>System</h2>
          <ul class="list">
            <li><span>.{data.tld} domains point to Corral</span><span class="badge" class:ok={data.system.dns}>{data.system.dns ? "on" : "off"}</span></li>
            <li><span>HTTPS certificates trusted</span><span class="badge" class:ok={data.system.https}>{data.system.https ? "on" : "off"}</span></li>
            <li><span>Ports 80/443 (URLs without a port number)</span><span class="badge" class:ok={data.system.ports}>{data.system.ports ? "on" : "off"}</span></li>
          </ul>
          <div class="buttons">
            {#if !(data.system.dns && data.system.https && data.system.ports)}
              <button class="btn primary" disabled={!!busy || data.system.ports_busy} onclick={() => run("system", api.systemSetup)}>
                {busy === "system" ? "Waiting for macOS…" : "Set up"}
              </button>
            {/if}
            {#if data.system.dns || data.system.https || data.system.ports}
              <button class="btn" disabled={!!busy} onclick={() => run("system", api.systemUndo)}>Remove</button>
            {/if}
          </div>
          {#if data.system.ports_busy}<p class="warn">Another app is using ports 80/443. Quit it to use plain https://site.{data.tld} URLs.</p>{/if}
          <p class="sub">Sites on .{data.tld} · HTTP :{data.http_port} · HTTPS :{data.https_port}</p>
        </section>
        <section>
          <h2>About</h2>
          <p class="sub">Corral {update?.current ?? ""}{update?.latest ? ` · latest release ${update.latest}` : ""}</p>
          <button class="btn small" disabled={!!busy} onclick={() => act("upd", async () => {
            update = await api.checkUpdate();
            notice = update.newer ? `Corral ${update.latest} is available.` : "Corral is up to date.";
          })}>Check for updates</button>
        </section>
        <section>
          <h2>Uninstall</h2>
          {#if !confirmUninstall}
            <button class="btn danger" onclick={() => (confirmUninstall = true)}>Uninstall Corral…</button>
          {:else}
            <p>This removes Corral's runtimes, settings, certificates and system integration, then quits. Your projects are never touched.</p>
            <label class="toggle"><input type="checkbox" bind:checked={deleteData} /> Also delete databases (MySQL, PostgreSQL, Valkey data)</label>
            <div class="buttons">
              <button class="btn danger" disabled={!!busy} onclick={() => act("uninstall", () => api.uninstall(deleteData))}>
                {busy === "uninstall" ? "Removing…" : "Uninstall now"}
              </button>
              <button class="btn" onclick={() => (confirmUninstall = false)}>Cancel</button>
            </div>
          {/if}
        </section>
      {:else if data}
        <h1>Sites</h1>
        <p class="sub">{data.sites.length} sites on .{data.tld} · {statusText}</p>
        <ul class="list">
          {#each data.sites as s (s.name)}
            <li>
              <button class="link" onclick={() => selectSite(s.name)}>{s.host}</button>
              <span class="meta">{s.driver_label} · PHP {s.php ?? "–"} · {s.secure ? "HTTPS" : "HTTP"}</span>
            </li>
          {:else}
            <p class="empty">No sites. Park a folder or link a project.</p>
          {/each}
        </ul>
      {/if}
    </main>
  </div>
{/if}

<style>
  :global(:root) {
    --bg: #f6f6f4;
    --panel: #ffffff;
    --text: #1d1d1b;
    --muted: #6b6b66;
    --border: #e2e2dd;
    --accent: #2f6f4f;
    --accent-text: #ffffff;
    --danger: #b3261e;
    --hover: #efefeb;
    --ok: #2e9d5b;
    font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", system-ui, sans-serif;
    font-size: 14px;
    color: var(--text);
    background: var(--bg);
    -webkit-font-smoothing: antialiased;
  }
  @media (prefers-color-scheme: dark) {
    :global(:root) {
      --bg: #1b1c1a;
      --panel: #232421;
      --text: #ecece8;
      --muted: #9a9a93;
      --border: #34352f;
      --accent: #5fb38a;
      --accent-text: #0f1a14;
      --danger: #f08b84;
      --hover: #2c2d29;
    }
  }
  :global(body) { margin: 0; background: var(--bg); }

  .shell { display: grid; grid-template-columns: 280px 1fr; height: 100vh; }
  .sidebar {
    display: flex; flex-direction: column; gap: 8px; padding: 12px;
    border-right: 1px solid var(--border); background: var(--panel); min-height: 0;
  }
  .brand { display: flex; align-items: center; gap: 8px; padding: 4px 4px 4px; }
  .brand .title { flex: 1; display: flex; flex-direction: column; line-height: 1.2; }
  .brand strong { font-size: 15px; }
  .brand small, small { color: var(--muted); font-size: 12px; }
  .dot { width: 8px; height: 8px; border-radius: 50%; background: var(--border); flex: none; }
  .dot.on { background: var(--ok); }
  .tabs { display: flex; gap: 2px; background: var(--bg); border-radius: 7px; padding: 2px; }
  .tabs button {
    flex: 1; border: 0; background: none; color: var(--muted); font: inherit; font-size: 12.5px;
    padding: 5px 0; border-radius: 5px;
  }
  .tabs button.active { background: var(--panel); color: var(--text); box-shadow: 0 0 0 1px var(--border); }
  .search {
    padding: 7px 10px; border-radius: 6px; border: 1px solid var(--border);
    background: var(--bg); color: var(--text); font: inherit;
  }
  .sites { flex: 1; overflow-y: auto; display: flex; flex-direction: column; gap: 2px; }
  .site-row {
    text-align: left; border: 0; background: none; color: inherit; font: inherit;
    padding: 7px 8px; border-radius: 6px; display: flex; flex-direction: column;
  }
  .site-row:hover { background: var(--hover); }
  .site-row.active { background: var(--accent); color: var(--accent-text); }
  .site-row.active .meta { color: inherit; opacity: 0.8; }
  .host { font-weight: 500; }
  .meta { font-size: 12px; color: var(--muted); }
  .actions { display: flex; gap: 6px; }
  .actions .btn { flex: 1; }

  .detail { padding: 28px 32px; overflow-y: auto; }
  h1 { margin: 0; font-size: 22px; font-weight: 600; }
  h2 { font-size: 13px; text-transform: uppercase; letter-spacing: 0.04em; color: var(--muted); margin: 28px 0 8px; }
  .sub { margin: 4px 0 20px; color: var(--muted); }
  .buttons { display: flex; flex-wrap: wrap; gap: 8px; margin: 12px 0 24px; }
  .grid { display: grid; grid-template-columns: 140px 1fr; gap: 12px 16px; margin: 0; }
  dt { color: var(--muted); }
  dd { margin: 0; min-width: 0; }
  .mono { font-family: ui-monospace, "SF Mono", Menlo, monospace; font-size: 12.5px; word-break: break-all; }

  .btn {
    font: inherit; padding: 6px 12px; border-radius: 6px; border: 1px solid var(--border);
    background: var(--panel); color: var(--text);
  }
  .btn:hover:not(:disabled) { background: var(--hover); }
  .btn:disabled { opacity: 0.5; }
  .btn.small { padding: 3px 9px; font-size: 12px; }
  .btn.primary { background: var(--accent); color: var(--accent-text); border-color: transparent; }
  .btn.primary:hover:not(:disabled) { background: var(--accent); filter: brightness(1.08); }
  .btn.danger { color: var(--danger); }
  .btn.add { margin-top: 12px; }
  .link { border: 0; background: none; padding: 0; font: inherit; font-weight: 500; color: var(--accent); text-align: left; }
  select { font: inherit; padding: 4px 6px; border-radius: 6px; border: 1px solid var(--border); background: var(--panel); color: var(--text); }
  .toggle { display: inline-flex; gap: 8px; align-items: center; white-space: nowrap; }

  .list { list-style: none; padding: 0; margin: 0; display: flex; flex-direction: column; gap: 6px; }
  .list li {
    display: flex; align-items: center; justify-content: space-between; gap: 12px;
    padding: 10px 12px; background: var(--panel); border: 1px solid var(--border); border-radius: 8px;
  }
  .list li > div { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .svc { flex-direction: row !important; align-items: center; gap: 10px !important; }
  .svc > div { display: flex; flex-direction: column; }
  .badge { font-size: 11px; padding: 2px 8px; border-radius: 99px; background: var(--hover); color: var(--muted); white-space: nowrap; }
  .badge.ok { color: var(--ok); }
  .empty { color: var(--muted); }
  .error {
    border: 1px solid var(--danger); color: var(--danger); padding: 10px 12px;
    border-radius: 8px; margin-bottom: 16px; white-space: pre-wrap;
  }
  code { font-family: ui-monospace, Menlo, monospace; font-size: 12.5px; }

  .notice {
    border: 1px solid var(--border); background: var(--panel); padding: 10px 12px;
    border-radius: 8px; margin-bottom: 16px;
  }
  textarea.code {
    width: 100%; box-sizing: border-box; min-height: 260px; resize: vertical;
    font-family: ui-monospace, "SF Mono", Menlo, monospace; font-size: 12.5px; line-height: 1.5;
    padding: 10px 12px; border-radius: 8px; border: 1px solid var(--border);
    background: var(--panel); color: var(--text);
  }
  .row { display: flex; gap: 6px; align-items: center; }
  .newform { display: flex; flex-direction: column; gap: 6px; padding: 8px; border: 1px solid var(--border); border-radius: 8px; }
  .newform input, .newform select {
    font: inherit; padding: 6px 8px; border-radius: 6px; border: 1px solid var(--border);
    background: var(--bg); color: var(--text);
  }
  .tabs { flex-wrap: wrap; }
  .tabs button { flex: 1 0 30%; }
  .head { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  .head h2 { margin: 28px 0 8px; }
  .subject { text-transform: none; letter-spacing: 0; color: var(--text); font-size: 17px; margin: 14px 0 10px; }
  .grid.tight { gap: 4px 12px; margin-bottom: 12px; }
  .seg { display: flex; gap: 4px; margin: 12px 0 8px; }
  .seg button { font: inherit; font-size: 12.5px; padding: 4px 10px; border-radius: 6px; border: 1px solid var(--border); background: var(--panel); color: var(--text); }
  .seg button.active { background: var(--accent); color: var(--accent-text); border-color: transparent; }
  .seg button.danger { margin-left: auto; color: var(--danger); }
  .mailframe { width: 100%; height: 60vh; border: 1px solid var(--border); border-radius: 8px; background: #fff; }
  .box { white-space: pre-wrap; padding: 10px 12px; border: 1px solid var(--border); border-radius: 8px; background: var(--panel); max-height: 60vh; overflow: auto; }
  table.headers { border-collapse: collapse; width: 100%; font-size: 12.5px; }
  table.headers th { text-align: left; vertical-align: top; padding: 4px 10px 4px 0; color: var(--muted); white-space: nowrap; font-weight: 500; }
  table.headers td { padding: 4px 0; word-break: break-all; font-family: ui-monospace, Menlo, monospace; }
  .list li.clickable { padding: 0; }
  .rowbtn { all: unset; display: flex; flex-direction: column; gap: 2px; width: 100%; padding: 10px 12px; box-sizing: border-box; cursor: default; }
  .rowbtn:hover { background: var(--hover); border-radius: 8px; }
  .dump { border: 1px solid var(--border); border-radius: 8px; background: var(--panel); margin-bottom: 10px; overflow: hidden; }
  .dump header { display: flex; justify-content: space-between; gap: 8px; padding: 8px 12px; border-bottom: 1px solid var(--border); }
  .dump pre { margin: 0; padding: 10px 12px; white-space: pre-wrap; max-height: 40vh; overflow: auto; }
  .log { border-bottom: 1px solid var(--border); }
  .log .rowbtn { flex-direction: row; align-items: baseline; gap: 8px; padding: 6px 4px; }
  .log .msg { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .log.lvl-error .badge, .log.lvl-critical .badge, .log.lvl-alert .badge, .log.lvl-emergency .badge { color: var(--danger); }
  .log.lvl-warning .badge { color: #b7791f; }
  .warn { color: #b7791f; }
  input.inline {
    font: inherit; padding: 4px 8px; margin-top: 4px; border-radius: 6px; border: 1px solid var(--border);
    background: var(--bg); color: var(--text); width: 200px;
  }
  .update {
    font: inherit; font-size: 12.5px; text-align: left; padding: 8px 10px; border-radius: 8px;
    border: 1px solid var(--accent); background: var(--panel); color: var(--accent);
  }
  .setup { max-width: 640px; margin: 0 auto; padding: 40px 24px; }
  .steps { list-style: none; padding: 0; margin: 0 0 24px; display: flex; flex-direction: column; gap: 10px; counter-reset: step; }
  .steps li {
    counter-increment: step; display: flex; gap: 16px; align-items: center; justify-content: space-between;
    padding: 14px 16px; background: var(--panel); border: 1px solid var(--border); border-radius: 10px;
  }
  .steps li > div::before { content: counter(step) ". "; color: var(--muted); }
  .steps li.done { opacity: 0.75; }
  .steps p { margin: 4px 0 0; color: var(--muted); font-size: 13px; }
</style>
