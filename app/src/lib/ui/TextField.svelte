<!-- macOS text field (single line) or text view (multiline). -->
<script lang="ts">
  let {
    value = $bindable(""),
    label,
    placeholder = "",
    multiline = false,
    mono = false,
    rows = 12,
    width,
  }: {
    value?: string;
    label: string;
    placeholder?: string;
    multiline?: boolean;
    mono?: boolean;
    rows?: number;
    width?: string;
  } = $props();
</script>

{#if multiline}
  <textarea class="field area" class:mono bind:value aria-label={label} {placeholder} {rows} spellcheck="false"></textarea>
{:else}
  <input class="field" class:mono bind:value aria-label={label} {placeholder} style:width spellcheck="false" autocomplete="off" />
{/if}

<style>
  .field {
    height: var(--control-h);
    padding: 0 7px;
    border-radius: var(--radius-control);
    border: 0.5px solid var(--control-border);
    background: var(--field);
    color: var(--label);
    font: inherit;
  }
  .field:focus {
    outline: 3px solid color-mix(in srgb, var(--accent) 45%, transparent);
    outline-offset: 0;
  }
  .area {
    display: block;
    width: 100%;
    height: auto;
    padding: 6px 8px;
    line-height: 18px;
    resize: vertical;
  }
  .mono {
    font-family: var(--mono);
    font-size: 12px;
  }
</style>
