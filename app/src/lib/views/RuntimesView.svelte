<!--
  PHP and Node.js versions: install, switch the default, open per-version
  PHP settings, and remove versions that are no longer needed.
-->
<script lang="ts">
  import Toolbar from "$lib/ui/Toolbar.svelte";
  import Group from "$lib/ui/Group.svelte";
  import Row from "$lib/ui/Row.svelte";
  import Button from "$lib/ui/Button.svelte";
  import Menu from "$lib/ui/Menu.svelte";
  import PopUp from "$lib/ui/PopUp.svelte";
  import Sheet from "$lib/ui/Sheet.svelte";
  import { api } from "$lib/api";
  import { app } from "$lib/app.svelte";

  const d = $derived(app.data!);

  const phpChoices = ["8.5", "8.4", "8.3", "8.2"];
  const nodeChoices = [24, 22];

  const defaultPhp = $derived(d.php.find((p) => p.is_default)?.version ?? "");
  const defaultNode = $derived(String(d.node.find((n) => n.is_default)?.major ?? ""));

  /** Sites served by each PHP version (pinned or through the default). */
  const usage = $derived.by(() => {
    const m: Record<string, { pinned: string[]; all: number }> = {};
    for (const s of d.sites) {
      const v = s.php ?? defaultPhp;
      m[v] ??= { pinned: [], all: 0 };
      m[v].all++;
      if (s.isolated) m[v].pinned.push(s.host);
    }
    return m;
  });

  function phpDescription(version: string, build: string, isDefault: boolean) {
    const u = usage[version];
    const parts = [build === "extended" ? "Extended build" : "Standard build"];
    if (isDefault) parts.push("Default");
    if (u?.all) parts.push(`${u.all} site${u.all === 1 ? "" : "s"}`);
    return parts.join(" · ");
  }

  /** Why a version can't be removed right now, if it can't. */
  function phpBlocked(version: string, isDefault: boolean) {
    if (isDefault) return "Make another version the default to remove this one.";
    const pinned = usage[version]?.pinned ?? [];
    if (pinned.length) return `Used by ${pinned.join(", ")}. Switch those sites first.`;
    return null;
  }

  let removing = $state<{ kind: "php" | "node"; id: string; label: string } | null>(null);

  async function confirmRemove() {
    const r = removing!;
    const ok = await app.run("rm-" + r.id, () => (r.kind === "php" ? api.phpRemove(r.id) : api.nodeRemove(Number(r.id))));
    if (ok) {
      removing = null;
      app.notice = `${r.label} was removed.`;
    }
  }

  function openPhp(version: string) {
    app.phpVersion = version;
    app.view = "php";
  }
</script>

<Toolbar
  title="PHP & Node"
  subtitle={[defaultPhp && `PHP ${defaultPhp}`, defaultNode && `Node.js ${defaultNode}`].filter(Boolean).join(" · ") || "Nothing installed yet"}
/>

