<!-- macOS pop-up button. A styled native <select> keeps keyboard and VoiceOver behaviour. -->
<script lang="ts">
  let {
    value = $bindable(""),
    options,
    label,
    disabled = false,
    onchange,
  }: {
    value?: string;
    options: { value: string; label: string; disabled?: boolean }[];
    label: string;
    disabled?: boolean;
    onchange?: (value: string) => void;
  } = $props();
</script>

<span class="popup" class:disabled>
  <select bind:value aria-label={label} {disabled} onchange={(e) => onchange?.(e.currentTarget.value)}>
    {#each options as o (o.value)}
      <option value={o.value} disabled={o.disabled}>{o.label}</option>
    {/each}
  </select>
  <svg width="7" height="10" viewBox="0 0 7 10" aria-hidden="true">
    <path d="M1 3.5 3.5 1 6 3.5M1 6.5 3.5 9 6 6.5" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round" />
  </svg>
</span>

<style>
  .popup {
    position: relative;
    display: inline-flex;
    align-items: center;
    height: var(--control-h);
    border-radius: var(--radius-control);
    border: 0.5px solid var(--control-border);
    background: var(--control);
    box-shadow: var(--control-shadow);
  }
  .popup.disabled {
    opacity: 0.45;
  }
  select {
    appearance: none;
    height: 100%;
    padding: 0 26px 0 9px;
    border: 0;
    background: transparent;
    color: var(--label);
    font: inherit;
  }
  svg {
    position: absolute;
    right: 8px;
    pointer-events: none;
    color: var(--label-2);
  }
</style>
