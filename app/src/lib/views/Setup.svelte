<!-- First-run checklist. Each step is one row with its own action. -->
<script lang="ts">
  import Icon from "$lib/Icon.svelte";
  import Button from "$lib/ui/Button.svelte";
  import Group from "$lib/ui/Group.svelte";
  import Row from "$lib/ui/Row.svelte";
  import Banner from "$lib/ui/Banner.svelte";
  import { api } from "$lib/api";
  import { app } from "$lib/app.svelte";
  import { addParkedFolder } from "./actions";

  const d = $derived(app.data!);
  const sys = $derived(d.system.dns && d.system.https && d.system.ports);
  const ready = $derived(d.caddy_installed && d.php.length > 0);
</script>

{#snippet mark(done: boolean)}
  {#if done}
    <span class="mark done"><Icon name="check" size={11} /></span>
  {:else}
    <span class="mark"></span>
  {/if}
{/snippet}

<div class="setup">
  <div class="drag" data-tauri-drag-region></div>
  <div class="content">
    <h1>Welcome to Corral</h1>
    <p class="lead">A few steps get your PHP sites running on .{d.tld}. You can change or undo all of this later in Settings.</p>

    {#if app.error}<Banner kind="error" text={app.error} ondismiss={() => (app.error = null)} />{/if}

    <Group>
      <Row title="Web server and Composer" description="Caddy and Composer from their official releases, checksum-verified.">
        {#snippet leading()}{@render mark(d.caddy_installed)}{/snippet}
        {#if !d.caddy_installed}
          <Button variant="primary" disabled={!!app.busy} onclick={() => app.run("web", () => api.install("web"))}>
            {app.busy === "web" ? "Installing…" : "Install"}
          </Button>
        {/if}
      </Row>
      <Row title="PHP 8.4" description={d.php.length ? `Installed: PHP ${d.php.map((p) => p.version).join(", ")}` : "Static build from static-php.dev. Upstream publishes no checksums yet."}>
        {#snippet leading()}{@render mark(d.php.length > 0)}{/snippet}
        {#if !d.php.length}
          <Button variant={d.caddy_installed ? "primary" : "default"} disabled={!!app.busy} onclick={() => app.run("php", () => api.install("php", "8.4"))}>
            {app.busy === "php" ? "Installing…" : "Install"}
          </Button>
        {/if}
      </Row>
      <Row title="Your projects" description={d.parked.length ? `${d.sites.length} sites found` : `Every project in the folder you choose becomes name.${d.tld}.`}>
        {#snippet leading()}{@render mark(d.parked.length + d.sites.length > 0)}{/snippet}
        {#if !d.parked.length}
          <Button variant={ready ? "primary" : "default"} disabled={!!app.busy} onclick={addParkedFolder}>Choose Folder…</Button>
        {/if}
      </Row>
      <Row title="Domains and HTTPS" description={d.system.ports_busy ? "Another app is using ports 80 and 443. Quit it, then try again." : `Opens https://site.${d.tld} with a trusted certificate. macOS asks for your password once.`}>
        {#snippet leading()}{@render mark(sys)}{/snippet}
        {#if !sys}
          <Button variant={ready ? "primary" : "default"} disabled={!!app.busy || !d.caddy_installed || d.system.ports_busy} onclick={() => app.run("system", api.systemSetup)}>
            {app.busy === "system" ? "Waiting for macOS…" : "Set Up…"}
          </Button>
        {/if}
      </Row>
    </Group>

    <Group footer="Puts php, composer, node and mysql from Corral first in new Terminal windows.">
      <Row title="Use Corral’s tools in Terminal">
        {#snippet leading()}{@render mark(d.path_configured)}{/snippet}
        {#if !d.path_configured}
          <Button disabled={!!app.busy} onclick={() => app.run("path", () => api.setShellPath(true))}>Add to PATH</Button>
        {/if}
      </Row>
    </Group>

    <div class="footer">
      <Button variant={ready && sys ? "primary" : "default"} onclick={() => (app.skipSetup = true)}>
        {ready ? "Continue" : "Skip for Now"}
      </Button>
    </div>
  </div>
</div>

<style>
  .setup {
    height: 100vh;
    overflow-y: auto;
    background: var(--window);
  }
  .drag {
    height: var(--toolbar-h);
  }
  .content {
    max-width: 560px;
    margin: 0 auto;
    padding: 8px 24px 40px;
  }
  h1 {
    font-size: 22px;
    line-height: 28px;
    font-weight: 700;
  }
  .lead {
    margin: 6px 0 22px;
    color: var(--label-2);
    line-height: 18px;
  }
  .content :global(.banner) {
    margin: 0 0 16px;
  }
  .mark {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    box-shadow: inset 0 0 0 1px var(--label-3);
  }
  .mark.done {
    background: var(--accent);
    box-shadow: none;
    color: var(--accent-text);
  }
  .footer {
    display: flex;
    justify-content: flex-end;
  }
</style>