<div class="pane">
  <Group
    title="PHP"
    footer="The default serves every site without its own version and runs php and composer in Terminal. Pin a site to a version on its page."
  >
    {#snippet actions()}
      <Menu
        label="Install PHP Version"
        align="right"
        disabled={!!app.busy}
        items={phpChoices.map((v) => ({ label: `PHP ${v}`, disabled: d.php.some((p) => p.version === v), onselect: () => app.run("php-" + v, () => api.install("php", v)) }))}
      />
    {/snippet}
    {#if d.php.length > 1}
      <Row title="Default version" description="Used by every site that isn’t pinned to a version.">
        <PopUp
          label="Default PHP version"
          value={defaultPhp}
          disabled={!!app.busy}
          options={d.php.map((p) => ({ value: p.version, label: `PHP ${p.version}` }))}
          onchange={(v) => app.run("defphp", () => api.setDefaultPhp(v))}
        />
      </Row>
    {/if}
    {#each d.php as p (p.version)}
      {@const blocked = phpBlocked(p.version, p.is_default)}
      <Row title="PHP {p.version}" description={phpDescription(p.version, p.build, p.is_default)}>
        {#if !p.is_default}
          <Button size="small" disabled={!!app.busy} onclick={() => app.run("defphp", () => api.setDefaultPhp(p.version))}>Make Default</Button>
        {/if}
        <Button size="small" onclick={() => openPhp(p.version)}>Settings & Extensions…</Button>
        <span title={blocked ?? undefined}>
          <Button
            variant="plain"
            size="small"
            icon="trash"
            label="Remove PHP {p.version}…"
            disabled={!!app.busy || !!blocked}
            onclick={() => (removing = { kind: "php", id: p.version, label: `PHP ${p.version}` })}
          />
        </span>
      </Row>
    {:else}
      <Row title="No PHP installed" description="Choose a version from the + menu." />
    {/each}
    {#if app.busy?.startsWith("php-")}
      <Row title="Installing PHP {app.busy.slice(4)}…" description="Downloading and verifying. This takes a minute." />
    {/if}
  </Group>

  <Group title="Node.js" footer="The default runs node, npm and npx in Terminal and for site processes such as Vite.">
    {#snippet actions()}
      <Menu
        label="Install Node.js Version"
        align="right"
        disabled={!!app.busy}
        items={[
          { label: "Latest LTS", onselect: () => app.run("node-lts", () => api.nodeInstall()) },
          ...nodeChoices.map((m) => ({ label: `Node.js ${m}`, disabled: d.node.some((n) => n.major === m), onselect: () => app.run("node-" + m, () => api.nodeInstall(m)) })),
        ]}
      />
    {/snippet}
    {#if d.node.length > 1}
      <Row title="Default version">
        <PopUp
          label="Default Node.js version"
          value={defaultNode}
          disabled={!!app.busy}
          options={d.node.map((n) => ({ value: String(n.major), label: `Node.js ${n.major}` }))}
          onchange={(v) => app.run("nodeuse", () => api.nodeUse(Number(v)))}
        />
      </Row>
    {/if}
    {#each d.node as n (n.major)}
      {@const blocked = n.is_default && d.node.length > 1 ? "Make another version the default to remove this one." : null}
      <Row title="Node.js {n.major}" description={[n.version, n.is_default ? "Default" : ""].filter(Boolean).join(" · ")}>
        {#if !n.is_default}
          <Button size="small" disabled={!!app.busy} onclick={() => app.run("nodeuse", () => api.nodeUse(n.major))}>Make Default</Button>
        {/if}
        <span title={blocked ?? undefined}>
          <Button
            variant="plain"
            size="small"
            icon="trash"
            label="Remove Node.js {n.major}…"
            disabled={!!app.busy || !!blocked}
            onclick={() => (removing = { kind: "node", id: String(n.major), label: `Node.js ${n.major}` })}
          />
        </span>
      </Row>
    {:else}
      <Row title="No Node.js installed" description="Choose a version from the + menu." />
    {/each}
    {#if app.busy?.startsWith("node-")}
      <Row title="Installing Node.js…" description="Downloading and verifying." />
    {/if}
  </Group>
</div>

{#if removing}
  <Sheet
    title="Remove {removing.label}?"
    message={removing.kind === "php"
      ? "Deletes this PHP version and its php.ini settings. No site uses it."
      : d.node.length === 1
        ? "Deletes Node.js. node, npm and npx will no longer come from Corral in Terminal."
        : "Deletes this Node.js version and its global packages."}
    onclose={() => (removing = null)}
  >
    {#snippet buttons()}
      <Button onclick={() => (removing = null)}>Cancel</Button>
      <Button variant="destructive" disabled={!!app.busy} onclick={confirmRemove}>{app.busy === "rm-" + removing?.id ? "Removing…" : "Remove"}</Button>
    {/snippet}
  </Sheet>
{/if}

<style>
  .pane {
    flex: 1;
    overflow-y: auto;
    padding: 20px max(20px, calc((100% - 720px) / 2));
  }
</style>
