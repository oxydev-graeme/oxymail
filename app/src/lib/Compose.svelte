<script>
  import { onMount, untrack } from "svelte";
  import { api, accountLabel, fileToBase64, formatSize } from "./api.js";
  import Icon from "./Icon.svelte";
  import RecipientInput from "./RecipientInput.svelte";
  import TimeMenu from "./TimeMenu.svelte";

  /**
   * draft: { account_id, to, cc, bcc, subject, quote (HTML), html (whole body, when reopening),
   *          in_reply_to, references, attachments, forward_from, forwarded, note }
   * accounts: live accounts that can send.
   * onsend(accountId, mail, reopen): hands the message over; `reopen` is the draft to bring back
   * if the send is undone or fails.
   */
  let { draft, accounts, onclose, onsend, onschedule } = $props();
  let pickingTime = $state(false);

  // The draft is read once, when the window opens.
  const initial = untrack(() => ({ ...draft, account_id: draft.account_id ?? accounts[0]?.id }));
  let accountId = $state(initial.account_id);
  let to = $state(initial.to ?? "");
  let cc = $state(initial.cc ?? "");
  let bcc = $state(initial.bcc ?? "");
  let showCc = $state(Boolean(initial.cc || initial.bcc));
  let subject = $state(initial.subject ?? "");
  let attachments = $state(initial.attachments ?? []);
  /** Attachments carried over from the forwarded message: { index, name, size }. */
  let forwarded = $state(initial.forwarded ?? []);
  let error = $state("");
  let editor = $state();

  let account = $derived(accounts.find((a) => a.id === accountId));

  // ---- Autosave to local drafts --------------------------------------------
  let draftId = $state(initial.draft_id ?? null);
  let bodyVersion = $state(0);
  let saveTimer;

  function snapshot() {
    return { ...initial, account_id: accountId, to, cc, bcc, subject, html: editor?.innerHTML ?? "", attachments, forwarded, draft_id: draftId, error: undefined };
  }

  const hasContent = () => Boolean(to.trim() || cc.trim() || bcc.trim() || subject.trim() || editor?.innerText.trim() || attachments.length);

  async function saveDraft() {
    clearTimeout(saveTimer);
    if (!editor || !hasContent()) return;
    try {
      draftId = await api.saveDraft(draftId, accountId ?? null, subject, to, JSON.stringify(snapshot()));
    } catch {
      // Autosave is best-effort; the Compose window still holds everything.
    }
  }

  $effect(() => {
    // Re-run on any change to the fields or the body.
    [to, cc, bcc, subject, attachments, forwarded, accountId, bodyVersion];
    clearTimeout(saveTimer);
    saveTimer = setTimeout(saveDraft, 1200);
  });

  async function discard() {
    clearTimeout(saveTimer);
    if (draftId) await api.deleteDraft(draftId).catch(() => {});
    onclose(false);
  }

  onMount(() => {
    editor.innerHTML =
      initial.html ?? `<p><br></p>${signatureBlock(account)}` + (initial.quote ? `<p><br></p>${initial.quote}` : "");
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

  /** Validates and builds the outgoing message; null (with an error shown) if not ready. */
  function outgoing() {
    error = "";
    if (!account) return (error = "Add a live account in Settings → Accounts to send mail."), null;
    if (!split(to).length && !split(cc).length && !split(bcc).length) return (error = "Add at least one recipient."), null;
    return {
      to: split(to),
      cc: split(cc),
      bcc: split(bcc),
      subject,
      html: editor.innerHTML,
      text: editor.innerText,
      in_reply_to: initial.in_reply_to ?? null,
      references: initial.references ?? [],
      attachments: attachments.map(({ name, content_type, data }) => ({ name, content_type, data })),
      forward_from: forwarded.length ? initial.forward_from : null,
      forward_attachments: forwarded.map((a) => a.index),
    };
  }

  async function sendLater(at, when) {
    pickingTime = false;
    const mail = outgoing();
    if (!mail) return;
    clearTimeout(saveTimer);
    onschedule(account.id, mail, { ...snapshot(), account_id: account.id }, at, when);
  }

  async function send() {
    const mail = outgoing();
    if (!mail) return;
    if (!subject.trim() && !confirm("Send without a subject?")) return;
    clearTimeout(saveTimer);
    await saveDraft();
    const reopen = { ...snapshot(), account_id: account.id };
    onsend(account.id, mail, reopen);
  }

  /** Closing keeps the message as a draft; Discard throws it away. */
  async function close() {
    clearTimeout(saveTimer);
    if (hasContent()) {
      await saveDraft();
      onclose(true);
    } else {
      if (draftId) await api.deleteDraft(draftId).catch(() => {});
      onclose(false);
    }
  }

  // ---- Window geometry: drag by the header, resize from any edge, maximise ----
  const GEOMETRY_KEY = "oxymail.compose.geometry";
  const MIN_W = 480, MIN_H = 360, MARGIN = 8;
  let maximized = $state(false);
  let box = $state(loadGeometry());

  function loadGeometry() {
    let saved = null;
    try {
      saved = JSON.parse(localStorage.getItem(GEOMETRY_KEY) ?? "null");
    } catch {}
    if (saved?.maximized) maximized = true;
    const w = Math.min(saved?.w ?? 820, innerWidth * 0.94);
    const h = Math.min(saved?.h ?? 680, innerHeight * 0.9);
    return clamp({ x: saved?.x ?? (innerWidth - w) / 2, y: saved?.y ?? (innerHeight - h) / 2, w, h });
  }

  function saveGeometry() {
    try {
      localStorage.setItem(GEOMETRY_KEY, JSON.stringify({ ...box, maximized }));
    } catch {}
  }

  /** Keeps the window on screen and no smaller than the minimum. */
  function clamp({ x, y, w, h }) {
    w = Math.max(Math.min(w, innerWidth - 2 * MARGIN), Math.min(MIN_W, innerWidth - 2 * MARGIN));
    h = Math.max(Math.min(h, innerHeight - 2 * MARGIN), Math.min(MIN_H, innerHeight - 2 * MARGIN));
    x = Math.min(Math.max(x, MARGIN), innerWidth - w - MARGIN);
    y = Math.min(Math.max(y, MARGIN), innerHeight - h - MARGIN);
    return { x, y, w, h };
  }

  /** Starts a move (edges = "") or a resize from the given edges ("n", "se", …). */
  function startDrag(event, edges = "") {
    if (event.button !== 0 || maximized) return;
    if (!edges && event.target.closest("button")) return;
    event.preventDefault();
    const start = { px: event.clientX, py: event.clientY, ...box };
    const el = event.currentTarget;
    el.setPointerCapture(event.pointerId);
    const move = (e) => {
      const dx = e.clientX - start.px, dy = e.clientY - start.py;
      if (!edges) {
        box = clamp({ ...start, x: start.x + dx, y: start.y + dy });
        return;
      }
      let { x, y, w, h } = start;
      if (edges.includes("e")) w = Math.max(MIN_W, start.w + dx);
      if (edges.includes("s")) h = Math.max(MIN_H, start.h + dy);
      if (edges.includes("w")) { w = Math.max(MIN_W, start.w - dx); x = start.x + start.w - w; }
      if (edges.includes("n")) { h = Math.max(MIN_H, start.h - dy); y = start.y + start.h - h; }
      box = clamp({ x, y, w, h });
    };
    const end = () => {
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", end);
      el.removeEventListener("pointercancel", end);
      saveGeometry();
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", end);
    el.addEventListener("pointercancel", end);
  }

  function toggleMaximized() {
    maximized = !maximized;
    saveGeometry();
  }

  const EDGES = ["n", "s", "e", "w", "ne", "nw", "se", "sw"];

  function onKey(event) {
    if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      send();
    } else if (event.key === "Escape") {
      close();
    }
  }
</script>

<svelte:window onkeydown={onKey} onresize={() => (box = clamp(box))} />

<div class="backdrop">
  <div
    class="compose"
    class:maximized
    role="dialog"
    aria-label="New message"
    style:left="{box.x}px"
    style:top="{box.y}px"
    style:width="{box.w}px"
    style:height="{box.h}px"
  >
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <header onpointerdown={(e) => startDrag(e)} ondblclick={(e) => !e.target.closest("button") && toggleMaximized()}>
      <span class="title">{subject || "New message"}</span>
      <button class="icon" onclick={toggleMaximized} title={maximized ? "Restore size" : "Maximise"}>
        <Icon name={maximized ? "unmaximize" : "maximize"} size={16} />
      </button>
      <button class="icon" onclick={close} title="Close and keep as draft (Esc)"><Icon name="x" /></button>
    </header>
    {#if !maximized}
      {#each EDGES as edge}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div class="grip {edge}" onpointerdown={(e) => startDrag(e, edge)}></div>
      {/each}
    {/if}

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
          <RecipientInput id="compose-to" bind:value={to} placeholder="Type a name or address" />
          {#if !showCc}<button class="link" onclick={() => (showCc = true)}>Cc / Bcc</button>{/if}
        </label>
        {#if showCc}
          <label class="field"><span>Cc</span><RecipientInput bind:value={cc} /></label>
          <label class="field"><span>Bcc</span><RecipientInput bind:value={bcc} /></label>
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

      <div class="editor" contenteditable="true" role="textbox" aria-multiline="true" tabindex="0" bind:this={editor} oninput={() => bodyVersion++}></div>

      {#if attachments.length || forwarded.length}
        <div class="attachments">
          {#each forwarded as a, i (a.index)}
            <span class="chip" title="From the forwarded message">
              <Icon name="forward" size={12} />
              <span class="ellipsis">{a.name}</span>
              <span class="muted">{formatSize(a.size)}</span>
              <button onclick={() => (forwarded = forwarded.filter((_, j) => j !== i))} title="Remove"><Icon name="x" size={12} /></button>
            </span>
          {/each}
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
        {#if error}<span class="error">{error}</span>{:else if initial.error}<span class="error">{initial.error}</span>{:else if initial.note}<span class="muted">{initial.note}</span>{/if}
        <span class="grow"></span>
        <button class="ghost" onclick={discard} title="Delete this draft">Discard</button>
        <span class="muted">Ctrl+Enter</span>
        <button class="ghost later" onclick={() => (pickingTime = true)} title="Choose when to send"><Icon name="clock" size={15} /> Send later</button>
        <button class="primary" onclick={send}>Send</button>
      </footer>
    {/if}
  </div>
</div>

{#if pickingTime}
  <TimeMenu title="Send later" onpick={sendLater} onclose={() => (pickingTime = false)} />
{/if}

<style>
  .backdrop { position: fixed; inset: 0; background: rgb(10 14 20 / 0.25); z-index: 25; }
  .compose {
    position: absolute;
    display: flex;
    flex-direction: column;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 12px;
    box-shadow: 0 20px 60px rgb(0 0 0 / 0.3);
    overflow: hidden;
  }
  .compose.maximized { left: 8px !important; top: 8px !important; width: calc(100% - 16px) !important; height: calc(100% - 16px) !important; }
  header { display: flex; align-items: center; gap: 6px; padding: 10px 12px 10px 18px; border-bottom: 1px solid var(--border); background: var(--header); cursor: move; user-select: none; touch-action: none; }
  .maximized header { cursor: default; }
  .grip { position: absolute; z-index: 2; touch-action: none; }
  .grip.n, .grip.s { left: 10px; right: 10px; height: 6px; cursor: ns-resize; }
  .grip.e, .grip.w { top: 10px; bottom: 10px; width: 6px; cursor: ew-resize; }
  .grip.n { top: 0; } .grip.s { bottom: 0; } .grip.e { right: 0; } .grip.w { left: 0; }
  .grip.ne, .grip.nw, .grip.se, .grip.sw { width: 12px; height: 12px; }
  .grip.ne { top: 0; right: 0; cursor: nesw-resize; } .grip.sw { bottom: 0; left: 0; cursor: nesw-resize; }
  .grip.nw { top: 0; left: 0; cursor: nwse-resize; } .grip.se { bottom: 0; right: 0; cursor: nwse-resize; }
  .title { flex: 1; margin-right: 4px; font-weight: 650; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
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
  .chip .muted { white-space: nowrap; flex-shrink: 0; }
  footer { display: flex; align-items: center; gap: 12px; padding: 10px 18px; border-top: 1px solid var(--border); }
  .ghost { padding: 7px 12px; border-radius: 8px; font-size: 13px; color: var(--muted); }
  .ghost:hover { background: var(--hover); color: var(--danger); }
  .later { display: flex; align-items: center; gap: 6px; color: var(--text); border: 1px solid var(--border); }
  .later:hover { color: var(--accent) !important; border-color: var(--accent); background: none !important; }
  .primary { padding: 8px 22px; border-radius: 8px; background: var(--accent); color: #fff; font-weight: 600; }
  .primary:disabled { opacity: 0.6; cursor: default; }
  .muted { color: var(--muted); font-size: 12px; }
  .error { color: var(--danger); font-size: 13px; user-select: text; }
  .empty { margin: auto; padding: 24px; color: var(--muted); text-align: center; }
  .ellipsis { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>
