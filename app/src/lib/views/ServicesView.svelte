<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import Toolbar from "$lib/ui/Toolbar.svelte";
  import Group from "$lib/ui/Group.svelte";
  import Row from "$lib/ui/Row.svelte";
  import Button from "$lib/ui/Button.svelte";
  import Switch from "$lib/ui/Switch.svelte";
  import StatusDot from "$lib/ui/StatusDot.svelte";
  import Menu from "$lib/ui/Menu.svelte";
  import Empty from "$lib/ui/Empty.svelte";
  import { api } from "$lib/api";
  import { app } from "$lib/app.svelte";

  const d = $derived(app.data!);
  const has = (k: string) => d.services.some((s) => s.kind === k);
  const catalog = [
    { kind: "mysql", label: "MySQL 8.4", note: "≈170 MB download" },
    { kind: "postgres", label: "PostgreSQL 18", note: "built from source, a few minutes" },
    { kind: "valkey", label: "Valkey (Redis-compatible)", note: "built from source" },
    { kind: "mongodb", label: "MongoDB 8.2", note: "SSPL, downloaded from MongoDB" },
  ] as const;
  const installing = $derived(catalog.find((c) => app.busy === c.kind));
</script>

<Toolbar title="Services" subtitle={installing ? `Installing ${installing.label} (${installing.note})…` : "Run on 127.0.0.1 while Corral runs"}>
  {#snippet actions()}
    <Menu
      label="Add Service"
      align="right"
      disabled={!!app.busy}
      items={catalog.map((c) => ({
        label: `${c.label}`,
        disabled: has(c.kind),
        onselect: () => app.run(c.kind, () => api.install(c.kind)),
      }))}
    />
  {/snippet}
</Toolbar>

<div class="pane">
  {#if !d.services.length}
    <Empty icon="database" title="No services" text="Add MySQL, PostgreSQL, Valkey or MongoDB. Data stays in Corral’s folder.">
      <Button variant="primary" disabled={!!app.busy} onclick={() => app.run("mysql", () => api.install("mysql"))}>
        {app.busy === "mysql" ? "Installing…" : "Add MySQL"}
      </Button>
    </Empty>
  {:else}
    <Group footer={d.running || d.running_elsewhere ? undefined : "Services start together with Corral."}>
      {#each d.services as s (s.name)}
        <Row title={s.name} description="{s.label} {s.version} · 127.0.0.1:{s.port}{s.listening ? '' : s.enabled ? ' · not running' : ''}">
          {#snippet leading()}<StatusDot on={s.listening} label={s.listening ? "Running" : "Not running"} />{/snippet}
          <Switch label="Enable {s.name}" checked={s.enabled} disabled={!!app.busy} onchange={(v) => app.run("svc", () => api.setServiceEnabled(s.name, v))} />
        </Row>
      {/each}
    </Group>
  {/if}

  <Group title="Database GUI" footer="Adminer for MySQL, PostgreSQL and SQLite. Only reachable from this Mac.">
    <Row title="Adminer" description={d.adminer ? `adminer.${d.tld}` : "Not installed"}>
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
</style>
