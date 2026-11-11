<script lang="ts">
  import Toolbar from "$lib/ui/Toolbar.svelte";
  import Group from "$lib/ui/Group.svelte";
  import Row from "$lib/ui/Row.svelte";
  import Button from "$lib/ui/Button.svelte";
  import Switch from "$lib/ui/Switch.svelte";
  import PopUp from "$lib/ui/PopUp.svelte";
  import TextField from "$lib/ui/TextField.svelte";
  import SearchField from "$lib/ui/SearchField.svelte";
  import Sheet from "$lib/ui/Sheet.svelte";
  import { api, type PhpDetails, type PhpBuild } from "$lib/api";
  import { app } from "$lib/app.svelte";

  const version = $derived(app.phpVersion!);
  const info = $derived(app.data?.php.find((p) => p.version === version));

  let details = $state<PhpDetails | null>(null);
  /** Edited values, keyed by ini directive. */
  let draft = $state<Record<string, string>>({});
  let extQuery = $state("");
  let switchTo = $state<PhpBuild | null>(null);
  let iniText = $state<string | null>(null);

  type Kind = "size" | "int" | "bool" | "choice" | "text";
  type Field = { key: string; title: string; description: string; kind: Kind; fallback: string; choices?: { value: string; label: string }[] };

  const SIZE_HINT = "Use K, M or G, e.g. 512M. -1 means no limit.";
  const limits: Field[] = [
    { key: "upload_max_filesize", title: "Maximum upload size", description: "Largest single file a form can upload.", kind: "size", fallback: "2M" },
    { key: "post_max_size", title: "Maximum POST size", description: "Whole request body, including every uploaded file. Keep it at least as large as the upload size.", kind: "size", fallback: "8M" },
    { key: "max_file_uploads", title: "Files per request", description: "How many files one request may upload.", kind: "int", fallback: "20" },
    { key: "memory_limit", title: "Memory limit", description: "Per request or command. " + SIZE_HINT, kind: "size", fallback: "128M" },
  ];
  const timeouts: Field[] = [
    { key: "max_execution_time", title: "Maximum execution time", description: "Seconds a web request may run. 0 means no limit. Commands in Terminal have no limit.", kind: "int", fallback: "30" },
    { key: "max_input_time", title: "Maximum input time", description: "Seconds allowed to receive the request (slow uploads). -1 uses the execution time.", kind: "int", fallback: "-1" },
    { key: "default_socket_timeout", title: "Socket timeout", description: "Seconds to wait for HTTP and other stream connections.", kind: "int", fallback: "60" },
  ];
  const input: Field[] = [
    { key: "max_input_vars", title: "Maximum input variables", description: "Form fields per request. Raise it for large admin forms.", kind: "int", fallback: "1000" },
    { key: "max_input_nesting_level", title: "Maximum input nesting", description: "How deeply arrays in forms may nest.", kind: "int", fallback: "64" },
    { key: "session.gc_maxlifetime", title: "Session lifetime", description: "Seconds before an idle session may be cleaned up.", kind: "int", fallback: "1440" },
  ];
  const levels = [
    { value: "E_ALL", label: "Everything" },
    { value: "E_ALL & ~E_DEPRECATED", label: "Everything except deprecations" },
    { value: "E_ALL & ~E_DEPRECATED & ~E_NOTICE", label: "Warnings and errors" },
    { value: "E_ERROR | E_PARSE", label: "Fatal errors only" },
  ];
  const errors: Field[] = [
    { key: "display_errors", title: "Show errors in pages", description: "Print errors in the response. Frameworks may override this.", kind: "bool", fallback: "Off" },
    { key: "error_reporting", title: "Report", description: "Which problems PHP reports.", kind: "choice", fallback: "E_ALL", choices: levels },
    { key: "log_errors", title: "Write errors to the log", description: "Logs are under Corral’s logs folder.", kind: "bool", fallback: "On" },
  ];
  const opcache: Field[] = [
    { key: "opcache.enable", title: "OPcache", description: "Caches compiled scripts for speed.", kind: "bool", fallback: "On" },
    { key: "opcache.validate_timestamps", title: "Check files for changes", description: "Keep this on while developing so edits show up immediately.", kind: "bool", fallback: "On" },
  ];
  let zones: { value: string; label: string }[] = [];
  try {
    zones = (Intl as any).supportedValuesOf("timeZone").map((z: string) => ({ value: z, label: z.replaceAll("_", " ") }));
  } catch {}
  if (!zones.some((z) => z.value === "UTC")) zones.unshift({ value: "UTC", label: "UTC" });
  const locale: Field[] = [
    { key: "date.timezone", title: "Time zone", description: "Default for date functions.", kind: "choice", fallback: "UTC", choices: zones },
  ];
  const all = [...limits, ...timeouts, ...input, ...errors, ...opcache, ...locale];

  // Extensions only the Extended build compiles in (verified against it).
  const EXTENDED_ONLY = ["mysqli", "intl", "imagick", "apcu", "swoole", "ldap", "imap", "sodium", "event", "protobuf"];

  async function load(v: string) {
    const d = await api.phpDetails(v);
    if (v !== app.phpVersion) return;
    details = d;
    draft = Object.fromEntries(all.map((f) => [f.key, current(f, d)]));
  }

  $effect(() => {
    const v = version;
    details = null;
    load(v).catch((e) => (app.error = String(e)));
  });

  function current(f: Field, d: PhpDetails) {
    const v = d.settings[f.key] ?? f.fallback;
    return f.kind === "bool" ? (truthy(v) ? "On" : "Off") : v;
  }
  const truthy = (v: string) => ["1", "on", "yes", "true"].includes(v.trim().toLowerCase());

  function valid(f: Field, v: string) {
    if (f.kind === "size") return /^-1$|^\d+\s*[KMG]?$/i.test(v.trim());
    if (f.kind === "int") return /^-?\d+$/.test(v.trim());
    return true;
  }
  const bytes = (v: string) => {
    const m = /^(-?\d+)\s*([KMG]?)$/i.exec(v.trim());
    if (!m) return NaN;
    const n = Number(m[1]);
    return n < 0 ? Infinity : n * ({ "": 1, K: 1024, M: 1024 ** 2, G: 1024 ** 3 } as Record<string, number>)[m[2].toUpperCase()];
  };

  const changed = $derived(details ? all.filter((f) => draft[f.key] !== current(f, details!)) : []);
  const invalid = $derived(all.filter((f) => draft[f.key] !== undefined && !valid(f, draft[f.key])));
  const postTooSmall = $derived(bytes(draft.post_max_size ?? "") < bytes(draft.upload_max_filesize ?? ""));
  const overridden = $derived(details ? all.filter((f) => f.key in details!.overrides) : []);

  async function apply() {
    const changes = Object.fromEntries(changed.map((f) => [f.key, f.key === "date.timezone" ? `"${draft[f.key]}"` : draft[f.key].trim()]));
    await app.act("phpset", async () => {
      await api.setPhpSettings(version, changes);
      await load(version);
      app.notice = `PHP ${version} restarted with the new settings.`;
    });
  }

  async function restoreDefaults() {
    await app.act("phpset", async () => {
      await api.setPhpSettings(version, Object.fromEntries(overridden.map((f) => [f.key, null])));
      await load(version);
      app.notice = `PHP ${version} is back on Corral’s defaults.`;
    });
  }

  async function switchBuild(b: PhpBuild) {
    switchTo = null;
    const ok = await app.run("phpbuild", () => api.install("php", version, b));
    if (ok) {
      await load(version).catch(() => {});
      app.notice = `PHP ${version} now uses the ${b === "extended" ? "Extended" : "Standard"} build.`;
    }
  }

  const q = $derived(extQuery.trim().toLowerCase());
  const shown = $derived(details ? details.modules.filter((m) => m.includes(q)) : []);
  const missingHint = $derived.by(() => {
    if (!details || !q || shown.length) return null;
    if (details.build === "standard" && EXTENDED_ONLY.some((e) => e.includes(q)))
      return `“${q}” is in the Extended build. Switch builds below to add it.`;
    return `“${q}” isn’t available in Corral’s PHP builds.`;
  });
