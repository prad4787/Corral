<!-- macOS sheet: modal panel attached to the top of the window. Esc cancels. -->
<script lang="ts">
  import type { Snippet } from "svelte";
  let {
    title,
    message,
    onclose,
    children,
    buttons,
    width = "420px",
  }: {
    title: string;
    message?: string;
    onclose: () => void;
    children?: Snippet;
    buttons: Snippet;
    width?: string;
  } = $props();
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && onclose()} />

<div class="scrim" role="presentation" onclick={onclose}></div>
<div class="sheet" role="dialog" aria-modal="true" aria-label={title} style:width>
  <h2>{title}</h2>
  {#if message}<p class="msg">{message}</p>{/if}
  {#if children}<div class="body">{@render children()}</div>{/if}
  <div class="buttons">{@render buttons()}</div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 50;
    background: rgb(0 0 0 / 0.12);
  }
  .sheet {
    position: fixed;
    z-index: 51;
    top: var(--toolbar-h);
    left: 50%;
    transform: translateX(-50%);
    max-width: calc(100vw - 40px);
    max-height: calc(100vh - var(--toolbar-h) - 20px);
    overflow: auto;
    padding: 18px 20px 16px;
    border-radius: 12px;
    background: var(--window);
    box-shadow:
      0 0 0 0.5px rgb(0 0 0 / 0.2),
      0 12px 40px rgb(0 0 0 / 0.25);
  }
  h2 {
    font-size: 13px;
    font-weight: 700;
  }
  .msg {
    margin-top: 6px;
    color: var(--label-2);
    line-height: 17px;
  }
  .body {
    margin-top: 12px;
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 16px;
  }
</style>
