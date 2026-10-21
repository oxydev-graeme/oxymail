<script>
  import { onMount, untrack } from "svelte";
  import { api, accountLabel, fileToBase64, formatSize, escapeHtml } from "./api.js";
  import Icon from "./Icon.svelte";

  /**
   * draft: { account_id, to, cc, subject, quote (HTML), in_reply_to, references, note }
   * accounts: live accounts that can send.
   */
  let { draft, accounts, onclose, onsent } = $props();

  // The draft is read once, when the window opens.
  const initial = untrack(() => ({ ...draft, account_id: draft.account_id ?? accounts[0]?.id }));
  let accountId = $state(initial.account_id);
  let to = $state(initial.to ?? "");
  let cc = $state(initial.cc ?? "");
  let bcc = $state("");
  let showCc = $state(Boolean(initial.cc));
  let subject = $state(initial.subject ?? "");
  let attachments = $state([]);
  let sending = $state(false);
  let error = $state("");
  let editor = $state();

  let account = $derived(accounts.find((a) => a.id === accountId));

  onMount(() => {
    editor.innerHTML =
      `<p><br></p>${signatureBlock(account)}` + (draft.quote ? `<p><br></p>${draft.quote}` : "");
    (to ? editor : document.getElementById("compose-to"))?.focus();
  });

  function signatureBlock(a) {
    return a?.signature ? `<div data-oxymail-signature><br>${a.signature}</div>` : `<div data-oxymail-signature></div>`;
  }

  /** Swaps the signature when the From account changes. */
  function changeAccount(id) {
    accountId = id;
    const block = editor.querySelector("[data-oxymail-signature]");
    const next = accounts.find((a) => a.id === id);
    if (block) block.outerHTML = signatureBlock(next);
  }

  function exec(command, arg = null) {
    editor.focus();
    document.execCommand(command, false, arg);
  }

  function link() {
    const url = prompt("Link address (https://… or mailto:…)");
    if (url) exec("createLink", url);
  }

  async function addFiles(event) {
    const files = [...(event.currentTarget.files ?? [])];
    event.currentTarget.value = "";
    for (const file of files) {
      attachments = [
        ...attachments,
        { name: file.name, content_type: file.type || "application/octet-stream", size: file.size, data: await fileToBase64(file) },
      ];
    }
  }

  const split = (s) => s.split(/[,;]\s*|\s+(?=[^\s]+@)/).map((x) => x.trim()).filter(Boolean);

  async function send() {
    error = "";
    if (!account) return (error = "Add a live account in Settings → Accounts to send mail.");
    if (!split(to).length && !split(cc).length && !split(bcc).length) return (error = "Add at least one recipient.");
    if (!subject.trim() && !confirm("Send without a subject?")) return;
    sending = true;
    try {
      const note = await api.sendMessage(account.id, {
        to: split(to),
        cc: split(cc),
        bcc: split(bcc),
        subject,
        html: editor.innerHTML,
        text: editor.innerText,
        in_reply_to: draft.in_reply_to ?? null,
        references: draft.references ?? [],
        attachments: attachments.map(({ name, content_type, data }) => ({ name, content_type, data })),
      });
      onsent(note);
    } catch (e) {
      error = String(e);
    } finally {
      sending = false;
    }
  }

  function close() {
    const hasContent = to || subject || editor?.innerText.trim() || attachments.length;
    if (hasContent && !draft.quote && !confirm("Discard this message?")) return;
    onclose();
  }

  function onKey(event) {
    if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      send();
    } else if (event.key === "Escape") {
      close();
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="backdrop">
  <div class="compose" role="dialog" aria-label="New message">
    <header>
      <span class="title">{subject || "New message"}</span>
      <button class="icon" onclick={close} title="Close (Esc)"><Icon name="x" /></button>
    </header>

    {#if !accounts.length}
      <div class="empty">
        Sending needs a live account. Add one in <b>Settings → Accounts → Add account</b>.
      </div>
    {:else}
      <div class="fields">
        <label class="field">
          <span>From</span>
          <select value={accountId} onchange={(e) => changeAccount(Number(e.currentTarget.value))}>
            {#each accounts as a (a.id)}
              <option value={a.id}>{a.sender_name ? `${a.sender_name} <${a.name}>` : accountLabel(a)}</option>
            {/each}
          </select>
        </label>
        <label class="field">
          <span>To</span>
          <input id="compose-to" bind:value={to} placeholder="name@example.com, …" />
          {#if !showCc}<button class="link" onclick={() => (showCc = true)}>Cc / Bcc</button>{/if}
        </label>
        {#if showCc}
          <label class="field"><span>Cc</span><input bind:value={cc} /></label>
          <label class="field"><span>Bcc</span><input bind:value={bcc} /></label>
        {/if}
        <label class="field"><span>Subject</span><input bind:value={subject} /></label>
      </div>

      <div class="toolbar">
        <button onclick={() => exec("bold")} title="Bold"><b>B</b></button>
        <button onclick={() => exec("italic")} title="Italic"><i>I</i></button>
        <button onclick={() => exec("underline")} title="Underline"><u>U</u></button>
        <button onclick={() => exec("insertUnorderedList")} title="Bulleted list">• List</button>
        <button onclick={() => exec("insertOrderedList")} title="Numbered list">1. List</button>
        <button onclick={link} title="Link">Link</button>
        <span class="grow"></span>
        <label class="attach" title="Attach files">
          <Icon name="clip" size={15} /> Attach
          <input type="file" multiple onchange={addFiles} />
        </label>
      </div>

      <div class="editor" contenteditable="true" role="textbox" aria-multiline="true" tabindex="0" bind:this={editor}></div>

      {#if attachments.length}
        <div class="attachments">
          {#each attachments as a, i}
            <span class="chip">
              <span class="ellipsis">{a.name}</span>
              <span class="muted">{formatSize(a.size)}</span>
              <button onclick={() => (attachments = attachments.filter((_, j) => j !== i))} title="Remove"><Icon name="x" size={12} /></button>
            </span>
          {/each}
        </div>
      {/if}

      <footer>
        {#if error}<span class="error">{error}</span>{:else if draft.note}<span class="muted">{draft.note}</span>{/if}
        <span class="grow"></span>
        <span class="muted">Ctrl+Enter</span>
        <button class="primary" onclick={send} disabled={sending}>{sending ? "Sending…" : "Send"}</button>
      </footer>
    {/if}
  </div>
</div>

<style>
  .backdrop { position: fixed; inset: 0; display: grid; place-items: center; background: rgb(10 14 20 / 0.4); z-index: 25; }
  .compose {
    display: flex;
    flex-direction: column;
    width: min(820px, 94vw);
    height: min(680px, 90vh);
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 12px;
    box-shadow: 0 20px 60px rgb(0 0 0 / 0.3);
    overflow: hidden;
  }
  header { display: flex; align-items: center; gap: 10px; padding: 10px 12px 10px 18px; border-bottom: 1px solid var(--border); background: var(--header); }
  .title { flex: 1; font-weight: 650; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .icon { display: grid; place-items: center; padding: 4px; border-radius: 6px; color: var(--muted); }
  .icon:hover { background: var(--hover); color: var(--text); }
  .fields { padding: 4px 18px; border-bottom: 1px solid var(--border); }
  .field { display: flex; align-items: center; gap: 10px; padding: 6px 0; border-bottom: 1px solid var(--border); font-size: 13px; }
  .field:last-child { border-bottom: 0; }
  .field > span { width: 56px; color: var(--muted); flex-shrink: 0; }
  .field input, .field select { flex: 1; min-width: 0; padding: 4px 0; border: 0; background: none; color: var(--text); outline: none; font: inherit; }
  .field select { padding: 4px; border-radius: 6px; background: var(--bg); }
  .link { color: var(--accent); font-size: 12px; white-space: nowrap; }
  .toolbar { display: flex; align-items: center; gap: 2px; padding: 6px 12px; border-bottom: 1px solid var(--border); background: var(--bg); }
  .toolbar button, .attach { display: flex; align-items: center; gap: 5px; padding: 4px 9px; border-radius: 6px; font-size: 12px; color: var(--muted); cursor: pointer; }
  .toolbar button:hover, .attach:hover { background: var(--hover); color: var(--text); }
  .attach { position: relative; }
  .attach input { position: absolute; inset: 0; opacity: 0; cursor: pointer; }
  .grow { flex: 1; }
  .editor {
    flex: 1;
    overflow: auto;
    padding: 16px 20px;
    background: #fff;
    color: #1d2433;
    font: 14px/1.55 system-ui, sans-serif;
    outline: none;
    user-select: text;
  }
  .editor :global(img) { max-width: 100%; height: auto; }
  .editor :global(blockquote) { margin: 0 0 0 0.8ex; padding-left: 1ex; border-left: 2px solid #c8d3d8; color: #4a5563; }
  .attachments { display: flex; flex-wrap: wrap; gap: 6px; padding: 8px 18px; border-top: 1px solid var(--border); }
  .chip { display: flex; align-items: center; gap: 6px; max-width: 260px; padding: 4px 6px 4px 10px; border: 1px solid var(--border); border-radius: 8px; font-size: 12px; }
  .chip button { display: grid; place-items: center; color: var(--muted); }
  footer { display: flex; align-items: center; gap: 12px; padding: 10px 18px; border-top: 1px solid var(--border); }
  .primary { padding: 8px 22px; border-radius: 8px; background: var(--accent); color: #fff; font-weight: 600; }
  .primary:disabled { opacity: 0.6; cursor: default; }
  .muted { color: var(--muted); font-size: 12px; }
  .error { color: var(--danger); font-size: 13px; user-select: text; }
  .empty { margin: auto; padding: 24px; color: var(--muted); text-align: center; }
  .ellipsis { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>
