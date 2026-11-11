<script lang="ts">
  import "../app.css";
  import { installMock } from "$lib/devmock";
  // Design preview in a plain browser during `pnpm dev`; inert in the app.
  if (import.meta.env.DEV) installMock();
  let { children } = $props();
</script>

{@render children()}
