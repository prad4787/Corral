<!-- macOS segmented control for switching views within a pane. -->
<script lang="ts">
  let {
    value = $bindable(""),
    options,
    label,
  }: { value?: string; options: { value: string; label: string }[]; label: string } = $props();
</script>

<div class="seg" role="radiogroup" aria-label={label}>
  {#each options as o (o.value)}
    <button
      role="radio"
      aria-checked={value === o.value}
      class:selected={value === o.value}
      onclick={() => (value = o.value)}>{o.label}</button
    >
  {/each}
</div>

<style>
  .seg {
    display: inline-flex;
    padding: 1px;
    gap: 1px;
    border-radius: 7px;
    background: var(--fill-strong);
  }
  button {
    height: 20px;
    padding: 0 12px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--label);
    font: inherit;
    font-size: 12px;
  }
  button.selected {
    background: var(--control);
    box-shadow: 0 0.5px 1.5px rgb(0 0 0 / 0.18);
  }
</style>
