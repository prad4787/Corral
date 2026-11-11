<!-- macOS switch (NSSwitch). Use with a label + description in a Row. -->
<script lang="ts">
  let {
    checked = false,
    disabled = false,
    label,
    onchange,
  }: {
    checked?: boolean;
    disabled?: boolean;
    label: string;
    onchange?: (value: boolean) => void;
  } = $props();
</script>

<button
  class="switch"
  class:on={checked}
  role="switch"
  aria-checked={checked}
  aria-label={label}
  {disabled}
  onclick={() => onchange?.(!checked)}
>
  <span class="knob"></span>
</button>

<style>
  .switch {
    position: relative;
    width: 32px;
    height: 18px;
    flex: none;
    padding: 0;
    border: 0;
    border-radius: 9px;
    background: var(--fill-strong);
    box-shadow: inset 0 0 0 0.5px var(--control-border);
    transition: background 0.15s ease;
  }
  .switch.on {
    background: var(--accent);
    box-shadow: none;
  }
  .switch:disabled {
    opacity: 0.45;
  }
  .knob {
    position: absolute;
    top: 1px;
    left: 1px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: #fff;
    box-shadow: 0 0.5px 2px rgb(0 0 0 / 0.3);
    transition: transform 0.15s ease;
  }
  .on .knob {
    transform: translateX(14px);
  }
  @media (prefers-reduced-motion: reduce) {
    .switch,
    .knob {
      transition: none;
    }
  }
</style>
