<script lang="ts">
  import Icon from "$lib/Icon.svelte";
  let { value = $bindable(""), placeholder = "Search" }: { value?: string; placeholder?: string } = $props();
</script>

<label class="search">
  <Icon name="search" size={12} />
  <input bind:value {placeholder} aria-label={placeholder} spellcheck="false" autocomplete="off" />
  {#if value}
    <button aria-label="Clear search" onclick={() => (value = "")}>
      <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><circle cx="6" cy="6" r="6" fill="currentColor" /><path d="m4 4 4 4M8 4l-4 4" stroke="var(--window)" stroke-width="1.3" stroke-linecap="round" /></svg>
    </button>
  {/if}
</label>

<style>
  .search {
    display: flex;
    align-items: center;
    gap: 5px;
    height: 24px;
    padding: 0 6px;
    border-radius: var(--radius-control);
    background: var(--fill);
    color: var(--label-2);
  }
  input {
    flex: 1;
    min-width: 0;
    border: 0;
    background: transparent;
    color: var(--label);
    font: inherit;
    outline: none;
  }
  .search:focus-within {
    outline: 3px solid color-mix(in srgb, var(--accent) 45%, transparent);
  }
  button {
    display: flex;
    padding: 0;
    border: 0;
    background: none;
    color: var(--label-3);
  }
</style>
