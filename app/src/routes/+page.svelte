<script lang="ts">
  import { onMount } from "svelte";
  import { api, type MailSummary, type Dump, type WatchEvent } from "$lib/api";
  import { app } from "$lib/app.svelte";
  import Banner from "$lib/ui/Banner.svelte";
  import Sidebar from "$lib/views/Sidebar.svelte";
  import Setup from "$lib/views/Setup.svelte";
  import SitesView from "$lib/views/SitesView.svelte";
  import SiteView from "$lib/views/SiteView.svelte";
  import ServicesView from "$lib/views/ServicesView.svelte";
  import MailView from "$lib/views/MailView.svelte";
  import DumpsView from "$lib/views/DumpsView.svelte";
  import SettingsView from "$lib/views/SettingsView.svelte";
  import PhpView from "$lib/views/PhpView.svelte";
  import RuntimesView from "$lib/views/RuntimesView.svelte";

  let mails = $state<MailSummary[]>([]);
  let dumps = $state<Dump[]>([]);
  let events = $state<WatchEvent[]>([]);
  let update = $state<{ current: string; latest: string | null; newer: boolean; url: string | null } | null>(null);

  async function refreshLists() {
    try {
      if (app.view === "mail") mails = await api.mailList();
      if (app.view === "dumps") [dumps, events] = await Promise.all([api.dumpsList(), api.eventsList()]);
    } catch {
      /* transient while polling */
    }
  }

  // Load lists as soon as their view opens, not only on the next poll.
  $effect(() => {
    app.view;
    refreshLists();
  });

  // Notices are informational; let them fade after a few seconds.
  $effect(() => {
    if (!app.notice) return;
    const t = setTimeout(() => (app.notice = null), 6000);
    return () => clearTimeout(t);
  });

  onMount(() => {
    app.run("load", api.overview).then(() => {
      // Design preview only: jump to a screen with ?view=… or ?site=…
      if (!import.meta.env.DEV) return;
      const q = new URLSearchParams(location.search);
      if (q.get("site")) app.openSite(q.get("site")!);
      else if (q.get("php")) { app.phpVersion = q.get("php"); app.view = "php"; }
      else if (q.get("view")) app.view = q.get("view") as typeof app.view;
    });
    api.checkUpdate().then((u) => (update = u)).catch(() => {});
    let unlisten: (() => void) | undefined;
    import("@tauri-apps/api/event").then(({ listen }) =>
      listen<string>("corral://site", (e) => app.openSite(e.payload)).then((u) => (unlisten = u)),
    );
    const t = setInterval(() => {
      app.refresh();
      refreshLists();
    }, 3000);
    return () => {
      clearInterval(t);
      unlisten?.();
    };
  });
</script>

{#if !app.data}
  <div class="loading" data-tauri-drag-region></div>
{:else if app.needsSetup}
  <Setup />
{:else}
  <div class="window">
    <Sidebar />
    <main class="content">
      {#if app.view === "site" && app.site}
        <SiteView />
      {:else if app.view === "services"}
        <ServicesView />
      {:else if app.view === "mail"}
        <MailView {mails} />
      {:else if app.view === "dumps"}
        <DumpsView {dumps} {events} />
      {:else if app.view === "php" && app.phpVersion && app.data?.php.some((p) => p.version === app.phpVersion)}
        <PhpView />
      {:else if app.view === "runtimes" || app.view === "php"}
        <RuntimesView />
      {:else if app.view === "settings"}
        <SettingsView bind:update />
      {:else}
        <SitesView />
      {/if}
      {#if app.error || app.notice}
        <div class="banners">
          {#if app.error}<Banner kind="error" text={app.error} ondismiss={() => (app.error = null)} />{/if}
          {#if app.notice}<Banner text={app.notice} ondismiss={() => (app.notice = null)} />{/if}
        </div>
      {/if}
      {#if update?.newer && update.url && app.view !== "settings"}
        <button class="update" onclick={() => (app.view = "settings")}>Corral {update.latest} is available</button>
      {/if}
    </main>
  </div>
{/if}

<style>
  .loading {
    height: 100vh;
    background: var(--window);
  }
  .window {
    display: flex;
    height: 100vh;
  }
  .content {
    position: relative;
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    background: var(--window);
  }
  .banners {
    position: absolute;
    top: var(--toolbar-h);
    left: 0;
    right: 0;
    z-index: 30;
    display: flex;
    flex-direction: column;
    gap: 6px;
    pointer-events: none;
  }
  .banners :global(.banner) {
    pointer-events: auto;
    background: var(--window);
    box-shadow:
      0 0 0 0.5px var(--separator),
      0 4px 14px rgb(0 0 0 / 0.08);
  }
  .update {
    position: absolute;
    right: 16px;
    bottom: 14px;
    height: 24px;
    padding: 0 10px;
    border: 0.5px solid var(--control-border);
    border-radius: 12px;
    background: var(--window);
    color: var(--label-2);
    font: inherit;
    font-size: 12px;
    box-shadow: 0 2px 8px rgb(0 0 0 / 0.08);
  }
</style>
