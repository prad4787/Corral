<script lang="ts">
  import Toolbar from "$lib/ui/Toolbar.svelte";
  import Group from "$lib/ui/Group.svelte";
  import Row from "$lib/ui/Row.svelte";
  import Button from "$lib/ui/Button.svelte";
  import Switch from "$lib/ui/Switch.svelte";
  import Menu from "$lib/ui/Menu.svelte";
  import Sheet from "$lib/ui/Sheet.svelte";
  import StatusDot from "$lib/ui/StatusDot.svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { api } from "$lib/api";
  import { app } from "$lib/app.svelte";
  import { addParkedFolder } from "./actions";

  let { update = $bindable() }: { update: { current: string; latest: string | null; newer: boolean; url: string | null } | null } = $props();
  const d = $derived(app.data!);
  const sys = $derived(d.system);

  let confirmUninstall = $state(false);
  let deleteData = $state(false);

  const phpChoices = ["8.5", "8.4", "8.3", "8.2"];
  const nodeChoices = [24, 22];
</script>

<Toolbar title="Settings" />

<div class="pane">
  <Group title="General">
    <Row title="Open at login" description="Start Corral in the menu bar and serve your sites after you log in.">
      <Switch label="Open at login" checked={d.autostart} disabled={!!app.busy} onchange={(v) => app.run("auto", () => api.setAutostart(v))} />
    </Row>
    <Row title="Use Corral’s tools in Terminal" description="Puts php, composer, node and mysql from Corral first in new Terminal windows.">
      <Switch label="Use Corral’s tools in Terminal" checked={d.path_configured} disabled={!!app.busy} onchange={(v) => app.run("path", () => api.setShellPath(v))} />
    </Row>
  </Group>

  <Group
    title="Domains and HTTPS"
    footer={sys.ports_busy ? "Another app is using ports 80 and 443. Quit it to use addresses without a port number." : `Sites open at https://name.${d.tld}. Setting up asks for your password once.`}
  >
    {#snippet actions()}
      {#if !(sys.dns && sys.https && sys.ports)}
        <Button size="small" variant="primary" disabled={!!app.busy || sys.ports_busy} onclick={() => app.run("system", api.systemSetup)}>{app.busy === "system" ? "Waiting for macOS…" : "Set Up…"}</Button>
      {/if}
      {#if sys.dns || sys.https || sys.ports}
        <Button size="small" disabled={!!app.busy} onclick={() => app.run("system", api.systemUndo)}>Remove</Button>
      {/if}
    {/snippet}
    <Row title=".{d.tld} domains" description="Resolved by Corral’s DNS server.">{#snippet leading()}<StatusDot on={sys.dns} label={sys.dns ? "On" : "Off"} />{/snippet}</Row>
    <Row title="Trusted certificates" description="Corral’s local certificate authority, valid only for .{d.tld}.">{#snippet leading()}<StatusDot on={sys.https} label={sys.https ? "On" : "Off"} />{/snippet}</Row>
    <Row title="Standard ports" description={sys.ports ? "Ports 80 and 443." : `Using ${d.http_port} and ${d.https_port}.`}>{#snippet leading()}<StatusDot on={sys.ports} label={sys.ports ? "On" : "Off"} />{/snippet}</Row>
  </Group>

  <Group title="PHP" footer="Upload size, memory, timeouts, error display and extensions are set per version.">
    {#snippet actions()}
      <Menu
        label="Install PHP Version"
        align="right"
        disabled={!!app.busy}
        items={phpChoices.map((v) => ({ label: `PHP ${v}`, disabled: d.php.some((p) => p.version === v), onselect: () => app.run("php", () => api.install("php", v)) }))}
      />
    {/snippet}
    {#each d.php as p (p.version)}
      <Row title="PHP {p.version}" description="{p.build === 'extended' ? 'Extended build' : 'Standard build'}{p.is_default ? ' · Default' : ''}">
        {#if !p.is_default}
          <Button size="small" disabled={!!app.busy} onclick={() => app.run("defphp", () => api.setDefaultPhp(p.version))}>Make Default</Button>
        {/if}
        <Button size="small" onclick={() => { app.phpVersion = p.version; app.view = "php"; }}>Settings & Extensions…</Button>
      </Row>
    {:else}
      <Row title="No PHP installed" description="Use the + button to install a version." />
    {/each}
  </Group>

  <Group title="Node.js">
    {#snippet actions()}
      <Menu
        label="Install Node.js Version"
        align="right"
        disabled={!!app.busy}
        items={[
          { label: "Latest LTS", onselect: () => app.run("node", () => api.nodeInstall()) },
          ...nodeChoices.map((m) => ({ label: `Node.js ${m}`, disabled: d.node.some((n) => n.major === m), onselect: () => app.run("node", () => api.nodeInstall(m)) })),
        ]}
      />
    {/snippet}
    {#each d.node as n (n.major)}
      <Row title="Node.js {n.version || n.major}" description={n.is_default ? "Default" : undefined}>
        {#if !n.is_default}
          <Button size="small" disabled={!!app.busy} onclick={() => app.run("nodeuse", () => api.nodeUse(n.major))}>Make Default</Button>
        {/if}
      </Row>
    {:else}
      <Row title="No Node.js installed" description="Use the + button to install a version." />
    {/each}
  </Group>

  <Group title="Folders" footer="Every project inside these folders is served automatically.">
    {#snippet actions()}
      <Button variant="plain" size="small" icon="plus" label="Add Folder" onclick={addParkedFolder} />
    {/snippet}
    {#each d.parked as dir (dir)}
      <Row title={dir.split("/").pop() ?? dir} description={dir}>
        <Button size="small" disabled={!!app.busy} onclick={() => app.run("unpark", () => api.unpark(dir))}>Remove</Button>
      </Row>
    {:else}
      <Row title="No folders" />
    {/each}
  </Group>

  <Group title="About">
    <Row title="Corral {update?.current ?? ''}" description={update?.newer ? `Version ${update.latest} is available.` : update?.latest ? "You have the latest version." : "Free and open source (MIT)."}>
      {#if update?.newer && update.url}
        <Button size="small" variant="primary" onclick={() => openUrl(update!.url!)}>Download</Button>
      {:else}
        <Button size="small" disabled={!!app.busy} onclick={() => app.act("upd", async () => { update = await api.checkUpdate(); })}>Check for Updates</Button>
      {/if}
    </Row>
  </Group>

  <div class="danger"><Button variant="destructive" onclick={() => (confirmUninstall = true)}>Uninstall Corral…</Button></div>
</div>

{#if confirmUninstall}
  <Sheet title="Uninstall Corral?" message="Removes Corral’s runtimes, settings, certificates and system changes, then quits. Your projects are not touched." onclose={() => (confirmUninstall = false)}>
    <label class="check"><input type="checkbox" bind:checked={deleteData} /> Also delete databases (MySQL, PostgreSQL, Valkey and MongoDB data)</label>
    {#snippet buttons()}
      <Button onclick={() => (confirmUninstall = false)}>Cancel</Button>
      <Button variant="destructive" disabled={!!app.busy} onclick={() => app.act("uninstall", () => api.uninstall(deleteData))}>{app.busy === "uninstall" ? "Removing…" : "Uninstall"}</Button>
    {/snippet}
  </Sheet>
{/if}

<style>
  .pane {
    flex: 1;
    overflow-y: auto;
    padding: 20px max(20px, calc((100% - 720px) / 2));
  }
  .danger {
    display: flex;
    justify-content: flex-end;
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
</style>
