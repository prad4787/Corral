<script lang="ts">
  import Sheet from "$lib/ui/Sheet.svelte";
  import Button from "$lib/ui/Button.svelte";
  import TextField from "$lib/ui/TextField.svelte";
  import PopUp from "$lib/ui/PopUp.svelte";
  import { api } from "$lib/api";
  import { app } from "$lib/app.svelte";

  let { onclose }: { onclose: () => void } = $props();
  let name = $state("");
  let kit = $state("none");
  const valid = $derived(/^[a-z0-9]+(-[a-z0-9]+)*$/.test(name.trim()));
  const folder = $derived(app.data?.parked[0]?.split("/").pop() ?? "");

  async function create() {
    const n = name.trim();
    if (await app.run("new", () => api.newProject(n, kit))) {
      onclose();
      app.openSite(n);
    }
  }
</script>

<Sheet title="New Laravel Project" message="Creates the project in “{folder}” with Composer and serves it right away." {onclose}>
  <div class="form">
    <span>Name</span>
    <div>
      <TextField label="Project name" bind:value={name} placeholder="my-app" width="220px" />
      <p class="help">{name && !valid ? "Use lowercase letters, numbers and hyphens." : `https://${name.trim() || "my-app"}.${app.data?.tld}`}</p>
    </div>
    <span>Starter kit</span>
    <PopUp
      label="Starter kit"
      bind:value={kit}
      options={[
        { value: "none", label: "None" },
        { value: "livewire", label: "Livewire" },
        { value: "react", label: "React" },
        { value: "vue", label: "Vue" },
      ]}
    />
  </div>
  {#snippet buttons()}
    <Button onclick={onclose}>Cancel</Button>
    <Button variant="primary" disabled={!valid || !!app.busy} onclick={create}>{app.busy === "new" ? "Creating…" : "Create"}</Button>
  {/snippet}
</Sheet>

<style>
  .form {
    display: grid;
    grid-template-columns: 90px 1fr;
    align-items: start;
    gap: 10px 12px;
  }
  span {
    padding-top: 4px;
    text-align: right;
    color: var(--label-2);
  }
  .help {
    margin-top: 4px;
    font-size: 11px;
    color: var(--label-2);
  }
</style>
