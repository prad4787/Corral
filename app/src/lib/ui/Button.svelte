<!--
  macOS push button. Variants: default (bordered), primary (the window's
  default button, accent filled), destructive (red label), plain (borderless,
  for toolbars and inline actions). Title-case labels, per the HIG.
-->
<script lang="ts">
  import type { Snippet } from "svelte";
  import Icon, { type IconName } from "$lib/Icon.svelte";

  let {
    variant = "default",
    size = "regular",
    icon,
    label,
    disabled = false,
    type = "button",
    onclick,
    children,
  }: {
    variant?: "default" | "primary" | "destructive" | "plain";
    size?: "small" | "regular";
    icon?: IconName;
    /** Accessible name for icon-only buttons (also shown as tooltip). */
    label?: string;
    disabled?: boolean;
    type?: "button" | "submit";
    onclick?: (e: MouseEvent) => void;
    children?: Snippet;
  } = $props();
</script>

<button
  class="btn {variant} {size}"
  class:icon-only={!children}
  {type}
  {disabled}
  {onclick}
  aria-label={label}
  title={label}
>
  {#if icon}<Icon name={icon} size={size === "small" ? 12 : 14} />{/if}
  {#if children}<span>{@render children()}</span>{/if}
</button>

<style>
  .btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 5px;
    height: var(--control-h);
    padding: 0 11px;
    border-radius: var(--radius-control);
    border: 0.5px solid var(--control-border);
    background: var(--control);
    box-shadow: var(--control-shadow);
    color: var(--label);
    font: inherit;
    white-space: nowrap;
  }
  .btn:active:not(:disabled) {
    background: var(--fill-strong);
  }
  .btn:disabled {
    opacity: 0.45;
  }
  .small {
    height: 20px;
    padding: 0 8px;
    font-size: 12px;
  }
  .primary {
    background: var(--accent);
    color: var(--accent-text);
    border-color: transparent;
  }
  .primary:active:not(:disabled) {
    background: color-mix(in srgb, var(--accent) 85%, black);
  }
  .destructive {
    color: var(--red);
  }
  .plain {
    background: transparent;
    border-color: transparent;
    box-shadow: none;
    color: var(--label-2);
  }
  .plain:hover:not(:disabled) {
    background: var(--fill);
    color: var(--label);
  }
  .icon-only {
    width: 28px;
    padding: 0;
  }
  .icon-only.small {
    width: 22px;
  }
</style>
