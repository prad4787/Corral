<!--
  Web server status, databases and caches (install, enable, remove
  completely) and database tools (install, open, remove).
-->
<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import Toolbar from "$lib/ui/Toolbar.svelte";
  import Group from "$lib/ui/Group.svelte";
  import Row from "$lib/ui/Row.svelte";
  import Button from "$lib/ui/Button.svelte";
  import Switch from "$lib/ui/Switch.svelte";
  import StatusDot from "$lib/ui/StatusDot.svelte";
  import Sheet from "$lib/ui/Sheet.svelte";
  import BackupSheet from "./BackupSheet.svelte";
  import ConnectionSheet from "./ConnectionSheet.svelte";
  import Menu from "$lib/ui/Menu.svelte";
  import { api, type Service } from "$lib/api";
  import { app } from "$lib/app.svelte";

  const d = $derived(app.data!);
  const engineUp = $derived(d.running || d.running_elsewhere);
  const web = $derived(d.core.find((c) => c.id === "web"));

  const notes: Record<string, string> = {
    mysql: "About 170 MB download.",
    postgres: "Builds from verified source; takes a few minutes.",
    valkey: "Redis-compatible. Builds from verified source; about a minute.",
    mongodb: "About 100 MB download from MongoDB (SSPL licence).",
  };
  const order = ["mysql", "postgres", "valkey", "mongodb"];
  const notInstalled = $derived(
    d.catalog.filter((c) => !c.installed).sort((a, b) => order.indexOf(a.kind) - order.indexOf(b.kind)),
  );

  // Removal sheet state.
  let removing = $state<{ name: string; label: string } | null>(null);
  let connecting = $state<Service | null>(null);
  let backup = $state<{ service: Service; mode: "backup" | "restore" } | null>(null);
  let deleteData = $state(true);
</script>

<Toolbar title="Services" subtitle={engineUp ? "Everything runs on 127.0.0.1" : "Corral is stopped"} />

