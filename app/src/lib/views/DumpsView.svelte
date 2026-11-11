<script lang="ts">
  import Toolbar from "$lib/ui/Toolbar.svelte";
  import Button from "$lib/ui/Button.svelte";
  import Segmented from "$lib/ui/Segmented.svelte";
  import PopUp from "$lib/ui/PopUp.svelte";
  import Empty from "$lib/ui/Empty.svelte";
  import { api, type Dump, type WatchEvent } from "$lib/api";
  import { app } from "$lib/app.svelte";

  let { dumps, events }: { dumps: Dump[]; events: WatchEvent[] } = $props();
  let pane = $state("dumps");
  let kind = $state("");
  const shown = $derived(kind ? events.filter((e) => e.kind === kind) : events);

  function summary(e: WatchEvent): string {
    const d = e.data;
    switch (e.kind) {
      case "query": return String(d.sql);
      case "http": return `${d.method} ${d.url}`;
      case "log": return String(d.message);
      case "view": return String(d.name);
      case "cache": return String(d.key);
      case "job": return `${d.name}${d.error ? " — " + d.error : ""}`;
    }
    return JSON.stringify(d);
  }
  function detail(e: WatchEvent): string {
    const d = e.data;
    switch (e.kind) {
      case "query": return `${Number(d.time_ms ?? 0).toFixed(2)} ms`;
      case "http": return String(d.status ?? d.error ?? "");
      case "log": return String(d.level);
      case "cache": return String(d.op);
      case "job": return String(d.status);
    }
    return "";
  }
  const short = (f: string | null) => (f ? f.split("/").slice(-2).join("/") : "unknown");
</script>

<Toolbar title="Dumps" subtitle={pane === "dumps" ? "dump() and dd() output while Corral runs" : "Queries, HTTP calls, logs, views, cache and jobs"}>
  {#snippet actions()}
    {#if pane === "dumps"}
      <Button size="small" disabled={!dumps.length} onclick={() => app.act("clear", api.dumpsClear)}>Clear</Button>
    {:else}
      <Button size="small" disabled={!events.length} onclick={() => app.act("clear", api.eventsClear)}>Clear</Button>
    {/if}
  {/snippet}
</Toolbar>

<div class="bar">
  <Segmented label="Show" bind:value={pane} options={[{ value: "dumps", label: "Dumps" }, { value: "activity", label: "Activity" }]} />
  {#if pane === "activity"}
    <PopUp
      label="Kind"
      bind:value={kind}
      options={[
        { value: "", label: "All Kinds" },
        { value: "query", label: "Queries" },
        { value: "http", label: "HTTP Calls" },
        { value: "log", label: "Logs" },
        { value: "view", label: "Views" },
        { value: "cache", label: "Cache" },
        { value: "job", label: "Jobs" },
      ]}
    />
  {/if}
</div>

<div class="pane">
  {#if pane === "dumps"}
    {#each dumps as d (d.id)}
      <article class="dump">
        <header><span class="mono">{short(d.file)}{d.line ? `:${d.line}` : ""}</span><time>{new Date(d.time * 1000).toLocaleTimeString()}</time></header>
        <pre class="selectable">{d.text}</pre>
      </article>
    {:else}
      <Empty icon="activity" title="No dumps" text="Call dump($value) in your code and reload the page. Pages stay clean while Corral runs." />
    {/each}
  {:else if shown.length}
    <table>
      <thead><tr><th class="k">Kind</th><th>Event</th><th class="r">Detail</th><th class="r">Time</th></tr></thead>
      <tbody>
        {#each shown as e (e.id)}
          <tr>
            <td class="k">{e.kind}</td>
            <td class="ev mono selectable">{summary(e)}</td>
            <td class="r">{detail(e)}</td>
            <td class="r t">{new Date(e.time * 1000).toLocaleTimeString()}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {:else}
    <Empty icon="activity" title="No activity" text="Turn on Watchers for a Laravel site (open the site, then Laravel → Watchers)." />
  {/if}
</div>

<style>
  .bar {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 20px;
    border-bottom: 0.5px solid var(--separator);
  }
  .pane {
    flex: 1;
    overflow-y: auto;
    padding: 16px 20px;
  }
  .dump {
    margin-bottom: 10px;
    border-radius: var(--radius-group);
    background: var(--group);
    box-shadow: 0 0 0 0.5px var(--separator);
    overflow: hidden;
  }
  .dump header {
    display: flex;
    justify-content: space-between;
    padding: 6px 12px;
    font-size: 11px;
    color: var(--label-2);
    box-shadow: inset 0 -0.5px 0 var(--separator);
  }
  .dump pre {
    margin: 0;
    max-height: 40vh;
    overflow: auto;
    padding: 10px 12px;
    font-family: var(--mono);
    font-size: 12px;
    line-height: 17px;
    white-space: pre-wrap;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    table-layout: fixed;
  }
  th {
    position: sticky;
    top: -16px;
    padding: 4px 8px;
    background: var(--window);
    font-size: 11px;
    font-weight: 500;
    text-align: left;
    color: var(--label-2);
    box-shadow: inset 0 -0.5px 0 var(--separator);
  }
  td {
    padding: 5px 8px;
    box-shadow: inset 0 -0.5px 0 var(--separator);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .k {
    width: 64px;
    color: var(--label-2);
    text-transform: capitalize;
  }
  .r {
    width: 96px;
    text-align: right;
    color: var(--label-2);
  }
  .t {
    width: 96px;
    font-variant-numeric: tabular-nums;
  }
  .ev {
    font-size: 11.5px;
  }
</style>
