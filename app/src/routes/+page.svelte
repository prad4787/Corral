<script lang="ts">
  import { onMount } from "svelte";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
  import { api, type Overview } from "$lib/api";

  let data = $state<Overview | null>(null);
  let selected = $state<string | null>(null);
  let query = $state("");
  let error = $state<string | null>(null);
  let busy = $state(false);

  const site = $derived(data?.sites.find((s) => s.name === selected) ?? null);
  const filtered = $derived(
    (data?.sites ?? []).filter((s) => s.host.includes(query.toLowerCase().trim())),
  );

  async function run(op: () => Promise<Overview>) {
    busy = true;
    error = null;
    try {
      data = await op();
      if (selected && !data.sites.some((s) => s.name === selected)) selected = null;
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function pickDir(): Promise<string | null> {
    const dir = await openDialog({ directory: true, multiple: false });
    return typeof dir === "string" ? dir : null;
  }

  async function addParked() {
    const dir = await pickDir();
    if (dir) await run(() => api.park(dir));
  }

  async function linkFolder() {
    const dir = await pickDir();
    if (dir) await run(() => api.link(dir));
  }

  onMount(() => {
    run(api.overview);
    // Pick up changes made from the CLI.
    const t = setInterval(() => !busy && api.overview().then((d) => (data = d)).catch(() => {}), 3000);
    return () => clearInterval(t);
  });
</script>

<div class="shell">
  <aside class="sidebar">
    <header class="brand">
      <span class="dot" class:on={data?.running}></span>
      <strong>Corral</strong>
      <button
        class="btn small"
        disabled={busy || !data?.caddy_installed}
        title={data?.caddy_installed ? "" : "Run `corral setup` first"}
        onclick={() => run(data?.running ? api.stop : api.start)}
      >
        {data?.running ? "Stop" : "Start"}
      </button>
    </header>

    <input class="search" placeholder="Search sites" bind:value={query} aria-label="Search sites" />

    <nav class="sites" aria-label="Sites">
      {#each filtered as s (s.name)}
        <button class="site-row" class:active={s.name === selected} onclick={() => (selected = s.name)}>
          <span class="host">{s.host}</span>
          <span class="meta">{s.driver_label} · PHP {s.php ?? "–"}</span>
        </button>
      {:else}
        <p class="empty">{data ? "No sites yet." : "Loading…"}</p>
      {/each}
    </nav>

    <footer class="actions">
      <button class="btn" onclick={addParked} disabled={busy}>Park folder…</button>
      <button class="btn" onclick={linkFolder} disabled={busy}>Link project…</button>
    </footer>
  </aside>

  <main class="detail">
    {#if error}
      <div class="error" role="alert">{error}</div>
    {/if}

    {#if site}
      <h1>{site.host}</h1>
      <p class="sub">{site.driver_label} · {site.origin === "linked" ? "Linked" : "Parked"}</p>

      <div class="buttons">
        <button class="btn primary" onclick={() => openUrl(site.url)}>Open {site.url}</button>
        <button class="btn" onclick={() => revealItemInDir(site.path)}>Show in Finder</button>
        {#if site.origin === "linked"}
          <button class="btn danger" onclick={() => run(() => api.unlink(site.name))}>Unlink</button>
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
            onchange={(e) => run(() => api.setSitePhp(site.name, e.currentTarget.value || null))}
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
              onchange={(e) => run(() => api.setSiteSecure(site.name, e.currentTarget.checked))}
            />
            {site.secure ? "Enabled" : "Disabled"}
          </label>
        </dd>
      </dl>
    {:else if data}
      <h1>Overview</h1>
      <p class="sub">{data.sites.length} sites on .{data.tld}</p>

      <section>
        <h2>PHP versions</h2>
        {#if data.php.length === 0}
          <p class="empty">None installed. Run <code>corral php install 8.4</code>.</p>
        {/if}
        <ul class="list">
          {#each data.php as p (p.version)}
            <li>
              <span>PHP {p.version}</span>
              {#if p.is_default}
                <span class="badge">default</span>
              {:else}
                <button class="btn small" onclick={() => run(() => api.setDefaultPhp(p.version))}>
                  Make default
                </button>
              {/if}
            </li>
          {/each}
        </ul>
      </section>

      <section>
        <h2>Parked folders</h2>
        <ul class="list">
          {#each data.parked as dir (dir)}
            <li>
              <span class="mono">{dir}</span>
              <button class="btn small" onclick={() => run(() => api.unpark(dir))}>Remove</button>
            </li>
          {:else}
            <p class="empty">No parked folders.</p>
          {/each}
        </ul>
      </section>
    {/if}
  </main>
</div>

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
  .brand { display: flex; align-items: center; gap: 8px; padding: 4px 4px 8px; }
  .brand strong { flex: 1; font-size: 15px; }
  .dot { width: 8px; height: 8px; border-radius: 50%; background: var(--border); }
  .dot.on { background: var(--ok); }
  .search {
    padding: 7px 10px; border-radius: 6px; border: 1px solid var(--border);
    background: var(--bg); color: var(--text); font: inherit;
  }
  .sites { flex: 1; overflow-y: auto; display: flex; flex-direction: column; gap: 2px; }
  .site-row {
    text-align: left; border: 0; background: none; color: inherit; font: inherit;
    padding: 7px 8px; border-radius: 6px; display: flex; flex-direction: column; cursor: default;
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
  .buttons { display: flex; flex-wrap: wrap; gap: 8px; margin-bottom: 24px; }
  .grid { display: grid; grid-template-columns: 140px 1fr; gap: 12px 16px; margin: 0; }
  dt { color: var(--muted); }
  dd { margin: 0; min-width: 0; }
  .mono { font-family: ui-monospace, "SF Mono", Menlo, monospace; font-size: 12.5px; word-break: break-all; }

  .btn {
    font: inherit; padding: 6px 12px; border-radius: 6px; border: 1px solid var(--border);
    background: var(--panel); color: var(--text); cursor: default;
  }
  .btn:hover:not(:disabled) { background: var(--hover); }
  .btn:disabled { opacity: 0.5; }
  .btn.small { padding: 3px 9px; font-size: 12px; }
  .btn.primary { background: var(--accent); color: var(--accent-text); border-color: transparent; }
  .btn.danger { color: var(--danger); }
  select { font: inherit; padding: 4px 6px; border-radius: 6px; border: 1px solid var(--border); background: var(--panel); color: var(--text); }
  .toggle { display: inline-flex; gap: 8px; align-items: center; }

  .list { list-style: none; padding: 0; margin: 0; display: flex; flex-direction: column; gap: 6px; }
  .list li {
    display: flex; align-items: center; justify-content: space-between; gap: 12px;
    padding: 8px 12px; background: var(--panel); border: 1px solid var(--border); border-radius: 8px;
  }
  .badge { font-size: 11px; padding: 2px 8px; border-radius: 99px; background: var(--hover); color: var(--muted); }
  .empty { color: var(--muted); }
  .error {
    border: 1px solid var(--danger); color: var(--danger); padding: 10px 12px;
    border-radius: 8px; margin-bottom: 16px;
  }
  code { font-family: ui-monospace, Menlo, monospace; }
</style>
