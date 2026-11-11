<!-- Content-area toolbar in the unified title bar: title (+ subtitle) leading, actions trailing. Draggable. -->
<script lang="ts">
  import type { Snippet } from "svelte";
  let { title, subtitle, actions }: { title: string; subtitle?: string; actions?: Snippet } = $props();
</script>

<header class="toolbar" data-tauri-drag-region>
  <div class="titles" data-tauri-drag-region>
    <h1 data-tauri-drag-region>{title}</h1>
    {#if subtitle}<p data-tauri-drag-region>{subtitle}</p>{/if}
  </div>
  {#if actions}<div class="actions">{@render actions()}</div>{/if}
</header>

<style>
  .toolbar {
    flex: none;
    display: flex;
    align-items: center;
    gap: 12px;
    height: var(--toolbar-h);
    padding: 0 16px 0 20px;
    border-bottom: 0.5px solid var(--separator);
  }
  .titles {
    flex: 1;
    min-width: 0;
  }
  h1 {
    font-size: 15px;
    line-height: 19px;
    font-weight: 700;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  p {
    font-size: 11px;
    line-height: 14px;
    color: var(--label-2);
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 4px;
  }
</style>
