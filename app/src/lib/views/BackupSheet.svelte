<!--
  Back up or restore one database service. Backups go wherever the user
  saves them; restores read any file of a format the service understands.
-->
<script lang="ts">
  import { save as saveDialog, open as openDialog } from "@tauri-apps/plugin-dialog";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import { downloadDir, join } from "@tauri-apps/api/path";
  import Sheet from "$lib/ui/Sheet.svelte";
  import Button from "$lib/ui/Button.svelte";
  import PopUp from "$lib/ui/PopUp.svelte";
  import TextField from "$lib/ui/TextField.svelte";
  import { api, type BackupOptions, type BackupFormat, type Service } from "$lib/api";
  import { app, fmtSize } from "$lib/app.svelte";

  let { service, mode, onclose }: { service: Service; mode: "backup" | "restore"; onclose: () => void } = $props();

  const ALL = "*";
  const NEW = "+";

  let opts = $state<BackupOptions | null>(null);
  let database = $state(ALL);
  let format = $state<BackupFormat>("sql");
  let saved = $state<{ path: string; size: number } | null>(null);

  // Restore state.
  let file = $state<string | null>(null);
  let target = $state(NEW);
  let newName = $state("");

  $effect(() => {
    api
      .backupOptions(service.name)
      .then((o) => {
        opts = o;
        format = o.formats[0]?.id ?? "sql";
      })
      .catch((e) => (app.error = String(e)));
  });

  const formats = $derived(opts ? opts.formats.filter((f) => database !== ALL || f.whole_server) : []);
  // Keep the chosen format valid when the database choice changes.
  $effect(() => {
    if (formats.length && !formats.some((f) => f.id === format)) format = formats[0].id;
  });

  const fileName = $derived(file?.split("/").pop() ?? "");
  const isCustom = $derived(/\.(dump|backup)$/i.test(fileName));

  const stamp = () => {
    const d = new Date();
    const p = (n: number) => String(n).padStart(2, "0");
    return `${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}-${p(d.getHours())}${p(d.getMinutes())}`;
  };

  /** A database name guessed from a backup file name, e.g. shop-20261112-0930.sql.gz → shop. */
  function guessName(name: string) {
    const stem = name.replace(/\.(sql\.gz|sql|dump|backup|gz)$/i, "");
    const pre = stem.startsWith(service.name + "-") ? stem.slice(service.name.length + 1) : stem;
    const g = pre.replace(/-\d{8}-\d{4}$/, "").replace(/[^A-Za-z0-9_-]/g, "_").slice(0, 64);
    return g && g !== "all" ? g : "";
  }

  async function runBackup() {
    const f = opts!.formats.find((x) => x.id === format)!;
    let dir = "";
    try {
      dir = await downloadDir();
    } catch {}
    const base = `${service.name}-${database === ALL ? "all" : database}-${stamp()}.${f.extension}`;
    const dest = await saveDialog({
      title: `Back Up ${service.label}`,
      defaultPath: dir ? await join(dir, base) : base,
      filters: [{ name: f.label, extensions: [f.extension.split(".").pop()!] }],
    });
    if (!dest) return;
    await app.act("backup", async () => {
      const size = await api.backupDatabase(service.name, database === ALL ? null : database, format, dest);
      saved = { path: dest, size };
    });
  }

  const extensions: Record<string, string[]> = {
    mysql: ["sql", "gz"],
    postgres: ["sql", "gz", "dump", "backup"],
    valkey: ["rdb"],
    mongodb: ["gz", "archive"],
  };

  async function chooseFile() {
    const f = await openDialog({
      title: `Choose a ${service.label} backup`,
      multiple: false,
      directory: false,
      filters: [{ name: "Backups", extensions: extensions[service.kind] ?? ["*"] }],
    });
    if (typeof f !== "string") return;
    file = f;
    const g = guessName(f.split("/").pop() ?? "");
    if (opts?.per_database) {
      if (g && opts.databases.includes(g)) target = g;
      else {
        target = NEW;
        newName = g;
      }
    }
  }

  const targetDb = $derived(!opts?.per_database ? null : target === ALL ? null : target === NEW ? newName.trim() : target);
  const nameOk = $derived(target !== NEW || /^[A-Za-z0-9_-]{1,64}$/.test(newName.trim()));
  const replacing = $derived(
    service.kind === "valkey" || target === ALL || (target !== NEW && !!target) || (target === NEW && !!opts?.databases.includes(newName.trim())),
  );

  async function runRestore() {
    const ok = await app.act("restore", () => api.restoreDatabase(service.name, targetDb, file!));
    if (ok) {
      app.notice =
        service.kind === "valkey"
          ? `${service.label} restarted with the snapshot.`
          : `Restored ${fileName} into ${targetDb ?? service.label}.`;
      onclose();
    }
  }
</script>

