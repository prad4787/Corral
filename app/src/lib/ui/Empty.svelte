<!-- Empty state: muted symbol, one short line, optional action. -->
<script lang="ts">
  import type { Snippet } from "svelte";
  import Icon, { type IconName } from "$lib/Icon.svelte";
  let { icon, title, text, children }: { icon: IconName; title: string; text?: string; children?: Snippet } = $props();
</script>

<div class="empty">
  <Icon name={icon} size={28} />
  <p class="title">{title}</p>
  {#if text}<p class="text">{text}</p>{/if}
  {#if children}<div class="action">{@render children()}</div>{/if}
</div>

<style>
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 48px 24px;
    text-align: center;
    color: var(--label-3);
  }
  .title {
    margin-top: 4px;
    font-size: 13px;
    font-weight: 600;
    color: var(--label-2);
  }
  .text {
    max-width: 320px;
    font-size: 12px;
    line-height: 16px;
    color: var(--label-2);
  }
  .action {
    margin-top: 8px;
  }
</style>
