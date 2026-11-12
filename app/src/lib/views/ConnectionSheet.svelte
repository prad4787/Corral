<!--
  How to connect to a service (host, port, user, password, URL, .env), and
  setting, changing or removing its admin password.
-->
<script lang="ts">
  import Sheet from "$lib/ui/Sheet.svelte";
  import Button from "$lib/ui/Button.svelte";
  import TextField from "$lib/ui/TextField.svelte";
  import { api, type Connection, type Service } from "$lib/api";
  import { app } from "$lib/app.svelte";

  let { service, onclose }: { service: Service; onclose: () => void } = $props();

  let conn = $state<Connection | null>(null);
  let reveal = $state(false);
  let copied = $state<string | null>(null);
  /** "view" → details; "set" → choose a password; "remove" → confirm. */
  let step = $state<"view" | "set" | "remove">("view");
  let draft = $state("");

  async function load() {
    try {
      conn = await api.serviceConnection(service.name);
    } catch (e) {
      app.error = String(e);
    }
  }
  $effect(() => {
    service.name;
    load();
  });

  async function copy(label: string, text: string) {
    try {
      await navigator.clipboard.writeText(text);
      copied = label;
      setTimeout(() => copied === label && (copied = null), 1500);
    } catch (e) {
      app.error = "Couldn’t copy: " + String(e);
    }
  }

  const masked = (s: string) => (reveal ? s : s.replace(conn?.password ?? "\u0000", "••••••••"));

  const valid = $derived(draft.length >= 8 && draft.length <= 128 && /^[!%&()*+,\-./0-9:;<=>?@A-Z\[\]^_a-z{|}~]+$/.test(draft));
  // MySQL and PostgreSQL change the password inside the running server.
  const needsRunning = $derived(service.kind === "mysql" || service.kind === "postgres");
  const canChange = $derived(!needsRunning || service.listening);

  async function startSet() {
    draft = "";
    try {
      draft = await api.generatePassword();
    } catch {}
    step = "set";
  }

  async function save(pw: string | null) {
    const ok = await app.run("svcpw", () => api.setServicePassword(service.name, pw));
    if (ok) {
      await load();
      step = "view";
      reveal = pw !== null;
      app.notice = pw ? `${service.label} now requires a password.` : `${service.label} no longer requires a password.`;
    }
  }

  const affects: Record<string, string> = {
    mysql: "DB_PASSWORD",
    postgres: "DB_PASSWORD",
    valkey: "REDIS_PASSWORD",
  };
</script>

<Sheet
  title={step === "set" ? `${conn?.password ? "Change" : "Set"} ${service.label} Password` : step === "remove" ? `Remove the ${service.label} password?` : `Connect to ${service.label}`}
  message={step === "set"
    ? `Sites and apps must then use this password. Update ${affects[service.kind] ?? "the password"} in each project’s .env.`
    : step === "remove"
      ? "Anything running on this Mac can then connect without a password. That’s the usual setup for local development."
      : undefined}
  width="500px"
  {onclose}