<div class="pane">
  {#if web}
    <Group title="Web Server">
      <Row title={web.name} description={web.detail}>
        {#snippet leading()}<StatusDot on={web.running} label={web.running ? "Running" : "Stopped"} />{/snippet}
        <span class="state">{web.running ? "Running" : "Stopped"}</span>
      </Row>
    </Group>
  {/if}

  <Group title="Databases and Caches" footer="Listen on 127.0.0.1 only. Connect… shows the login details and lets you set a password. Back up and restore while a service is running. Removing a service stops it first and deletes its files.">
    {#each d.services as s (s.name)}
      <Row title={s.label} description="{s.name} · {s.version} · 127.0.0.1:{s.port}{s.secured ? ' · Password protected' : ''}{s.enabled && !s.listening ? (engineUp ? ' · starting…' : ' · starts with Corral') : ''}">
        {#snippet leading()}<StatusDot on={s.listening} label={s.listening ? "Running" : "Not running"} />{/snippet}
        <Button size="small" onclick={() => (connecting = s)}>Connect…</Button>
        <Menu
          icon="chevron-down"
          label="More actions for {s.label}"
          align="right"
          disabled={!!app.busy}
          items={[
            { label: "Back Up…", disabled: !s.listening, onselect: () => (backup = { service: s, mode: "backup" }) },
            { label: "Restore…", disabled: !s.listening, onselect: () => (backup = { service: s, mode: "restore" }) },
            { label: `Remove ${s.label}…`, separator: true, onselect: () => { deleteData = true; removing = { name: s.name, label: s.label }; } },
          ]}
        />
        <Switch label="Enable {s.label}" checked={s.enabled} disabled={!!app.busy} onchange={(v) => app.run("svc", () => api.setServiceEnabled(s.name, v))} />
      </Row>
      {#if s.extras_missing}
        <Row
          title="{s.label} extensions"
          description={app.busy === "extras-" + s.name
            ? "Building from verified source. Takes a few minutes; the server keeps running."
            : "dblink, hstore, pg_trgm, uuid-ossp, citext, postgres_fdw and more aren’t installed. Backups that use them can’t be restored."}
        >
          <Button disabled={!!app.busy} onclick={() => app.run("extras-" + s.name, () => api.installServiceExtras(s.name)).then((ok) => { if (ok) app.notice = `${s.label} extensions are installed.`; })}>
            {app.busy === "extras-" + s.name ? "Building…" : "Install Extensions"}
          </Button>
        </Row>
      {/if}
    {/each}
    {#each notInstalled as c (c.kind)}
      <Row title={c.label} description={c.available ? `${c.version} · ${notes[c.kind] ?? ""}` : "Not available for this Mac yet."}>
        {#snippet leading()}<StatusDot on={false} label="Not installed" />{/snippet}
        <Button disabled={!!app.busy || !c.available} onclick={() => app.run(c.kind, () => api.install(c.kind as "mysql"))}>
          {app.busy === c.kind ? (c.kind === "postgres" || c.kind === "valkey" ? "Building…" : "Installing…") : "Install"}
        </Button>
      </Row>
    {/each}
  </Group>

  <Group title="Database Tools" footer="Each tool has its own address and is only reachable from this Mac.">
    {#each d.tools as t (t.id)}
      <Row title={t.label} description={t.installed ? (t.note ?? `${t.host} · ${t.description}`) : (t.note ? `${t.description} ${t.note}` : t.description)}>
        {#snippet leading()}<StatusDot on={t.running} label={t.running ? "Running" : t.installed ? "Stopped" : "Not installed"} />{/snippet}
        {#if t.installed}
          <Button variant="plain" size="small" icon="trash" label="Remove {t.label}" disabled={!!app.busy} onclick={() => app.run("tool-" + t.id, () => api.toolRemove(t.id))} />
          <Button variant="primary" disabled={!engineUp} onclick={() => openUrl(app.url(t.host))}>Open</Button>
        {:else}
          <Button disabled={!!app.busy} onclick={() => app.run("tool-" + t.id, () => api.toolInstall(t.id))}>
            {app.busy === "tool-" + t.id ? "Installing…" : "Install"}
          </Button>
        {/if}
      </Row>
    {/each}
  </Group>
</div>

{#if connecting}
  {#key connecting.name}
    <ConnectionSheet service={d.services.find((x) => x.name === connecting!.name) ?? connecting} onclose={() => (connecting = null)} />
  {/key}
{/if}

{#if backup}
  {#key backup}
    <BackupSheet service={backup.service} mode={backup.mode} onclose={() => (backup = null)} />
  {/key}
{/if}

{#if removing}
  <Sheet
    title="Remove {removing.label}?"
    message="Corral stops it, then deletes its program files, settings and logs."
    onclose={() => (removing = null)}
  >
    <label class="check">
      <input type="checkbox" bind:checked={deleteData} />
      <span>Also delete its data<small>All databases and keys stored in “{removing.name}”. This can’t be undone.</small></span>
    </label>
    {#snippet buttons()}
      <Button onclick={() => (removing = null)}>Cancel</Button>
      <Button
        variant="destructive"
        disabled={!!app.busy}
        onclick={() => { const r = removing!; app.run("rm-" + r.name, () => api.serviceUninstall(r.name, deleteData)).then((ok) => { if (ok) removing = null; }); }}
      >{app.busy === "rm-" + removing?.name ? "Removing…" : "Remove"}</Button>
    {/snippet}
  </Sheet>
{/if}

<style>
  .pane {
    flex: 1;
    overflow-y: auto;
    padding: 20px max(20px, calc((100% - 720px) / 2));
  }
  .state {
    font-size: 12px;
    color: var(--label-2);
  }
  .check {
    display: flex;
    gap: 8px;
    align-items: flex-start;
    line-height: 17px;
  }
  .check input {
    margin: 2px 0 0;
    accent-color: var(--accent);
  }
  .check span {
    display: flex;
    flex-direction: column;
  }
  .check small {
    font-size: 11px;
    color: var(--label-2);
  }
</style>
