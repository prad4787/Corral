<!-- All sites. Rows navigate to the site. "+" adds sites. -->
<script lang="ts">
  import Toolbar from "$lib/ui/Toolbar.svelte";
  import Group from "$lib/ui/Group.svelte";
  import Row from "$lib/ui/Row.svelte";
  import Empty from "$lib/ui/Empty.svelte";
  import Button from "$lib/ui/Button.svelte";
  import Menu from "$lib/ui/Menu.svelte";
  import Icon from "$lib/Icon.svelte";
  import NewProjectSheet from "./NewProjectSheet.svelte";
  import { app } from "$lib/app.svelte";
  import { addParkedFolder, linkProject } from "./actions";

  let creating = $state(false);
  const d = $derived(app.data!);
  const sites = $derived(d.sites.filter((s) => s.host.includes(app.query.toLowerCase().trim())));
</script>

<Toolbar title="Sites" subtitle="{d.sites.length} {d.sites.length === 1 ? 'site' : 'sites'} on .{d.tld}">
  {#snippet actions()}
    <Menu
      label="Add Site"
      align="right"
      items={[
        { label: "New Laravel Project…", onselect: () => (creating = true), disabled: !d.parked.length },
        { label: "Add Folder of Projects…", onselect: addParkedFolder },
        { label: "Link Single Project…", onselect: linkProject },
      ]}
    />
  {/snippet}
</Toolbar>

<div class="pane">
  {#if !d.sites.length}
    <Empty icon="folder" title="No sites" text="Choose a folder of projects, or link a single project folder.">
      <Button variant="primary" onclick={addParkedFolder}>Add Folder…</Button>
    </Empty>
  {:else if !sites.length}
    <Empty icon="search" title="No results" text="No site matches “{app.query}”." />
  {:else}
    <Group>
      {#each sites as s (s.name)}
        <Row
          title={s.host}
          description="{s.driver_label} · PHP {s.php ?? '–'}{s.origin === 'linked' ? ' · Linked' : ''}"
          onclick={() => app.openSite(s.name)}
        >
          {#snippet leading()}<Icon name={s.secure ? "lock" : "globe"} size={15} />{/snippet}
        </Row>
      {/each}
    </Group>
    {#if d.parked.length}
      <p class="hint">Projects in {d.parked.map((p) => p.split("/").pop()).join(", ")} appear here automatically.</p>
    {/if}
  {/if}
</div>

{#if creating}<NewProjectSheet onclose={() => (creating = false)} />{/if}

<style>
  .pane {
    flex: 1;
    overflow-y: auto;
    padding: 20px max(20px, calc((100% - 720px) / 2));
  }
  .hint {
    margin: -12px 4px 0;
    font-size: 11px;
    color: var(--label-2);
  }
</style>
