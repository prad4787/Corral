<!-- One row of a Group: title + optional description on the left, control on the right. -->
<script lang="ts">
  import type { Snippet } from "svelte";
  let {
    title,
    description,
    leading,
    children,
    onclick,
  }: {
    title: string;
    description?: string;
    leading?: Snippet;
    children?: Snippet;
    /** Makes the whole row a navigation target. */
    onclick?: () => void;
  } = $props();
</script>

{#if onclick}
  <button class="row nav" {onclick}>
    {#if leading}<span class="leading">{@render leading()}</span>{/if}
    <span class="text">
      <span class="title">{title}</span>
      {#if description}<span class="desc">{description}</span>{/if}
    </span>
    {#if children}<span class="control">{@render children()}</span>{/if}
    <svg class="chev" width="7" height="12" viewBox="0 0 7 12" aria-hidden="true"><path d="m1 1 5 5-5 5" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg>
  </button>
{:else}
  <div class="row">
    {#if leading}<span class="leading">{@render leading()}</span>{/if}
    <span class="text">
      <span class="title">{title}</span>
      {#if description}<span class="desc">{description}</span>{/if}
    </span>
    {#if children}<span class="control">{@render children()}</span>{/if}
  </div>
{/if}

<style>
  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    min-height: 40px;
    padding: 8px 12px;
    border: 0;
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: left;
  }
  .row + :global(.row) {
    box-shadow: inset 0 0.5px 0 var(--separator);
  }
  .nav:active {
    background: var(--fill);
  }
  .leading {
    display: flex;
    color: var(--label-2);
  }
  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .desc {
    font-size: 11px;
    line-height: 14px;
    color: var(--label-2);
  }
  .control {
    display: flex;
    align-items: center;
    gap: 6px;
    flex: none;
    color: var(--label-2);
  }
  .chev {
    flex: none;
    color: var(--label-3);
  }
</style>
