<script>
  import { onMount } from "svelte";
  import { api, shortDate, longDate, formatSize, messageDocument } from "./lib/api.js";

  const PAGE = 200;

  let accounts = $state([]);
  let foldersByAccount = $state({});
  let expanded = $state({});
  let folderId = $state(null);
  let folderName = $state("");

  let list = $state([]);
  let listDone = $state(false);
  let loadingList = $state(false);
  let selectedId = $state(null);
  let view = $state(null);

  let query = $state("");
  let searching = $state(false);

  let importPath = $state("~/MailbirdExport/mail");
  let importing = $state(false);
  let toast = $state("");
  let error = $state("");

  onMount(loadAccounts);

  async function loadAccounts() {
    try {
      accounts = await api.accounts();
      if (accounts.length && folderId === null) {
        await toggleAccount(accounts[0].id, true);
        const inbox = foldersByAccount[accounts[0].id]?.[0];
        if (inbox) openFolder(inbox);
      }
    } catch (e) {
      error = String(e);
    }
  }

  async function toggleAccount(id, open = !expanded[id]) {
    expanded[id] = open;
    if (open && !foldersByAccount[id]) {
      foldersByAccount[id] = await api.folders(id);
    }
  }

  async function openFolder(folder) {
    folderId = folder.id;
    folderName = folder.name;
    query = "";
    searching = false;
    list = [];
    listDone = false;
    await loadMore();
  }

  async function loadMore() {
    if (loadingList || listDone || searching || folderId === null) return;
    loadingList = true;
    try {
      const page = await api.messages(folderId, list.length, PAGE);
      list = [...list, ...page];
      listDone = page.length < PAGE;
    } catch (e) {
      error = String(e);
    } finally {
      loadingList = false;
    }
  }

  async function runSearch(event) {
    event.preventDefault();
    if (!query.trim()) {
      clearSearch();
      return;
    }
    searching = true;
    loadingList = true;
    try {
      list = await api.search(query);
      listDone = true;
    } catch (e) {
      error = String(e);
    } finally {
      loadingList = false;
    }
  }

  function clearSearch() {
    query = "";
    const folder = Object.values(foldersByAccount).flat().find((f) => f.id === folderId);
    if (folder) openFolder(folder);
  }

  async function select(id) {
    selectedId = id;
    try {
      view = await api.message(id);
    } catch (e) {
      view = null;
      error = String(e);
    }
  }

  function onListScroll(event) {
    const el = event.currentTarget;
    if (el.scrollTop + el.clientHeight > el.scrollHeight - 600) loadMore();
  }

  function onKey(event) {
    if (event.target instanceof HTMLInputElement || !list.length) return;
    const i = list.findIndex((m) => m.id === selectedId);
    if (event.key === "ArrowDown" || event.key === "j") {
      event.preventDefault();
      const next = list[Math.min(i + 1, list.length - 1)];
      if (next) select(next.id);
    } else if (event.key === "ArrowUp" || event.key === "k") {
      event.preventDefault();
      const prev = list[Math.max(i - 1, 0)];
      if (prev) select(prev.id);
    }
  }

  async function save(index) {
    try {
      const path = await api.saveAttachment(view.id, index);
      flash(`Saved to ${path}`);
    } catch (e) {
      error = String(e);
    }
  }

  async function runImport() {
    importing = true;
    try {
      const stats = await api.importArchive(importPath);
      flash(`Imported ${stats.added.toLocaleString()} messages`);
      await loadAccounts();
    } catch (e) {
      error = String(e);
    } finally {
      importing = false;
    }
  }

  function flash(message) {
    toast = message;
    setTimeout(() => (toast = ""), 4000);
  }

  function sender(m) {
    return m.from_name || m.from_addr || "(no sender)";
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="shell">
  <aside class="sidebar">
    <div class="brand">OxyMail</div>
    <nav>
      {#each accounts as account (account.id)}
        <div class="account">
          <button class="account-name" onclick={() => toggleAccount(account.id)} title={account.name}>
            <span class="chevron" class:open={expanded[account.id]}>›</span>
            <span class="ellipsis">{account.name}</span>
          </button>
          {#if expanded[account.id]}
            {#each foldersByAccount[account.id] ?? [] as folder (folder.id)}
              <button
                class="folder"
                class:active={folder.id === folderId && !searching}
                onclick={() => openFolder(folder)}
              >
                <span class="ellipsis">{folder.name}</span>
                {#if folder.unread > 0 && folder.role !== "sent" && folder.role !== "trash" && folder.role !== "spam"}
                  <span class="count">{folder.unread.toLocaleString()}</span>
                {/if}
              </button>
            {/each}
          {/if}
        </div>
      {/each}
    </nav>
    {#if accounts.length}
      <div class="sidebar-foot">Archive · read-only</div>
    {/if}
  </aside>

  <section class="list-pane">
    <form class="search" onsubmit={runSearch}>
      <input type="search" placeholder="Search all mail" bind:value={query} />
      {#if searching}
        <button type="button" class="link" onclick={clearSearch}>Clear</button>
      {/if}
    </form>
    <div class="list-title">
      {searching ? `Results for “${query}”` : folderName}
      {#if searching && !loadingList}<span class="muted"> · {list.length}</span>{/if}
    </div>
    <div class="list" onscroll={onListScroll}>
      {#each list as m (m.id)}
        <button class="row" class:selected={m.id === selectedId} class:unread={m.unread} onclick={() => select(m.id)}>
          <div class="row-top">
            <span class="ellipsis sender">{sender(m)}</span>
            <span class="date">{shortDate(m.date)}</span>
          </div>
          <div class="row-subject ellipsis">
            {#if m.starred}<span class="star">★</span>{/if}
            {#if m.attachments > 0}<span class="clip" title="Has attachments">📎</span>{/if}
            {m.subject || "(no subject)"}
          </div>
          <div class="row-snippet ellipsis">{m.snippet}</div>
        </button>
      {:else}
        {#if !loadingList && accounts.length}
          <div class="empty">{searching ? "No messages match." : "This folder is empty."}</div>
        {/if}
      {/each}
      {#if loadingList}<div class="empty">Loading…</div>{/if}
    </div>
  </section>

  <main class="reader">
    {#if !accounts.length}
      <div class="welcome">
        <h1>Welcome to OxyMail</h1>
        <p>Import a Maildir archive, such as the Mailbird export, to get started.</p>
        <div class="import">
          <input bind:value={importPath} />
          <button class="primary" onclick={runImport} disabled={importing}>
            {importing ? "Importing…" : "Import"}
          </button>
        </div>
      </div>
    {:else if view}
      <header class="msg-head">
        <h1>{view.subject || "(no subject)"}</h1>
        <div class="meta">
          <div><span class="label">From</span>{view.from.join(", ")}</div>
          {#if view.to.length}<div><span class="label">To</span>{view.to.join(", ")}</div>{/if}
          {#if view.cc.length}<div><span class="label">Cc</span>{view.cc.join(", ")}</div>{/if}
          <div class="muted">{longDate(view.date)}</div>
        </div>
        {#each view.notices as notice}
          <div class="notice">{notice}</div>
        {/each}
        {#if view.attachments.length}
          <div class="attachments">
            {#each view.attachments as a (a.index)}
              <button class="chip" onclick={() => save(a.index)} title="Save to Downloads">
                <span class="ellipsis">{a.name}</span>
                <span class="muted">{formatSize(a.size)}</span>
              </button>
            {/each}
          </div>
        {/if}
      </header>
      {#if view.html}
        <iframe class="body-frame" title="Message" sandbox="allow-popups" srcdoc={messageDocument(view.html)}></iframe>
      {:else}
        <pre class="body-text">{view.text ?? ""}</pre>
      {/if}
    {:else}
      <div class="placeholder">Select a message to read it.</div>
    {/if}
  </main>
</div>

{#if toast}<div class="toast">{toast}</div>{/if}
{#if error}
  <button class="toast error" onclick={() => (error = "")}>{error} — dismiss</button>
{/if}

<style>
  .shell {
    display: grid;
    grid-template-columns: 250px 380px 1fr;
    height: 100%;
  }

  .sidebar {
    display: flex;
    flex-direction: column;
    background: var(--sidebar);
    border-right: 1px solid var(--border);
    min-width: 0;
  }
  .brand {
    padding: 16px 18px 10px;
    font-weight: 700;
    font-size: 16px;
    letter-spacing: -0.01em;
  }
  nav {
    flex: 1;
    overflow-y: auto;
    padding: 4px 8px 12px;
  }
  .account { margin-bottom: 2px; }
  .account-name {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    padding: 6px 8px;
    border-radius: 6px;
    font-weight: 600;
    font-size: 13px;
    text-align: left;
  }
  .account-name:hover { background: var(--border); }
  .chevron { display: inline-block; width: 10px; color: var(--muted); transition: transform 0.12s; }
  .chevron.open { transform: rotate(90deg); }
  .folder {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    width: 100%;
    padding: 5px 8px 5px 26px;
    border-radius: 6px;
    font-size: 13px;
    text-align: left;
    color: var(--muted);
  }
  .folder:hover { background: var(--border); color: var(--text); }
  .folder.active { background: var(--selected); color: var(--text); font-weight: 600; }
  .count { font-size: 12px; color: var(--accent); font-weight: 600; }
  .sidebar-foot { padding: 10px 18px; font-size: 12px; color: var(--faint); border-top: 1px solid var(--border); }

  .list-pane {
    display: flex;
    flex-direction: column;
    background: var(--panel);
    border-right: 1px solid var(--border);
    min-width: 0;
  }
  .search {
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 12px 12px 6px;
  }
  .search input {
    flex: 1;
    padding: 8px 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--bg);
    outline: none;
  }
  .search input:focus { border-color: var(--accent); }
  .link { color: var(--accent); font-size: 13px; }
  .list-title { padding: 6px 16px 8px; font-weight: 600; font-size: 13px; }
  .list { flex: 1; overflow-y: auto; }
  .row {
    display: block;
    width: 100%;
    padding: 10px 16px;
    text-align: left;
    border-bottom: 1px solid var(--border);
    border-left: 3px solid transparent;
  }
  .row:hover { background: var(--bg); }
  .row.selected { background: var(--selected); }
  .row.unread { border-left-color: var(--accent); }
  .row.unread .sender, .row.unread .row-subject { font-weight: 700; }
  .row-top { display: flex; justify-content: space-between; gap: 10px; }
  .sender { font-size: 13px; }
  .date { flex-shrink: 0; font-size: 12px; color: var(--muted); }
  .row-subject { font-size: 13px; margin-top: 1px; }
  .row-snippet { font-size: 12px; color: var(--muted); margin-top: 2px; }
  .star { color: #e3a400; margin-right: 4px; }
  .clip { margin-right: 4px; font-size: 11px; }

  .reader {
    display: flex;
    flex-direction: column;
    min-width: 0;
    background: var(--panel);
  }
  .msg-head { padding: 20px 24px 14px; border-bottom: 1px solid var(--border); }
  .msg-head h1 { margin: 0 0 10px; font-size: 19px; line-height: 1.3; font-weight: 650; }
  .meta { font-size: 13px; display: grid; gap: 2px; }
  .label { display: inline-block; width: 42px; color: var(--muted); }
  .notice {
    margin-top: 10px;
    padding: 8px 10px;
    border-radius: 6px;
    background: var(--warn-bg);
    color: var(--warn-text);
    font-size: 13px;
  }
  .attachments { display: flex; flex-wrap: wrap; gap: 8px; margin-top: 12px; }
  .chip {
    display: flex;
    gap: 8px;
    max-width: 280px;
    padding: 6px 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    font-size: 12px;
  }
  .chip:hover { border-color: var(--accent); background: var(--accent-soft); }
  .body-frame { flex: 1; width: 100%; border: 0; background: #fff; }
  .body-text {
    flex: 1;
    margin: 0;
    padding: 20px 24px;
    overflow: auto;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    font: 14px/1.5 ui-monospace, "Cascadia Mono", "Ubuntu Mono", monospace;
  }

  .placeholder, .empty { padding: 24px; color: var(--muted); text-align: center; }
  .placeholder { margin: auto; }
  .welcome { margin: auto; max-width: 460px; padding: 24px; }
  .welcome h1 { font-size: 22px; margin: 0 0 6px; }
  .welcome p { color: var(--muted); margin: 0 0 16px; }
  .import { display: flex; gap: 8px; }
  .import input {
    flex: 1;
    padding: 8px 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--bg);
  }
  .primary {
    padding: 8px 16px;
    border-radius: var(--radius);
    background: var(--accent);
    color: #fff;
    font-weight: 600;
  }
  .primary:disabled { opacity: 0.6; cursor: default; }

  .muted { color: var(--muted); }
  .ellipsis { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }

  .toast {
    position: fixed;
    bottom: 18px;
    left: 50%;
    transform: translateX(-50%);
    max-width: 70vw;
    padding: 10px 16px;
    border-radius: var(--radius);
    background: var(--text);
    color: var(--panel);
    font-size: 13px;
    box-shadow: 0 6px 24px rgb(0 0 0 / 0.2);
  }
  .toast.error { background: #b42318; color: #fff; }
</style>