<Sheet
  title={mode === "backup" ? `Back Up ${service.label}` : `Restore ${service.label}`}
  message={mode === "backup"
    ? "Saves a copy you can restore here or load into another server."
    : service.kind === "valkey"
      ? "Valkey stops, loads the snapshot, and starts again. Every key is replaced; the current data is kept as dump.rdb.before-restore."
      : "Loads a backup made by Corral or by the server’s own tools."}
  width="460px"
  {onclose}
>
  {#if !opts}
    <p class="muted">Loading…</p>
  {:else if opts.unavailable}
    <p class="muted">{opts.unavailable}</p>
  {:else if mode === "backup" && saved}
    <div class="done">
      <p><strong>{saved.path.split("/").pop()}</strong> · {fmtSize(saved.size)}</p>
      <p class="muted path">{saved.path}</p>
    </div>
  {:else if mode === "backup"}
    <div class="form">
      {#if opts.per_database}
        <span class="label">Database</span>
        <PopUp
          label="Database"
          bind:value={database}
          options={[{ value: ALL, label: "All databases" }, ...opts.databases.map((d) => ({ value: d, label: d }))]}
        />
      {/if}
      <span class="label">Format</span>
      <PopUp label="Format" bind:value={format} options={formats.map((f) => ({ value: f.id, label: f.label }))} />
    </div>
    <p class="hint">
      {#if format === "sql"}Plain text. Opens in any editor and loads into other versions.
      {:else if format === "sql-gz"}The same SQL, compressed. Usually 5–10× smaller.
      {:else if format === "pg-custom"}Compressed. Restores faster and can restore single tables with pg_restore.
      {:else if format === "rdb"}The whole server: all 16 databases and their keys.
      {:else}Compressed archive for mongorestore.{/if}
      {#if opts.per_database && database === ALL}
        Includes every database except the server’s own system databases.
      {/if}
    </p>
  {:else}
    <div class="form">
      <span class="label">Backup file</span>
      <div class="file">
        <span class="name" title={file ?? undefined}>{fileName || "None chosen"}</span>
        <Button size="small" onclick={chooseFile}>Choose…</Button>
      </div>
      {#if opts.per_database}
        <span class="label">Restore into</span>
        <PopUp
          label="Restore into"
          bind:value={target}
          options={[
            { value: NEW, label: "New database…" },
            ...opts.databases.map((d) => ({ value: d, label: d })),
            ...(isCustom ? [] : [{ value: ALL, label: "Databases named in the backup" }]),
          ]}
        />
        {#if target === NEW}
          <span class="label">Name</span>
          <div class="name-field" class:bad={!!newName && !nameOk}>
            <TextField label="New database name" bind:value={newName} placeholder="my_app" width="220px" />
          </div>
        {/if}
      {/if}
    </div>
    {#if file && opts.per_database}
      <p class="hint" class:warn={replacing}>
        {#if target === ALL}
          For backups of all databases. Each database in the file is created or updated.
        {:else if replacing}
          Tables in “{targetDb}” that are also in the backup are replaced.
        {:else if targetDb}
          Creates “{targetDb}” and loads the backup into it.
        {:else}
          Letters, digits, “_” and “-”.
        {/if}
      </p>
    {/if}
  {/if}

  {#snippet buttons()}
    {#if mode === "backup" && saved}
      <Button onclick={() => revealItemInDir(saved!.path).catch((e) => (app.error = String(e)))}>Show in Finder</Button>
      <Button variant="primary" onclick={onclose}>Done</Button>
    {:else}
      <Button onclick={onclose}>Cancel</Button>
      {#if mode === "backup"}
        <Button variant="primary" disabled={!opts || !!opts.unavailable || !!app.busy || !formats.length} onclick={runBackup}>
          {app.busy === "backup" ? "Backing Up…" : "Back Up…"}
        </Button>
      {:else}
        <Button
          variant={replacing ? "destructive" : "primary"}
          disabled={!opts || !!opts.unavailable || !!app.busy || !file || !nameOk || (opts.per_database && !targetDb && target !== ALL)}
          onclick={runRestore}
        >
          {app.busy === "restore" ? "Restoring…" : "Restore"}
        </Button>
      {/if}
    {/if}
  {/snippet}
</Sheet>

<style>
  .form {
    display: grid;
    grid-template-columns: auto 1fr;
    align-items: center;
    gap: 8px 12px;
  }
  .form :global(.popup) {
    justify-self: start;
  }
  .label {
    color: var(--label-2);
    text-align: right;
  }
  .file {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .file .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .hint {
    margin-top: 12px;
    font-size: 11px;
    line-height: 15px;
    color: var(--label-2);
  }
  .hint.warn {
    color: var(--label);
  }
  .muted {
    color: var(--label-2);
    line-height: 17px;
  }
  .done p + p {
    margin-top: 4px;
  }
  .path {
    font-size: 11px;
    word-break: break-all;
  }
  .name-field.bad :global(input) {
    border-color: var(--red);
  }
</style>