</script>

<Toolbar title="PHP {version}" subtitle={details ? `${details.build === "extended" ? "Extended" : "Standard"} build${info?.is_default ? " · Default" : ""}` : undefined}>
  {#snippet actions()}
    <Button disabled={!changed.length || !!app.busy} onclick={() => details && load(version)}>Revert</Button>
    <Button variant="primary" disabled={!changed.length || invalid.length > 0 || !!app.busy} onclick={apply}>{app.busy === "phpset" ? "Applying…" : "Apply"}</Button>
  {/snippet}
</Toolbar>

<div class="pane">
  <button class="back" onclick={() => (app.view = "settings")}>
    <svg width="7" height="12" viewBox="0 0 7 12" aria-hidden="true"><path d="M6 1 1 6l5 5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" /></svg>
    Settings
  </button>

  {#if !details}
    <p class="loading">Loading…</p>
  {:else}
    {#snippet fields(list: Field[])}
      {#each list as f (f.key)}
        <Row title={f.title} description={f.description}>
          {#if f.kind === "bool"}
            <Switch label={f.title} checked={draft[f.key] === "On"} disabled={!!app.busy} onchange={(v) => (draft[f.key] = v ? "On" : "Off")} />
          {:else if f.kind === "choice"}
            <PopUp label={f.title} bind:value={draft[f.key]} options={f.choices!.some((c) => c.value === draft[f.key]) ? f.choices! : [{ value: draft[f.key], label: draft[f.key] }, ...f.choices!]} />
          {:else}
            <div class="field" class:bad={!valid(f, draft[f.key] ?? "")}>
              <TextField label={f.title} bind:value={draft[f.key]} width="96px" />
            </div>
          {/if}
        </Row>
      {/each}
    {/snippet}

    {#if invalid.length}
      <p class="warn error" role="alert">{invalid.map((f) => f.title).join(", ")}: {invalid.some((f) => f.kind === "size") ? SIZE_HINT : "Enter a whole number."}</p>
    {:else if postTooSmall}
      <p class="warn" role="status">Maximum POST size is smaller than the upload size, so large uploads will still fail.</p>
    {/if}

    <Group title="Uploads and Memory">{@render fields(limits)}</Group>
    <Group title="Timeouts">{@render fields(timeouts)}</Group>
    <Group title="Forms and Sessions">{@render fields(input)}</Group>
    <Group title="Errors">{@render fields(errors)}</Group>
    <Group title="Performance">{@render fields(opcache)}</Group>
    <Group title="Date and Time">{@render fields(locale)}</Group>

    <Group title="Extensions" footer="Corral’s PHP has its extensions built in. To add one, switch to a build that includes it. Your settings stay as they are.">
      {#snippet actions()}
        <SearchField bind:value={extQuery} placeholder="Find Extension" />
      {/snippet}
      <Row
        title={details.build === "extended" ? "Extended build" : "Standard build"}
        description={details.build === "extended"
          ? "Adds mysqli, intl, imagick, apcu, swoole, ldap, imap, sodium and more."
          : "The usual web extensions. Switch to Extended for mysqli, intl, imagick, apcu, swoole and more."}
      >
        <Button size="small" variant={details.build === "standard" ? "primary" : "default"} disabled={!!app.busy} onclick={() => (switchTo = details!.build === "standard" ? "extended" : "standard")}>
          {app.busy === "phpbuild" ? "Switching…" : details.build === "standard" ? "Switch to Extended…" : "Switch to Standard…"}
        </Button>
      </Row>
      <Row title="Xdebug" description={details.xdebug ? "Available. Start a debug session from your IDE or browser extension." : "Not included in this build."} />
      <div class="mods selectable">
        {#each shown as m (m)}<span class="mod">{m}</span>{/each}
        {#if missingHint}<p class="hint">{missingHint}</p>{/if}
      </div>
    </Group>

    <Group title="Advanced">
      <Row title="Edit php.ini overrides" description="Any directive, as text. Saved to 90-user.ini, which loads after Corral’s defaults.">
        <Button size="small" disabled={!!app.busy} onclick={() => app.act("ini", async () => { iniText = await api.readPhpIni(version); })}>Edit…</Button>
      </Row>
      <Row title="Restore defaults" description={overridden.length ? `Removes your changes to ${overridden.length} setting${overridden.length === 1 ? "" : "s"} on this page.` : "You’re using Corral’s defaults."}>
        <Button size="small" disabled={!overridden.length || !!app.busy} onclick={restoreDefaults}>Restore</Button>
      </Row>
    </Group>
  {/if}
</div>

{#if switchTo}
  <Sheet
    title="Switch PHP {version} to the {switchTo === 'extended' ? 'Extended' : 'Standard'} build?"
    message={switchTo === "extended"
      ? "Downloads about 70 MB and restarts PHP " + version + ". Sites keep working; your settings are kept."
      : "Downloads the smaller build and restarts PHP " + version + ". Extensions such as mysqli and intl will no longer be available."}
    onclose={() => (switchTo = null)}
  >
    {#snippet buttons()}
      <Button onclick={() => (switchTo = null)}>Cancel</Button>
      <Button variant="primary" onclick={() => switchBuild(switchTo!)}>Switch</Button>
    {/snippet}
  </Sheet>
{/if}

{#if iniText !== null}
  <Sheet title="PHP {version} overrides" message="One directive per line, e.g. memory_limit = 1G. PHP restarts after saving." width="560px" onclose={() => (iniText = null)}>
    <TextField label="php.ini overrides" bind:value={iniText} multiline mono rows={14} />
    {#snippet buttons()}
      <Button onclick={() => (iniText = null)}>Cancel</Button>
      <Button variant="primary" disabled={!!app.busy} onclick={() => app.act("saveini", async () => { await api.writePhpIni(version, iniText!); iniText = null; await load(version); })}>Save</Button>
    {/snippet}
  </Sheet>
{/if}

<style>
  .pane {
    flex: 1;
    overflow-y: auto;
    padding: 12px max(20px, calc((100% - 720px) / 2)) 20px;
  }
  .back {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    margin: 0 0 12px;
    padding: 2px 4px;
    border: 0;
    border-radius: 4px;
    background: none;
    color: var(--accent);
    font: inherit;
  }
  .back:hover {
    background: var(--fill);
  }
  .loading {
    color: var(--label-2);
    padding: 24px 0;
    text-align: center;
  }
  .field.bad :global(input) {
    border-color: var(--red);
  }
  .mods {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    padding: 10px 12px 12px;
    border-top: 1px solid var(--separator);
  }
  .mod {
    padding: 1px 6px;
    border-radius: 4px;
    background: var(--fill);
    font-family: var(--mono);
    font-size: 11px;
    line-height: 16px;
    color: var(--label-2);
  }
  .warn {
    margin: 0 0 16px;
    padding: 8px 12px;
    border-radius: var(--radius-control);
    background: var(--fill);
    color: var(--label);
  }
  .warn.error {
    color: var(--red);
  }
  .hint {
    color: var(--label-2);
  }
</style>
