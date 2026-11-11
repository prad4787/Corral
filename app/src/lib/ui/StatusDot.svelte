<!-- Running/stopped indicator: accent when on, hollow grey when off. -->
<script lang="ts">
  let { on, label }: { on: boolean; label: string } = $props();
</script>

<span class="dot" class:on role="img" aria-label={label} title={label}></span>

<style>
  .dot {
    display: inline-block;
    width: 7px;
    height: 7px;
    flex: none;
    border-radius: 50%;
    box-shadow: inset 0 0 0 1px var(--label-3);
  }
  .on {
    background: var(--accent);
    box-shadow: none;
  }
</style>
