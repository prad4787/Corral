<script lang="ts">
  import { onMount } from "svelte";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
  import { api, type Overview } from "$lib/api";

  type View = "overview" | "site" | "services" | "settings";

  let data = $state<Overview | null>(null);
  let view = $state<View>("overview");
  let selected = $state<string | null>(null);
  let query = $state("");
  let error = $state<string | null>(null);
  let busy = $state<string | null>(null);
  let skipSetup = $state(false);

  const site = $derived(data?.sites.find((s) => s.name === selected) ?? null);
  const filtered = $derived(
    (data?.sites ?? []).filter((s) => s.host.includes(query.toLowerCase().trim())),
  );
  const needsSetup = $derived(
    !!data && !skipSetup && (!data.caddy_installed || data.php.length === 0 || data.parked.length + data.sites.length === 0),
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
  }

  onMount(() => {
    run("load", api.overview);
    // Pick up changes made from the CLI.
    const t = setInterval(
      () => !busy && api.overview().then((d) => (data = d)).catch(() => {}),
      3000,
    );
    return () => clearInterval(t);
  });
</script>

{#if needsSetup && data}
  <main class="setup">
    <h1>Set up Corral</h1>
    <p class="sub">Free, local PHP development. Each step can be undone with <code>corral uninstall</code>.</p>

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
        <button class:active={view === "services"} onclick={() => (view = "services")}>Services</button>
        <button class:active={view === "settings"} onclick={() => (view = "settings")}>Settings</button>
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

      <footer class="actions">
        <button class="btn" onclick={addParked} disabled={!!busy}>Park folder…</button>
        <button class="btn" onclick={linkFolder} disabled={!!busy}>Link project…</button>
      </footer>
    </aside>

    <main class="detail">
      {#if error}
        <div class="error" role="alert">{error}</div>
      {/if}

      {#if view === "site" && site}
        <h1>{site.host}</h1>
        <p class="sub">{site.driver_label} · {site.origin === "linked" ? "Linked" : "Parked"}</p>

        <div class="buttons">
          <button class="btn primary" onclick={() => openUrl(site.url)}>Open {site.url}</button>
          <button class="btn" onclick={() => act("editor", () => api.openIn(site.path, "editor"))}>Open in editor</button>
          <button class="btn" onclick={() => act("terminal", () => api.openIn(site.path, "terminal"))}>Terminal</button>
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
        {#if !data.services.some((s) => s.kind === "mysql")}
          <button class="btn primary add" disabled={!!busy} onclick={() => run("mysql", () => api.install("mysql"))}>
            {busy === "mysql" ? "Installing MySQL (≈170 MB)…" : "Add MySQL 8.4"}
          </button>
        {/if}
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
                {#if p.is_default}<span class="badge">default</span>{:else}
                  <button class="btn small" onclick={() => run("defphp", () => api.setDefaultPhp(p.version))}>Make default</button>
                {/if}
              </li>
            {/each}
          </ul>
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
          <h2>Network</h2>
          <p class="sub">Sites on .{data.tld} · HTTP :{data.http_port} · HTTPS :{data.https_port}</p>
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
