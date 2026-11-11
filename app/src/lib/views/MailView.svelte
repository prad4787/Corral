<!-- Mail-style split view: message list, then the selected message. -->
<script lang="ts">
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import Toolbar from "$lib/ui/Toolbar.svelte";
  import Button from "$lib/ui/Button.svelte";
  import Segmented from "$lib/ui/Segmented.svelte";
  import Empty from "$lib/ui/Empty.svelte";
  import Icon from "$lib/Icon.svelte";
  import { api, type MailSummary, type MailDetail } from "$lib/api";
  import { app, fmtTime, fmtSize } from "$lib/app.svelte";

  let { mails }: { mails: MailSummary[] } = $props();
  let mail = $state<MailDetail | null>(null);
  let pane = $state("html");

  async function open(id: string) {
    await app.act("mail", async () => {
      mail = await api.mailDetail(id);
      pane = mail.html ? "html" : "text";
    });
  }
  const sender = (from: string) => from.replace(/\s*<.*>$/, "") || from;
</script>

<Toolbar title="Mail" subtitle="SMTP on 127.0.0.1:{app.data?.mail_port ?? 2525} · nothing is delivered">
  {#snippet actions()}
    {#if mail}
      <Button variant="plain" icon="trash" label="Delete Message" onclick={() => app.act("del", async () => { await api.mailDelete(mail!.id); mail = null; })} />
    {/if}
    <Button size="small" disabled={!mails.length} onclick={() => app.act("clear", async () => { await api.mailClear(); mail = null; })}>Delete All</Button>
  {/snippet}
</Toolbar>

{#if !mails.length}
  <div class="fill">
    <Empty icon="mail" title="No mail" text="Point your app at Corral and mail shows up here. In Laravel set MAIL_MAILER=smtp, MAIL_HOST=127.0.0.1 and MAIL_PORT=2525." />
  </div>
{:else}
  <div class="split">
    <ul class="list" aria-label="Messages">
      {#each mails as m (m.id)}
        <li>
          <button class="msg" class:selected={mail?.id === m.id} onclick={() => open(m.id)}>
            <span class="line"><strong>{sender(m.from)}</strong><time>{fmtTime(m.received)}</time></span>
            <span class="line subj">{m.subject || "No Subject"}{#if m.attachments}<Icon name="paperclip" size={11} />{/if}</span>
            <span class="to">To: {m.to.join(", ")}</span>
          </button>
        </li>
      {/each}
    </ul>

    <section class="reader">
      {#if !mail}
        <Empty icon="mail" title="No message selected" />
      {:else}
        <header class="head">
          <h2 class="selectable">{mail.subject || "No Subject"}</h2>
          <dl>
            <dt>From</dt><dd class="selectable">{mail.from}</dd>
            <dt>To</dt><dd class="selectable">{mail.to.join(", ")}</dd>
            {#if mail.cc.length}<dt>Cc</dt><dd class="selectable">{mail.cc.join(", ")}</dd>{/if}
            {#if mail.envelope_to.join() !== mail.to.join()}<dt>Delivered to</dt><dd class="selectable">{mail.envelope_to.join(", ")}</dd>{/if}
          </dl>
          {#if mail.attachment_list.length}
            <div class="atts">
              {#each mail.attachment_list as a (a.index)}
                <button class="att" onclick={() => app.act("att", async () => { const p = await api.mailSaveAttachment(mail!.id, a.index); app.notice = `Saved to Downloads.`; revealItemInDir(p); })}>
                  <Icon name="paperclip" size={12} /><span>{a.name}</span><small>{fmtSize(a.size)}</small>
                </button>
              {/each}
            </div>
          {/if}
          <Segmented
            label="Message view"
            bind:value={pane}
            options={[...(mail.html ? [{ value: "html", label: "HTML" }] : []), ...(mail.text ? [{ value: "text", label: "Plain Text" }] : []), { value: "headers", label: "Headers" }]}
          />
        </header>
        <div class="body">
          {#if pane === "html" && mail.html}
            <!-- Empty sandbox: no scripts, forms, popups or navigation from mail content. -->
            <iframe title="Message" sandbox="" srcdoc={mail.html}></iframe>
          {:else if pane === "text"}
            <pre class="selectable">{mail.text ?? ""}</pre>
          {:else}
            <table class="selectable">
              <tbody>{#each mail.headers as [k, v], i (i)}<tr><th>{k}</th><td>{v}</td></tr>{/each}</tbody>
            </table>
          {/if}
        </div>
      {/if}
    </section>
  </div>
{/if}

<style>
  .fill {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .split {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(240px, 300px) 1fr;
  }
  .list {
    margin: 0;
    padding: 6px;
    list-style: none;
    overflow-y: auto;
    border-right: 0.5px solid var(--separator);
  }
  .msg {
    display: flex;
    flex-direction: column;
    gap: 2px;
    width: 100%;
    padding: 8px 10px;
    border: 0;
    border-radius: 7px;
    background: none;
    color: var(--label);
    font: inherit;
    text-align: left;
  }
  .msg.selected {
    background: var(--accent);
    color: var(--accent-text);
  }
  .msg.selected .to,
  .msg.selected time {
    color: inherit;
    opacity: 0.8;
  }
  .line {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .line strong {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 600;
  }
  time {
    flex: none;
    font-size: 11px;
    color: var(--label-2);
  }
  .subj {
    overflow: hidden;
    white-space: nowrap;
  }
  .to {
    font-size: 11px;
    color: var(--label-2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .reader {
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .reader > :global(.empty) {
    flex: 1;
  }
  .head {
    padding: 16px 20px 12px;
    border-bottom: 0.5px solid var(--separator);
  }
  h2 {
    font-size: 15px;
    line-height: 20px;
    font-weight: 700;
  }
  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 2px 8px;
    margin: 8px 0 12px;
    font-size: 12px;
  }
  dt {
    color: var(--label-2);
  }
  dd {
    margin: 0;
    overflow-wrap: anywhere;
  }
  .atts {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-bottom: 12px;
  }
  .att {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 24px;
    padding: 0 9px;
    border: 0.5px solid var(--control-border);
    border-radius: var(--radius-control);
    background: var(--control);
    color: var(--label);
    font: inherit;
    font-size: 12px;
  }
  .att small {
    color: var(--label-2);
  }
  .body {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }
  iframe {
    display: block;
    width: 100%;
    height: 100%;
    border: 0;
    background: #fff;
  }
  pre {
    margin: 0;
    padding: 16px 20px;
    font-family: var(--font);
    font-size: 13px;
    line-height: 19px;
    white-space: pre-wrap;
  }
  table {
    border-collapse: collapse;
    margin: 12px 20px;
    font-size: 12px;
  }
  th {
    padding: 3px 12px 3px 0;
    text-align: right;
    vertical-align: top;
    font-weight: 500;
    color: var(--label-2);
    white-space: nowrap;
  }
  td {
    padding: 3px 0;
    font-family: var(--mono);
    font-size: 11px;
    word-break: break-all;
  }
</style>
