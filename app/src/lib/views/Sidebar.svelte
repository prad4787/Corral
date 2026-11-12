<!-- Source-list sidebar on the window's sidebar material. -->
<script lang="ts">
  import Icon, { type IconName } from "$lib/Icon.svelte";
  import SearchField from "$lib/ui/SearchField.svelte";
  import StatusDot from "$lib/ui/StatusDot.svelte";
  import Button from "$lib/ui/Button.svelte";
  import { api } from "$lib/api";
  import { app, type View } from "$lib/app.svelte";

  const nav: { view: View; label: string; icon: IconName }[] = [
    { view: "sites", label: "All Sites", icon: "globe" },
    { view: "services", label: "Services", icon: "database" },
    { view: "runtimes", label: "PHP & Node", icon: "layers" },
    { view: "mail", label: "Mail", icon: "mail" },
    { view: "dumps", label: "Dumps", icon: "activity" },
  ];

  const sites = $derived(
    (app.data?.sites ?? []).filter((s) => s.host.includes(app.query.toLowerCase().trim())),
  );
  const running = $derived(!!app.data?.running || !!app.data?.running_elsewhere);
</script>

<nav class="sidebar" aria-label="Sidebar">
  <div class="drag" data-tauri-drag-region></div>
  <div class="search"><SearchField bind:value={app.query} placeholder="Search Sites" /></div>

  <div class="scroll">
    <ul>
      {#each nav as n (n.view)}
        <li>
          <button
            class="item"
            class:selected={app.view === n.view || (n.view === "sites" && app.view === "site" && !app.selected) || (n.view === "runtimes" && app.view === "php")}
            onclick={() => (app.view = n.view)}
          >
            <Icon name={n.icon} size={15} />
            <span>{n.label}</span>
          </button>
        </li>
      {/each}
    </ul>

    <h3>Sites</h3>
    <ul>
      {#each sites as s (s.name)}
        <li>
          <button class="item" class:selected={app.view === "site" && app.selected === s.name} onclick={() => app.openSite(s.name)}>
            <Icon name={s.secure ? "lock" : "globe"} size={13} />
            <span>{s.host}</span>
          </button>
        </li>
      {:else}
        <li class="none">{app.query ? "No matches" : "No sites yet"}</li>
      {/each}
    </ul>
  </div>

  <footer>
    <StatusDot on={running} label={app.status} />
    <span class="status">{app.status}</span>
    {#if !app.data?.running_elsewhere}
      <Button
        size="small"
        disabled={!!app.busy || !app.data?.caddy_installed}
        onclick={() => app.run("engine", app.data?.running ? api.stop : api.start)}
      >{app.data?.running ? "Stop" : "Start"}</Button>
    {/if}
    <Button variant="plain" size="small" icon="settings" label="Settings" onclick={() => (app.view = "settings")} />
  </footer>
</nav>

<style>
  .sidebar {
    display: flex;
    flex-direction: column;
    width: 232px;
    flex: none;
    height: 100vh;
    border-right: 0.5px solid var(--separator);
  }
  .drag {
    flex: none;
    height: var(--toolbar-h);
  }
  .search {
    padding: 0 10px 8px;
  }
  .scroll {
    flex: 1;
    overflow-y: auto;
    padding: 0 10px 10px;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  h3 {
    margin: 14px 8px 4px;
    font-size: 11px;
    font-weight: 600;
    color: var(--label-3);
  }
  .item {
    display: flex;
    align-items: center;
    gap: 7px;
    width: 100%;
    height: 28px;
    padding: 0 8px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--label);
    font: inherit;
    text-align: left;
  }
  .item :global(svg) {
    color: var(--accent);
  }
  .item span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .item.selected {
    background: var(--sidebar-selected);
  }
  .none {
    padding: 4px 8px;
    font-size: 12px;
    color: var(--label-3);
  }
  footer {
    display: flex;
    align-items: center;
    gap: 7px;
    flex: none;
    height: 40px;
    padding: 0 10px 0 16px;
    border-top: 0.5px solid var(--separator);
  }
  .status {
    flex: 1;
    font-size: 12px;
    color: var(--label-2);
  }
</style>
