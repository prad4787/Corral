<!-- Inset grouped section (System Settings style): optional header and footer text. -->
<script lang="ts">
  import type { Snippet } from "svelte";
  let { title, footer, actions, children }: { title?: string; footer?: string; actions?: Snippet; children: Snippet } = $props();
</script>

<section class="group">
  {#if title || actions}
    <header>
      {#if title}<h2>{title}</h2>{/if}
      {#if actions}<div class="actions">{@render actions()}</div>{/if}
    </header>
  {/if}
  <div class="box">{@render children()}</div>
  {#if footer}<p class="footer">{footer}</p>{/if}
</section>

<style>
  .group {
    margin-bottom: 22px;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    min-height: 20px;
    margin: 0 4px 6px;
  }
  h2 {
    font-size: 13px;
    font-weight: 600;
  }
  .actions {
    display: flex;
    gap: 6px;
  }
  .box {
    border-radius: var(--radius-group);
    background: var(--group);
    box-shadow: 0 0 0 0.5px var(--separator);
    overflow: hidden;
  }
  .footer {
    margin: 6px 4px 0;
    font-size: 11px;
    line-height: 14px;
    color: var(--label-2);
  }
</style>
