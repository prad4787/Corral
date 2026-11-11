<!-- Pull-down menu attached to a button (e.g. the "+" add button). Esc or click outside closes it. -->
<script lang="ts">
  import Icon, { type IconName } from "$lib/Icon.svelte";
  let {
    icon = "plus",
    label,
    items,
    disabled = false,
    align = "left",
  }: {
    icon?: IconName;
    label: string;
    items: { label: string; onselect: () => void; disabled?: boolean; separator?: boolean }[];
    disabled?: boolean;
    align?: "left" | "right";
  } = $props();
  let open = $state(false);
  let root: HTMLElement;
</script>

<svelte:window
  onkeydown={(e) => open && e.key === "Escape" && (open = false)}
  onmousedown={(e) => open && !root?.contains(e.target as Node) && (open = false)}
/>

<div class="menu-root" bind:this={root}>
  <button class="trigger" class:open aria-label={label} title={label} aria-haspopup="menu" aria-expanded={open} {disabled} onclick={() => (open = !open)}>
    <Icon name={icon} size={14} />
  </button>
  {#if open}
    <div class="menu {align}" role="menu">
      {#each items as it, i (i)}
        {#if it.separator}<div class="sep" role="separator"></div>{/if}
        <button role="menuitem" disabled={it.disabled} onclick={() => { open = false; it.onselect(); }}>{it.label}</button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .menu-root {
    position: relative;
  }
  .trigger {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: var(--control-h);
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--label-2);
  }
  .trigger:hover:not(:disabled),
  .trigger.open {
    background: var(--fill);
    color: var(--label);
  }
  .trigger:disabled {
    opacity: 0.45;
  }
  .menu {
    position: absolute;
    z-index: 40;
    top: calc(100% + 4px);
    min-width: 200px;
    padding: 5px;
    border-radius: 8px;
    background: var(--window);
    box-shadow:
      0 0 0 0.5px rgb(0 0 0 / 0.18),
      0 8px 24px rgb(0 0 0 / 0.2);
  }
  .menu.left {
    left: 0;
  }
  .menu.right {
    right: 0;
  }
  [role="menuitem"] {
    display: block;
    width: 100%;
    height: 22px;
    padding: 0 10px;
    border: 0;
    border-radius: 4px;
    background: none;
    color: var(--label);
    font: inherit;
    text-align: left;
  }
  [role="menuitem"]:hover:not(:disabled) {
    background: var(--accent);
    color: var(--accent-text);
  }
  [role="menuitem"]:disabled {
    color: var(--label-3);
  }
  .sep {
    height: 0.5px;
    margin: 5px 6px;
    background: var(--separator);
  }
</style>
