<!-- One site: actions in the toolbar, settings in grouped sections. -->
<script lang="ts">
  import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
  import Toolbar from "$lib/ui/Toolbar.svelte";
  import Group from "$lib/ui/Group.svelte";
  import Row from "$lib/ui/Row.svelte";
  import Button from "$lib/ui/Button.svelte";
  import Switch from "$lib/ui/Switch.svelte";
  import PopUp from "$lib/ui/PopUp.svelte";
  import TextField from "$lib/ui/TextField.svelte";
  import Sheet from "$lib/ui/Sheet.svelte";
  import Icon from "$lib/Icon.svelte";
  import { api, type ProjectInfo, type LogEntry } from "$lib/api";
  import { app } from "$lib/app.svelte";

  const d = $derived(app.data!);
  const site = $derived(app.site!);
  const shareUrl = $derived(d.shares.find(([n]) => n === site.name)?.[1] ?? null);
  const engineUp = $derived(d.running || d.running_elsewhere);

  let project = $state<ProjectInfo | null>(null);
  let dbUser = $state("");
  let envText = $state<string | null>(null);
  let logs = $state<LogEntry[] | null>(null);
  let logLevel = $state("");
  let openLog = $state<number | null>(null);
  let confirmUnlink = $state(false);

  // Reload per-site details whenever another site is selected.
  $effect(() => {
    const name = site.name;
    project = null;
    logs = null;
    envText = null;
    api.projectInfo(name).then((p) => {
      if (name !== app.selected) return;
      project = p;
      dbUser = p.db?.database?.replace(/[^A-Za-z0-9_]/g, "_").slice(0, 32) ?? "";
    }).catch(() => {});
  });

  async function refreshProject() {
    project = await api.projectInfo(site.name);
  }

  const processes = [
    { kind: "queue", title: "Queue Worker", desc: "Runs php artisan queue:work while Corral runs." },
    { kind: "schedule", title: "Scheduler", desc: "Runs php artisan schedule:work while Corral runs." },
    { kind: "reverb", title: "Reverb", desc: "WebSocket server on 127.0.0.1 while Corral runs." },
  ];

  const phpOptions = $derived([
    { value: "", label: `Default (${d.default_php ?? "none"})` },
    ...d.php.map((p) => ({ value: p.version, label: `PHP ${p.version}` })),
    ...(site.isolated && site.php && !d.php.some((p) => p.version === site.php)
      ? [{ value: site.php, label: `PHP ${site.php} (not installed)` }]
      : []),
  ]);
</script>