>
  {#if !conn}
    <p class="muted">Loading…</p>
  {:else if step === "set"}
    <div class="setpw">
      <TextField label="New password" bind:value={draft} mono width="100%" />
      <Button size="small" onclick={async () => (draft = await api.generatePassword())}>Generate</Button>
    </div>
    <p class="hint" class:bad={!!draft && !valid}>
      8–128 characters. Letters, digits and symbols, except spaces, quotes, backslashes, $, # and `.
    </p>
  {:else if step === "view"}
    <dl class="grid">
      <dt>Host</dt>
      <dd><code class="selectable">{conn.host}</code><button class="copy" onclick={() => copy("host", conn!.host)}>{copied === "host" ? "Copied" : "Copy"}</button></dd>
      <dt>Port</dt>
      <dd><code class="selectable">{conn.port}</code><button class="copy" onclick={() => copy("port", String(conn!.port))}>{copied === "port" ? "Copied" : "Copy"}</button></dd>
      {#if conn.user}
        <dt>User</dt>
        <dd><code class="selectable">{conn.user}</code><button class="copy" onclick={() => copy("user", conn!.user!)}>{copied === "user" ? "Copied" : "Copy"}</button></dd>
      {/if}
      <dt>Password</dt>
      <dd>
        {#if conn.password}
          <code class="selectable">{reveal ? conn.password : "••••••••"}</code>
          <button class="copy" onclick={() => (reveal = !reveal)}>{reveal ? "Hide" : "Show"}</button>
          <button class="copy" onclick={() => copy("pw", conn!.password!)}>{copied === "pw" ? "Copied" : "Copy"}</button>
        {:else}
          <span class="muted">None</span>
        {/if}
      </dd>
      <dt>URL</dt>
      <dd><code class="selectable url">{masked(conn.url)}</code><button class="copy" onclick={() => copy("url", conn!.url)}>{copied === "url" ? "Copied" : "Copy"}</button></dd>
    </dl>
    {#if conn.note}<p class="hint">{conn.note}</p>{/if}

    <div class="env">
      <div class="env-head">
        <span>Laravel .env</span>
        <button class="copy" onclick={() => copy("env", conn!.env)}>{copied === "env" ? "Copied" : "Copy"}</button>
      </div>
      <pre class="selectable">{masked(conn.env)}</pre>
    </div>

    {#if conn.can_secure}
      <div class="security" class:open={!conn.password}>
        <div>
          <strong>{conn.password ? "Password protected" : "No password"}</strong>
          <p>
            {conn.password
              ? "Only apps with the password can connect. Corral’s tools and Terminal commands use it automatically."
              : "Any app on this Mac can connect as the admin user. Nothing outside this Mac can reach it."}
          </p>
          {#if !canChange}<p class="muted">Turn {service.label} on to change its password.</p>{/if}
        </div>
      </div>
    {/if}
  {/if}

  {#snippet buttons()}
    {#if step === "set"}
      <Button onclick={() => (step = "view")}>Back</Button>
      <Button variant="primary" disabled={!valid || !!app.busy} onclick={() => save(draft)}>{app.busy === "svcpw" ? "Saving…" : "Save Password"}</Button>
    {:else if step === "remove"}
      <Button onclick={() => (step = "view")}>Back</Button>
      <Button variant="destructive" disabled={!!app.busy} onclick={() => save(null)}>{app.busy === "svcpw" ? "Removing…" : "Remove Password"}</Button>
    {:else}
      {#if conn?.can_secure}
        {#if conn.password}
          <Button disabled={!canChange || !!app.busy} onclick={() => (step = "remove")}>Remove Password…</Button>
          <Button disabled={!canChange || !!app.busy} onclick={startSet}>Change Password…</Button>
        {:else}
          <Button disabled={!canChange || !!app.busy} onclick={startSet}>Set Password…</Button>
        {/if}
      {/if}
      <Button variant="primary" onclick={onclose}>Done</Button>
    {/if}
  {/snippet}
</Sheet>

<style>
  .grid {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 6px 12px;
    align-items: center;
    margin: 0;
  }
  dt {
    color: var(--label-2);
    text-align: right;
  }
  dd {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    margin: 0;
  }
  code {
    font-family: var(--mono);
    font-size: 12px;
  }
  .url {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .copy {
    flex: none;
    padding: 0 6px;
    height: 18px;
    border: 0;
    border-radius: 4px;
    background: var(--fill);
    color: var(--accent);
    font: inherit;
    font-size: 11px;
  }
  .copy:hover {
    background: var(--fill-strong);
  }
  .env {
    margin-top: 14px;
    border-radius: var(--radius-control);
    background: var(--fill);
  }
  .env-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 6px 10px 0;
    font-size: 11px;
    color: var(--label-2);
  }
  pre {
    margin: 0;
    padding: 6px 10px 10px;
    font-family: var(--mono);
    font-size: 11.5px;
    line-height: 16px;
    white-space: pre-wrap;
    word-break: break-all;
  }
  .security {
    margin-top: 14px;
    padding: 10px 12px;
    border-radius: var(--radius-control);
    border: 0.5px solid var(--separator);
  }
  .security p {
    margin-top: 2px;
    font-size: 11px;
    line-height: 15px;
    color: var(--label-2);
  }
  .setpw {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .setpw :global(input) {
    flex: 1;
  }
  .hint {
    margin-top: 8px;
    font-size: 11px;
    line-height: 15px;
    color: var(--label-2);
  }
  .hint.bad {
    color: var(--red);
  }
  .muted {
    color: var(--label-2);
  }
</style>
