<!-- Inline message under the toolbar: errors in red, notices neutral. -->
<script lang="ts">
  import Icon from "$lib/Icon.svelte";
  let { kind = "notice", text, ondismiss }: { kind?: "error" | "notice"; text: string; ondismiss: () => void } = $props();
</script>

<div class="banner {kind}" role={kind === "error" ? "alert" : "status"}>
  <Icon name={kind === "error" ? "alert-triangle" : "info"} size={14} />
  <p class="selectable">{text}</p>
  <button aria-label="Dismiss" onclick={ondismiss}><Icon name="x" size={12} /></button>
</div>

<style>
  .banner {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    margin: 12px 20px 0;
    padding: 8px 10px;
    border-radius: 8px;
    background: var(--fill);
    color: var(--label);
  }
  .error {
    background: color-mix(in srgb, var(--red) 10%, transparent);
  }
  .error :global(svg:first-child) {
    color: var(--red);
  }
  p {
    flex: 1;
    line-height: 17px;
    white-space: pre-wrap;
    word-break: break-word;
  }
  button {
    display: flex;
    padding: 2px;
    border: 0;
    background: none;
    color: var(--label-2);
  }
</style>
