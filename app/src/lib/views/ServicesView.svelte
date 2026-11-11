<!--
  Everything Corral runs: its own core services (live status), databases and
  caches (installed ones toggle, the rest install inline), and the database GUI.
-->
<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import Toolbar from "$lib/ui/Toolbar.svelte";
  import Group from "$lib/ui/Group.svelte";
  import Row from "$lib/ui/Row.svelte";
  import Button from "$lib/ui/Button.svelte";
  import Switch from "$lib/ui/Switch.svelte";
  import StatusDot from "$lib/ui/StatusDot.svelte";
  import { api } from "$lib/api";
  import { app } from "$lib/app.svelte";

  const d = $derived(app.data!);
  const engineUp = $derived(d.running || d.running_elsewhere);

  // What installing involves, so nobody is surprised by a long wait.
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
  const coreUp = $derived(d.core.filter((c) => c.running).length);
</script>

<Toolbar title="Services" subtitle={engineUp ? `${coreUp} of ${d.core.length} core services running` : "Corral is stopped"} />

<div class="pane">
  <Group title="Core" footer="Run whenever Corral runs. Start or stop them from the sidebar.">
    {#each d.core as c, i (c.name + i)}
      <Row title={c.name} description={c.detail}>
        {#snippet leading()}<StatusDot on={c.running} label={c.running ? "Running" : "Stopped"} />{/snippet}
        <span class="state">{c.running ? "Running" : "Stopped"}</span>
      </Row>
    {/each}
  </Group>

  <Group title="Databases and Caches" footer="Listen on 127.0.0.1 only. Data stays in Corral’s folder until you remove it.">
    {#each d.services as s (s.name)}
      <Row title={s.label} description="{s.name} · {s.version} · 127.0.0.1:{s.port}{s.enabled && !s.listening ? (engineUp ? ' · starting…' : ' · starts with Corral') : ''}">
        {#snippet leading()}<StatusDot on={s.listening} label={s.listening ? "Running" : "Not running"} />{/snippet}
        <Switch label="Enable {s.label}" checked={s.enabled} disabled={!!app.busy} onchange={(v) => app.run("svc", () => api.setServiceEnabled(s.name, v))} />
      </Row>
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

  <Group title="Database GUI" footer="Browse and edit MySQL, PostgreSQL and SQLite databases. Only reachable from this Mac.">
    <Row title="Adminer" description={d.adminer ? `adminer.${d.tld}` : "Not installed"}>
      {#snippet leading()}<StatusDot on={d.adminer && engineUp} label={d.adminer ? "Installed" : "Not installed"} />{/snippet}
      {#if d.adminer}
        <Button disabled={!!app.busy} onclick={() => app.run("adminer", api.adminerRemove)}>Remove</Button>
        <Button variant="primary" onclick={() => openUrl(app.url(`adminer.${d.tld}`))}>Open</Button>
      {:else}
        <Button disabled={!!app.busy} onclick={() => app.run("adminer", () => api.install("adminer"))}>{app.busy === "adminer" ? "Installing…" : "Install"}</Button>
      {/if}
    </Row>
  </Group>
</div>

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
</style>