<Toolbar title={site.host} subtitle="{site.driver_label}{site.origin === 'linked' ? ' · Linked' : ''}">
  {#snippet actions()}
    <Button variant="plain" icon="editor" label="Open in Editor" onclick={() => app.act("editor", () => api.openIn(site.path, "editor"))} />
    <Button variant="plain" icon="terminal" label="Open in Terminal" onclick={() => app.act("terminal", () => api.openIn(site.path, "terminal"))} />
    <Button variant="plain" icon="folder" label="Show in Finder" onclick={() => revealItemInDir(site.path)} />
    <Button variant="primary" icon="external" onclick={() => openUrl(site.url)}>Open</Button>
  {/snippet}
</Toolbar>

<div class="pane">
  <Group>
    <Row title="Address">
      <button class="link selectable" onclick={() => openUrl(site.url)}>{site.url}</button>
    </Row>
    <Row
      title="Folder"
      description={site.driver.docroot === site.path ? site.path : `${site.path} · serves ${site.driver.docroot.slice(site.path.length + 1)}/`}
    />
    <Row title="PHP version">
      <PopUp
        label="PHP version"
        value={site.isolated ? (site.php ?? "") : ""}
        options={phpOptions}
        disabled={!!app.busy}
        onchange={(v) => app.run("php", () => api.setSitePhp(site.name, v || null))}
      />
    </Row>
    <Row title="HTTPS" description={site.secure ? "Uses Corral’s local certificate." : "Served over plain HTTP."}>
      <Switch label="HTTPS" checked={site.secure} disabled={!!app.busy} onchange={(v) => app.run("secure", () => api.setSiteSecure(site.name, v))} />
    </Row>
  </Group>

  {#if project?.laravel}
    <Group title="Laravel">
      <Row title="Watchers" description="Show queries, HTTP calls, logs, views, cache and jobs in Dumps. Adds a dev-only package to composer.json.">
        <Switch
          label="Watchers"
          checked={project.watchers}
          disabled={!!app.busy}
          onchange={(v) => app.act("watch", async () => { await api.setWatchers(site.name, v); await refreshProject(); })}
        />
      </Row>
      <Row title="Tinker" description="Interactive shell for this app, in Terminal.">
        <Button onclick={() => app.act("tinker", () => api.openIn(site.path, "tinker"))}>Open Tinker</Button>
      </Row>
      {#each processes as p (p.kind)}
        {#if p.kind !== "reverb" || project.reverb_available}
          <Row title={p.title} description={p.desc}>
            <Switch
              label={p.title}
              checked={project.processes.includes(p.kind)}
              disabled={!!app.busy}
              onchange={(v) => app.act("proc", async () => { await api.setSiteProcess(site.name, p.kind, v); await refreshProject(); })}
            />
          </Row>
        {/if}
      {/each}
    </Group>

    {#if project.db && ["mysql", "mariadb", "pgsql"].includes(project.db.connection)}
      <Group
        title="Database"
        footer={project.db.needs_dedicated_user ? "The .env file uses root with a password. Corral creates its own user instead and backs up .env first." : undefined}
      >
        <Row
          title={project.db.database ?? "No DB_DATABASE in .env"}
          description="{project.db.connection === 'pgsql' ? 'PostgreSQL' : 'MySQL'}{project.db.username ? ` · user ${project.db.username}` : ''}{project.db.service ? '' : ' · add the service first'}"
        >
          {#if project.db.needs_dedicated_user}
            <TextField label="Database user" bind:value={dbUser} width="140px" />
          {/if}
          <Button
            disabled={!!app.busy || !project.db.service || !project.db.database}
            onclick={() => app.act("db", async () => {
              app.notice = await api.createDatabase(site.name, project?.db?.needs_dedicated_user ? dbUser : undefined);
              await refreshProject();
            })}
          >{app.busy === "db" ? "Creating…" : "Create Database"}</Button>
        </Row>
      </Group>
    {/if}
  {/if}

  <Group title="Sharing" footer={shareUrl ? "Anyone with the link can open this site until you stop sharing." : "A temporary public link through a Cloudflare quick tunnel. No account needed."}>
    {#if shareUrl}
      <Row title="Public link">
        <button class="link selectable" onclick={() => openUrl(shareUrl)}>{shareUrl}</button>
        <Button variant="plain" size="small" icon="copy" label="Copy Link" onclick={() => navigator.clipboard.writeText(shareUrl)} />
        <Button disabled={!!app.busy} onclick={() => app.run("share", () => api.shareStop(site.name))}>Stop</Button>
      </Row>
    {:else}
      <Row title="Share publicly" description={engineUp ? undefined : "Start Corral to share."}>
        <Button disabled={!!app.busy || !engineUp} onclick={() => app.run("share", () => api.shareStart(site.name))}>
          {app.busy === "share" ? "Opening Tunnel…" : "Share"}
        </Button>
      </Row>
    {/if}
  </Group>

  <Group title="Environment">
    <Row title=".env" description="Contains secrets, so it stays closed until you open it.">
      <Button disabled={!!app.busy} onclick={() => app.act("env", async () => { envText = (await api.readEnv(site.name)) ?? ""; })}>Edit…</Button>
    </Row>
  </Group>

  <Group title="Logs">
    {#snippet actions()}
      {#if logs}
        <PopUp
          label="Minimum level"
          bind:value={logLevel}
          options={[{ value: "", label: "All Levels" }, { value: "warning", label: "Warnings and Up" }, { value: "error", label: "Errors and Up" }]}
          onchange={() => app.act("logs", async () => { logs = await api.siteLogs(site.name, logLevel || undefined); })}
        />
      {/if}
      <Button size="small" disabled={!!app.busy} onclick={() => app.act("logs", async () => { logs = await api.siteLogs(site.name, logLevel || undefined); openLog = null; })}>
        {logs ? "Refresh" : "Show Logs"}
      </Button>
    {/snippet}
    {#if logs === null}
      <Row title="storage/logs" description="Newest entries from the site’s Laravel log." />
    {:else if !logs.length}
      <Row title="No entries" description={logLevel ? "Nothing at this level." : "The log is empty."} />
    {:else}
      {#each logs as e, i (i)}
        <button class="log" onclick={() => (openLog = openLog === i ? null : i)} aria-expanded={openLog === i}>
          <span class="level" class:bad={["error", "critical", "alert", "emergency"].includes(e.level)}>{e.level}</span>
          <span class="time">{e.time}</span>
          <span class="msg">{e.message}</span>
        </button>
        {#if openLog === i}
          <pre class="trace">{e.message}{e.details ? "\n\n" + e.details : ""}</pre>
        {/if}
      {/each}
    {/if}
  </Group>

  {#if site.origin === "linked"}
    <div class="danger"><Button variant="destructive" onclick={() => (confirmUnlink = true)}>Unlink Site…</Button></div>
  {/if}
</div>

{#if envText !== null}
  <Sheet title=".env — {site.host}" message="The first save keeps the original as .env.corral-backup." width="640px" onclose={() => (envText = null)}>
    <TextField label=".env contents" bind:value={envText} multiline mono rows={18} />
    {#snippet buttons()}
      <Button onclick={() => (envText = null)}>Cancel</Button>
      <Button variant="primary" disabled={!!app.busy} onclick={() => app.act("saveenv", async () => { await api.writeEnv(site.name, envText ?? ""); envText = null; app.notice = ".env saved."; })}>Save</Button>
    {/snippet}
  </Sheet>
{/if}

{#if confirmUnlink}
  <Sheet title="Unlink {site.host}?" message="Corral stops serving this folder. Your project files are not touched." onclose={() => (confirmUnlink = false)}>
    {#snippet buttons()}
      <Button onclick={() => (confirmUnlink = false)}>Cancel</Button>
      <Button variant="primary" onclick={() => { confirmUnlink = false; app.run("unlink", () => api.unlink(site.name)); }}>Unlink</Button>
    {/snippet}
  </Sheet>
{/if}

<style>
  .pane {
    flex: 1;
    overflow-y: auto;
    padding: 20px max(20px, calc((100% - 720px) / 2));
  }
  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent);
    font: inherit;
  }
  .log {
    display: flex;
    align-items: baseline;
    gap: 10px;
    width: 100%;
    padding: 6px 12px;
    border: 0;
    background: none;
    color: var(--label);
    font: inherit;
    text-align: left;
  }
  .log + .log,
  .trace + .log {
    box-shadow: inset 0 0.5px 0 var(--separator);
  }
  .level {
    width: 64px;
    flex: none;
    font-size: 11px;
    color: var(--label-2);
    text-transform: capitalize;
  }
  .level.bad {
    color: var(--red);
  }
  .time {
    flex: none;
    font-family: var(--mono);
    font-size: 11px;
    color: var(--label-2);
  }
  .msg {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .trace {
    margin: 0;
    max-height: 320px;
    overflow: auto;
    padding: 8px 12px 12px 86px;
    font-family: var(--mono);
    font-size: 11px;
    line-height: 15px;
    white-space: pre-wrap;
    color: var(--label-2);
  }
  .danger {
    display: flex;
    justify-content: flex-end;
  }
</style>
