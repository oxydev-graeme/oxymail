<script>
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { api, listen, isLive, quotableHtml, escapeHtml, accountLabel, DEFAULT_SETTINGS, shortDate, longDate, formatSize, messageDocument } from "./lib/api.js";
  import Logo from "./lib/Logo.svelte";
  import Icon from "./lib/Icon.svelte";
  import Settings from "./lib/Settings.svelte";
  import Avatar from "./lib/Avatar.svelte";
  import Compose from "./lib/Compose.svelte";

  const PAGE = 200;
  const appWindow = getCurrentWindow();

  let settings = $state({ ...DEFAULT_SETTINGS });
  let showSettings = $state(false);

  let accounts = $state([]);
  let accountId = $state(null);
  let folders = $state([]);
  let folder = $state(null);

  let account = $derived(accounts.find((a) => a.id === accountId) ?? null);
  let liveAccounts = $derived(accounts.filter(isLive));

  let composing = $state(null);
  /** account id → what sync is doing right now */
  let syncing = $state({});
  /** account id → last sync error */
  let syncErrors = $state({});
  let lastSync = $state(null);
  let syncText = $derived(Object.values(syncing)[0] ?? null);

  let list = $state([]);
  let listDone = $state(false);
  let loadingList = $state(false);
  let selectedId = $state(null);
  let view = $state(null);

  let query = $state("");
  let searching = $state(false);
  let searchInput;

  let importPath = $state("~/MailbirdExport/mail");
  let importing = $state(false);
  let toast = $state(null);
  let toastTimer;
  let error = $state("");

  let selected = $derived(list.find((m) => m.id === selectedId) ?? null);
  /** Role of the folder the selected message is in (search results can span folders). */
  let selectedRole = $derived(selected?.folder_role ?? null);
  let selectedLive = $derived(isLive(accounts.find((a) => a.id === selected?.account_id)));

  $effect(() => {
    const root = document.documentElement;
    if (settings.theme === "system") delete root.dataset.theme;
    else root.dataset.theme = settings.theme;
  });

  onMount(async () => {
    try {
      settings = { ...DEFAULT_SETTINGS, ...(await api.settings()) };
    } catch (e) {
      error = String(e);
    }
    await loadAccounts();

    listen("sync-status", ({ payload }) => {
      syncing[payload.account_id] = payload.text;
    });
    listen("sync-done", async ({ payload }) => {
      delete syncing[payload.account_id];
      lastSync = new Date();
      if (payload.error) {
        syncErrors[payload.account_id] = payload.error;
        return;
      }
      delete syncErrors[payload.account_id];
      accounts = await api.accounts();
      if (payload.account_id === accountId) {
        folders = await api.folders(accountId);
        if (!folder) {
          const inbox = folders.find((f) => f.role === "inbox") ?? folders[0];
          if (inbox) openFolder(inbox);
        } else if (!searching && payload.report?.new_messages + payload.report?.removed > 0) {
          refreshList();
        }
      }
    });
    listen("sync-error", ({ payload }) => {
      error = payload.error;
    });
  });

  /** Reloads the visible page of the folder without losing the selection. */
  async function refreshList() {
    if (!folder) return;
    const page = await api.messages(folder.id, 0, Math.max(list.length, PAGE));
    list = page;
    listDone = page.length < PAGE;
  }

  function syncNow() {
    api.syncNow().catch((e) => (error = String(e)));
  }

  // ---- Compose -------------------------------------------------------------

  const ownAddresses = () => new Set(liveAccounts.map((a) => a.name.toLowerCase()));
  const emailOf = (s) => (s.match(/<([^>]+)>/)?.[1] ?? s).trim();
  const prefixed = (prefix, subject = "") =>
    new RegExp(`^${prefix}:`, "i").test(subject.trim()) ? subject : `${prefix}: ${subject}`;
  /** The live account to send from: the message's own account if it is live, else the first. */
  const sendingAccount = (m) => (isLive(accounts.find((a) => a.id === m?.account_id)) ? m.account_id : liveAccounts[0]?.id);

  function originalHtml(v) {
    return v.html ? quotableHtml(v.html) : `<pre style="white-space:pre-wrap;font:inherit">${escapeHtml(v.text ?? "")}</pre>`;
  }

  function compose() {
    composing = { account_id: account && isLive(account) ? account.id : liveAccounts[0]?.id };
  }

  function reply(all = false) {
    if (!view) return;
    const own = ownAddresses();
    const primary = (view.reply_to.length ? view.reply_to : view.from).map(emailOf);
    const others = all ? [...view.to, ...view.cc].map(emailOf).filter((e) => !own.has(e.toLowerCase()) && !primary.includes(e)) : [];
    composing = {
      account_id: sendingAccount(selected),
      to: primary.join(", "),
      cc: others.join(", "),
      subject: prefixed("Re", view.subject ?? ""),
      in_reply_to: view.message_id,
      references: [...view.references, view.message_id].filter(Boolean),
      quote: `<div>On ${escapeHtml(longDate(view.date))}, ${escapeHtml(view.from.join(", "))} wrote:</div><blockquote>${originalHtml(view)}</blockquote>`,
    };
  }

  function forward() {
    if (!view) return;
    const head = [
      ["From", view.from.join(", ")],
      ["Date", longDate(view.date)],
      ["Subject", view.subject ?? ""],
      ["To", view.to.join(", ")],
    ]
      .map(([k, v]) => `<b>${k}:</b> ${escapeHtml(v)}`)
      .join("<br>");
    composing = {
      account_id: sendingAccount(selected),
      subject: prefixed("Fwd", view.subject ?? ""),
      quote: `<div>---------- Forwarded message ----------<br>${head}</div><br>${originalHtml(view)}`,
      note: view.attachments.length ? "The original attachments aren't forwarded yet; save and re-attach them." : "",
    };
  }

  async function loadAccounts() {
    try {
      accounts = await api.accounts();
      if (!accounts.some((a) => a.id === accountId)) {
        accountId = null;
        folders = [];
        folder = null;
        list = [];
        selectedId = null;
        view = null;
        if (accounts.length) await selectAccount(accounts[0].id);
      }
    } catch (e) {
      error = String(e);
    }
  }

  /** Shows an account's folders below the account list and opens its Inbox. */
  async function selectAccount(id) {
    accountId = id;
    folders = await api.folders(id);
    const inbox = folders.find((f) => f.role === "inbox") ?? folders[0];
    if (inbox) await openFolder(inbox);
    else {
      folder = null;
      list = [];
    }
  }

  /** Reloads unread counts after a change in `changedAccount`. */
  async function refreshFolders(changedAccount) {
    accounts = await api.accounts();
    if (changedAccount === accountId) {
      folders = await api.folders(accountId);
      if (folder) folder = folders.find((f) => f.id === folder.id) ?? folder;
    }
  }

  async function markFolderRead() {
    if (!folder) return;
    const ids = await api.markFolderRead(folder.id);
    afterMarkRead(ids, accountId, `Marked ${ids.length.toLocaleString()} as read in ${folder.name}`);
  }

  async function markAccountRead(a) {
    const ids = await api.markAccountRead(a.id);
    afterMarkRead(ids, a.id, `Marked ${ids.length.toLocaleString()} as read in ${a.label || a.name}`);
  }

  function afterMarkRead(ids, changedAccount, label) {
    const changed = new Set(ids);
    for (const m of list) if (changed.has(m.id)) m.unread = false;
    refreshFolders(changedAccount);
    if (!ids.length) return;
    flash(label, async () => {
      await act(() => api.setFlag(ids, "S", false));
      for (const m of list) if (changed.has(m.id)) m.unread = true;
      refreshFolders(changedAccount);
    });
  }

  async function openFolder(f) {
    folder = f;
    query = "";
    searching = false;
    list = [];
    listDone = false;
    selectedId = null;
    view = null;
    await loadMore();
  }

  async function loadMore() {
    if (loadingList || listDone || searching || !folder) return;
    loadingList = true;
    try {
      const page = await api.messages(folder.id, list.length, PAGE);
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
    if (!query.trim()) return clearSearch();
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
    if (folder) openFolder(folder);
  }

  async function select(id) {
    selectedId = id;
    try {
      view = await api.message(id);
      const m = list.find((x) => x.id === id);
      if (m?.unread && settings.mark_read_on_open === "true") {
        await api.setFlag([id], "S", true);
        m.unread = false;
        refreshFolders(m.account_id);
      }
    } catch (e) {
      view = null;
      error = String(e);
    }
  }

  // ---- Actions -------------------------------------------------------------

  async function toggleStar(m = selected) {
    if (!m) return;
    await act(() => api.setFlag([m.id], "F", !m.starred));
    m.starred = !m.starred;
  }

  async function toggleRead(m = selected) {
    if (!m) return;
    await act(() => api.setFlag([m.id], "S", m.unread));
    m.unread = !m.unread;
    refreshFolders(m.account_id);
  }

  /** Runs a move-style action, takes the message out of the list and offers Undo. */
  async function moveAway(m, action, label) {
    if (!m) return;
    const from = m.folder_id;
    const ok = await act(action);
    if (!ok) return;
    dropFromList(m.id);
    refreshFolders(m.account_id);
    flash(label, async () => {
      await act(() => api.moveToFolder([m.id], from));
      refreshFolders(m.account_id);
      if (folder?.id === from && !searching) openFolder(folder);
    });
  }

  const archive = (m = selected) => moveAway(m, () => api.moveTo([m.id], "archive"), "Archived");
  const moveToInbox = (m = selected) => moveAway(m, () => api.moveTo([m.id], "inbox"), "Moved to Inbox");
  const restore = (m = selected) => moveAway(m, () => api.restore([m.id]), "Restored");

  function remove(m = selected) {
    if (!m) return;
    if (m.folder_role === "trash") {
      const ok = confirm(
        "Remove this message from OxyMail?\n\nIt disappears from every folder and search. " +
          "For archive accounts the original file in your export is kept.",
      );
      if (!ok) return;
      return moveAway(m, () => api.remove([m.id]), "Removed from OxyMail");
    }
    return moveAway(m, () => api.remove([m.id]), "Moved to Trash");
  }

  async function act(fn) {
    try {
      await fn();
      return true;
    } catch (e) {
      error = String(e);
      return false;
    }
  }

  function dropFromList(id) {
    const i = list.findIndex((m) => m.id === id);
    if (i < 0) return;
    list = list.filter((m) => m.id !== id);
    if (selectedId === id) {
      const next = list[i] ?? list[i - 1];
      if (next) select(next.id);
      else {
        selectedId = null;
        view = null;
      }
    }
  }

  // ---- Settings, keys, toasts ---------------------------------------------

  async function changeSetting(key, value) {
    settings[key] = value;
    await act(() => api.setSetting(key, value));
  }

  function onListScroll(event) {
    const el = event.currentTarget;
    if (el.scrollTop + el.clientHeight > el.scrollHeight - 600) loadMore();
  }

  function onKey(event) {
    if (showSettings || composing) return;
    if (event.target instanceof HTMLInputElement) {
      if (event.key === "Escape") {
        event.target.blur();
        if (searching) clearSearch();
      }
      return;
    }
    if (event.ctrlKey || event.metaKey || event.altKey) return;
    const i = list.findIndex((m) => m.id === selectedId);
    const keys = {
      ArrowDown: () => list[Math.min(i + 1, list.length - 1)] && select(list[Math.min(i + 1, list.length - 1)].id),
      j: () => keys.ArrowDown(),
      ArrowUp: () => list[Math.max(i - 1, 0)] && select(list[Math.max(i - 1, 0)].id),
      k: () => keys.ArrowUp(),
      e: () => !selectedLive && (selectedRole === "archive" ? moveToInbox() : archive()),
      Delete: () => !selectedLive && remove(),
      "#": () => !selectedLive && remove(),
      s: () => toggleStar(),
      u: () => toggleRead(),
      "/": () => searchInput?.focus(),
      c: () => compose(),
      r: () => reply(false),
      a: () => reply(true),
      f: () => forward(),
      Escape: () => searching && clearSearch(),
    };
    const fn = keys[event.key];
    if (fn) {
      event.preventDefault();
      fn();
    }
  }

  function flash(message, undo = null) {
    clearTimeout(toastTimer);
    toast = { message, undo };
    toastTimer = setTimeout(() => (toast = null), 6000);
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

  const sender = (m) => m.from_name || m.from_addr || "(no sender)";
  const folderIcon = (role) =>
    ({ inbox: "inbox", sent: "send", drafts: "draft", archive: "archive", spam: "spam", trash: "trash" })[role] ?? "folder";
  const showUnread = (f) => f.unread > 0 && !["sent", "trash", "spam", "drafts"].includes(f.role);
</script>

<svelte:window onkeydown={onKey} />

<div class="app" class:compact={settings.density === "compact"}>
  <header class="titlebar" data-tauri-drag-region>
    <div class="brand" data-tauri-drag-region>
      <Logo size={24} />
      <span class="wordmark"><b>Oxy</b>Mail</span>
    </div>
    <form class="search" onsubmit={runSearch}>
      <Icon name="search" size={15} />
      <input bind:this={searchInput} type="search" placeholder="Search all mail" bind:value={query} />
      {#if searching}
        <button type="button" class="clear" onclick={clearSearch} title="Clear search"><Icon name="x" size={14} /></button>
      {/if}
    </form>
    <div class="spacer" data-tauri-drag-region></div>
    <div class="win">
      <button class="compose-btn" onclick={compose} title="New message (c)"><Icon name="pen" size={15} /><span>New</span></button>
      {#if liveAccounts.length}
        <button onclick={syncNow} class:spinning={syncText} title={syncText ? `Syncing ${syncText}` : "Check for new mail"}>
          <Icon name="refresh" size={16} />
        </button>
      {/if}
      <button onclick={() => (showSettings = true)} title="Settings"><Icon name="settings" size={17} /></button>
      <span class="divider"></span>
      <button onclick={() => appWindow.minimize()} title="Minimise"><Icon name="minimize" size={15} /></button>
      <button onclick={() => appWindow.toggleMaximize()} title="Maximise"><Icon name="maximize" size={13} /></button>
      <button class="close" onclick={() => appWindow.close()} title="Close"><Icon name="close" size={15} /></button>
    </div>
  </header>

  <div class="shell">
    <aside class="sidebar">
      <div class="section">Accounts</div>
      <nav class="accounts">
        {#each accounts as a (a.id)}
          <div class="account" class:active={a.id === accountId}>
            <button class="account-main" onclick={() => selectAccount(a.id)} title={a.name}>
              <Avatar account={a} size={24} />
              <span class="ellipsis">{accountLabel(a)}</span>
              {#if syncErrors[a.id]}<span class="sync-error" title={syncErrors[a.id]}>!</span>{/if}
              {#if a.unread > 0}<span class="count">{a.unread.toLocaleString()}</span>{/if}
            </button>
            {#if a.unread > 0}
              <button class="account-read" onclick={() => markAccountRead(a)} title="Mark all as read in this account">
                <Icon name="checks" size={14} />
              </button>
            {/if}
          </div>
        {/each}
      </nav>
      {#if account}
        <div class="section folders-head">
          <span class="ellipsis">Folders · {accountLabel(account)}</span>
        </div>
        <nav class="folders">
          {#each folders as f (f.id)}
            <button class="folder" class:active={f.id === folder?.id && !searching} onclick={() => openFolder(f)}>
              <Icon name={folderIcon(f.role)} size={15} />
              <span class="ellipsis">{f.name}</span>
              {#if showUnread(f)}<span class="count">{f.unread.toLocaleString()}</span>{/if}
            </button>
          {/each}
        </nav>
      {/if}
      {#if liveAccounts.length}
        <div class="status" title={syncText ?? ""}>
          {#if syncText}
            <span class="pulse"></span><span class="ellipsis">Syncing {syncText}</span>
          {:else if account && syncErrors[account.id]}
            <span class="ellipsis bad">{syncErrors[account.id]}</span>
          {:else if lastSync}
            <span class="ellipsis">Up to date · {lastSync.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })}</span>
          {/if}
        </div>
      {/if}
    </aside>

    <section class="list-pane">
      <div class="list-title">
        <span class="ellipsis">{searching ? `Results for “${query}”` : (folder?.name ?? "")}</span>
        {#if searching && !loadingList}<span class="muted">{list.length}</span>{/if}
        {#if !searching && folder?.unread > 0}
          <button class="mark-read" onclick={markFolderRead} title="Mark every message in this folder as read">
            <Icon name="checks" size={14} /> Mark all read
          </button>
        {/if}
      </div>
      <div class="list" onscroll={onListScroll}>
        {#each list as m (m.id)}
          <div class="row" class:selected={m.id === selectedId} class:unread={m.unread}>
            <button class="row-main" onclick={() => select(m.id)}>
              <div class="row-top">
                <span class="ellipsis sender">{sender(m)}</span>
                <span class="date">{shortDate(m.date)}</span>
              </div>
              <div class="row-subject ellipsis">
                {#if m.attachments > 0}<span class="clip"><Icon name="clip" size={12} /></span>{/if}
                {m.subject || "(no subject)"}
              </div>
              {#if settings.density !== "compact"}
                <div class="row-snippet ellipsis">{m.snippet}</div>
              {/if}
              {#if searching}<div class="row-folder">{m.folder_name}</div>{/if}
            </button>
            <div class="row-actions">
              <button class:starred={m.starred} onclick={() => toggleStar(m)} title={m.starred ? "Unstar" : "Star"}>
                <Icon name="star" size={15} />
              </button>
              {#if !isLive(accounts.find((a) => a.id === m.account_id))}
                {#if m.folder_role !== "archive" && m.folder_role !== "trash"}
                  <button onclick={() => archive(m)} title="Archive"><Icon name="archive" size={15} /></button>
                {/if}
                <button onclick={() => remove(m)} title={m.folder_role === "trash" ? "Remove from OxyMail" : "Delete"}>
                  <Icon name="trash" size={15} />
                </button>
              {/if}
            </div>
            {#if m.starred}<span class="star-pin"><Icon name="star" size={12} /></span>{/if}
          </div>
        {:else}
          {#if !loadingList && accounts.length}
            <div class="empty">{searching ? "No messages match." : "Nothing here."}</div>
          {/if}
        {/each}
        {#if loadingList}<div class="empty">Loading…</div>{/if}
      </div>
    </section>

    <main class="reader">
      {#if !accounts.length}
        <div class="welcome">
          <Logo size={64} />
          <h1>Welcome to OxyMail</h1>
          <p>Email with room to breathe. Import a Maildir archive, such as your Mailbird export, to begin.</p>
          <div class="import">
            <input bind:value={importPath} />
            <button class="primary" onclick={runImport} disabled={importing}>{importing ? "Importing…" : "Import"}</button>
          </div>
        </div>
      {:else if view && selected}
        <div class="toolbar">
          <button onclick={() => reply(false)} title="Reply (r)"><Icon name="reply" size={16} /><span>Reply</span></button>
          <button onclick={() => reply(true)} title="Reply all (a)"><Icon name="replyAll" size={16} /><span>Reply all</span></button>
          <button onclick={forward} title="Forward (f)"><Icon name="forward" size={16} /><span>Forward</span></button>
          <span class="sep"></span>
          {#if selectedLive}
            <!-- Moving and deleting live mail arrives with two-way sync. -->
          {:else if selectedRole === "trash"}
            <button onclick={() => restore()}><Icon name="restore" size={16} /><span>Restore</span></button>
            <button class="danger" onclick={() => remove()}><Icon name="trash" size={16} /><span>Remove from OxyMail</span></button>
          {:else}
            {#if selectedRole === "archive"}
              <button onclick={() => moveToInbox()} title="Move to Inbox (e)"><Icon name="inbox" size={16} /><span>Move to Inbox</span></button>
            {:else}
              <button onclick={() => archive()} title="Archive (e)"><Icon name="archive" size={16} /><span>Archive</span></button>
            {/if}
            <button onclick={() => remove()} title="Delete (Del)"><Icon name="trash" size={16} /><span>Delete</span></button>
          {/if}
          <span class="sep"></span>
          <button class:starred={selected.starred} onclick={() => toggleStar()} title="Star (s)">
            <Icon name="star" size={16} /><span>{selected.starred ? "Starred" : "Star"}</span>
          </button>
          <button onclick={() => toggleRead()} title="Mark read or unread (u)">
            <Icon name={selected.unread ? "open" : "mail"} size={16} /><span>{selected.unread ? "Mark read" : "Mark unread"}</span>
          </button>
        </div>
        <header class="msg-head">
          <h1>{view.subject || "(no subject)"}</h1>
          <div class="meta">
            <div><span class="label">From</span><span class="value">{view.from.join(", ")}</span></div>
            {#if view.to.length}<div><span class="label">To</span><span class="value">{view.to.join(", ")}</span></div>{/if}
            {#if view.cc.length}<div><span class="label">Cc</span><span class="value">{view.cc.join(", ")}</span></div>{/if}
            <div class="muted">{longDate(view.date)}</div>
          </div>
          {#each view.notices as notice}<div class="notice">{notice}</div>{/each}
          {#if view.attachments.length}
            <div class="attachments">
              {#each view.attachments as a (a.index)}
                <button class="chip" onclick={() => act(async () => flash(`Saved to ${await api.saveAttachment(view.id, a.index)}`))} title="Save to Downloads">
                  <Icon name="clip" size={13} />
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
        <div class="placeholder">
          <Logo size={40} />
          <p>Select a message to read it.</p>
        </div>
      {/if}
    </main>
  </div>
</div>

{#if showSettings}
  <Settings
    {settings}
    {accounts}
    onchange={changeSetting}
    onclose={() => (showSettings = false)}
    onimported={loadAccounts}
    onaccountschanged={loadAccounts}
  />
{/if}

{#if composing}
  <Compose
    draft={composing}
    accounts={liveAccounts}
    onclose={() => (composing = null)}
    onsent={(note) => { composing = null; flash(note); }}
  />
{/if}

{#if toast}
  <div class="toast">
    <span>{toast.message}</span>
    {#if toast.undo}
      <button onclick={() => { const u = toast.undo; toast = null; u(); }}>Undo</button>
    {/if}
  </div>
{/if}
{#if error}
  <button class="toast error" onclick={() => (error = "")}>{error} — dismiss</button>
{/if}

<style>
  /* minmax(0, …) and min-height: 0 stop grid rows growing to fit their content, so each pane
     gets a fixed height and its own scroll area actually scrolls. */
  .app { display: grid; grid-template-rows: 46px minmax(0, 1fr); height: 100%; overflow: hidden; }

  /* ---- Header bar ---- */
  .titlebar {
    display: grid;
    grid-template-columns: 250px 380px 1fr auto;
    align-items: center;
    background: var(--header);
    border-bottom: 1px solid var(--border);
  }
  .brand { display: flex; align-items: center; gap: 9px; padding-left: 16px; height: 100%; }
  .brand :global(*) { pointer-events: none; }
  .wordmark { font-size: 16px; letter-spacing: -0.03em; font-weight: 450; }
  .wordmark b { font-weight: 800; color: var(--brand); }
  .search {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 12px;
    padding: 0 10px;
    height: 32px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--bg);
    color: var(--muted);
  }
  .search:focus-within { border-color: var(--accent); color: var(--text); }
  .search input { flex: 1; min-width: 0; border: 0; background: none; outline: none; }
  .search input::-webkit-search-cancel-button { display: none; }
  .clear { display: grid; place-items: center; color: var(--muted); }
  .spacer { height: 100%; }
  .win { display: flex; align-items: center; gap: 2px; padding-right: 8px; }
  .win button { display: grid; place-items: center; width: 32px; height: 30px; border-radius: 6px; color: var(--muted); }
  .win button:hover { background: var(--hover); color: var(--text); }
  .win .close:hover { background: var(--danger); color: #fff; }
  .compose-btn { width: auto !important; gap: 6px; padding: 0 12px; margin-right: 6px; display: flex !important; background: var(--accent); color: #fff !important; font-weight: 600; font-size: 13px; }
  .compose-btn:hover { background: var(--brand) !important; }
  .spinning :global(svg) { animation: spin 1s linear infinite; color: var(--accent); }
  @keyframes spin { to { transform: rotate(360deg); } }
  .status { position: sticky; bottom: -12px; display: flex; align-items: center; gap: 8px; margin-top: 12px; padding: 8px 16px; font-size: 12px; color: var(--muted); background: var(--sidebar); border-top: 1px solid var(--border); }
  .status .bad { color: var(--danger); }
  .pulse { width: 7px; height: 7px; border-radius: 50%; background: var(--accent); flex-shrink: 0; animation: pulse 1.2s ease-in-out infinite; }
  @keyframes pulse { 50% { opacity: 0.3; } }
  .sync-error { display: grid; place-items: center; width: 16px; height: 16px; border-radius: 50%; background: var(--danger); color: #fff; font-size: 11px; font-weight: 800; flex-shrink: 0; }
  .divider { width: 1px; height: 18px; margin: 0 6px; background: var(--border); }

  .shell { display: grid; grid-template-columns: 250px 380px minmax(0, 1fr); grid-template-rows: minmax(0, 1fr); min-height: 0; }

  /* ---- Sidebar ---- */
  .sidebar { overflow-y: auto; background: var(--sidebar); border-right: 1px solid var(--border); min-width: 0; min-height: 0; padding-bottom: 12px; }
  .section { padding: 12px 16px 6px; font-size: 11px; font-weight: 700; letter-spacing: 0.06em; text-transform: uppercase; color: var(--faint); }
  .folders-head { border-top: 1px solid var(--border); margin-top: 6px; padding-top: 12px; display: flex; }
  .accounts, .folders { padding: 0 8px; }
  .account { position: relative; display: flex; align-items: center; border-radius: 7px; }
  .account:hover { background: var(--hover); }
  .account.active { background: var(--selected); }
  .account-main { display: flex; align-items: center; gap: 9px; flex: 1; min-width: 0; padding: 5px 8px; text-align: left; font-size: 13px; }
  .account.active .account-main { font-weight: 600; }
  .account-read { display: none; place-items: center; width: 26px; height: 26px; margin-right: 4px; border-radius: 6px; color: var(--muted); }
  .account:hover .account-read { display: grid; }
  .account:hover .account-main .count { display: none; }
  .account-read:hover { background: var(--panel); color: var(--accent); }
  .folder { display: flex; align-items: center; gap: 9px; width: 100%; padding: 6px 8px 6px 10px; border-radius: 6px; font-size: 13px; text-align: left; color: var(--muted); }
  .folder .ellipsis { flex: 1; }
  .folder:hover { background: var(--hover); color: var(--text); }
  .folder.active { background: var(--selected); color: var(--text); font-weight: 600; }
  .count { margin-left: auto; font-size: 11px; font-weight: 700; padding: 0 7px; border-radius: 99px; background: var(--accent); color: #fff; line-height: 18px; }

  /* ---- Message list ---- */
  .list-pane { display: flex; flex-direction: column; background: var(--panel); border-right: 1px solid var(--border); min-width: 0; min-height: 0; }
  .mark-read { display: flex; align-items: center; gap: 5px; margin-left: auto; padding: 2px 8px; border-radius: 6px; font-size: 12px; font-weight: 500; color: var(--accent); }
  .mark-read:hover { background: var(--accent-soft); }
  .list-title { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 12px 16px 10px; font-weight: 700; font-size: 14px; border-bottom: 1px solid var(--border); }
  .list { flex: 1; min-height: 0; overflow-y: auto; }
  .row { position: relative; border-bottom: 1px solid var(--border); border-left: 3px solid transparent; }
  .row:hover { background: var(--bg); }
  .row.selected { background: var(--selected); }
  .row.unread { border-left-color: var(--accent); }
  .row.unread .sender, .row.unread .row-subject { font-weight: 700; }
  .row-main { display: block; width: 100%; padding: 10px 16px 10px 13px; text-align: left; }
  .compact .row-main { padding: 6px 16px 6px 13px; }
  .row-top { display: flex; justify-content: space-between; gap: 10px; }
  .sender { font-size: 13px; }
  .date { flex-shrink: 0; font-size: 12px; color: var(--muted); }
  .row-subject { display: flex; align-items: center; gap: 4px; font-size: 13px; margin-top: 1px; }
  .row-snippet { font-size: 12px; color: var(--muted); margin-top: 2px; }
  .row-folder { display: inline-block; margin-top: 4px; padding: 0 7px; border-radius: 99px; font-size: 11px; background: var(--accent-soft); color: var(--accent); }
  .clip { display: inline-flex; color: var(--muted); }
  .row-actions {
    position: absolute;
    right: 8px;
    bottom: 6px;
    display: none;
    gap: 2px;
    padding: 2px;
    border-radius: 7px;
    background: var(--panel);
    border: 1px solid var(--border);
    box-shadow: 0 2px 6px rgb(0 0 0 / 0.08);
  }
  .row:hover .row-actions { display: flex; }
  .row-actions button { display: grid; place-items: center; width: 28px; height: 26px; border-radius: 5px; color: var(--muted); }
  .row-actions button:hover { background: var(--hover); color: var(--text); }
  .row-actions button.starred, .toolbar button.starred { color: var(--star); }
  .row-actions button.starred :global(path), .toolbar button.starred :global(path), .star-pin :global(path) { fill: currentColor; }
  .star-pin { position: absolute; right: 12px; bottom: 9px; color: var(--star); display: flex; }
  .row:hover .star-pin { display: none; }

  /* ---- Reader ---- */
  .reader { display: flex; flex-direction: column; min-width: 0; min-height: 0; background: var(--panel); }
  .toolbar { display: flex; align-items: center; gap: 2px; padding: 8px 14px; border-bottom: 1px solid var(--border); }
  .toolbar button { display: flex; align-items: center; gap: 6px; padding: 6px 10px; border-radius: 6px; font-size: 13px; color: var(--muted); }
  .toolbar button:hover { background: var(--hover); color: var(--text); }
  .toolbar button.danger:hover { color: var(--danger); }
  .sep { width: 1px; height: 18px; margin: 0 6px; background: var(--border); }
  .msg-head { padding: 18px 24px 14px; border-bottom: 1px solid var(--border); user-select: text; }
  .msg-head h1 { margin: 0 0 10px; font-size: 19px; line-height: 1.3; font-weight: 650; }
  .meta { font-size: 13px; display: grid; gap: 2px; }
  .label { display: inline-block; width: 42px; color: var(--muted); }
  .notice { margin-top: 10px; padding: 8px 10px; border-radius: 6px; background: var(--warn-bg); color: var(--warn-text); font-size: 13px; }
  .attachments { display: flex; flex-wrap: wrap; gap: 8px; margin-top: 12px; }
  .chip { display: flex; align-items: center; gap: 7px; max-width: 300px; padding: 6px 10px; border: 1px solid var(--border); border-radius: var(--radius); font-size: 12px; }
  .chip:hover { border-color: var(--accent); background: var(--accent-soft); }
  .body-frame { flex: 1; min-height: 0; width: 100%; border: 0; background: #fff; }
  .body-text { flex: 1; min-height: 0; margin: 0; padding: 20px 24px; overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; font: 14px/1.5 ui-monospace, "Ubuntu Mono", monospace; user-select: text; }

  .placeholder { margin: auto; display: grid; justify-items: center; gap: 4px; color: var(--muted); opacity: 0.8; }
  .empty { padding: 24px; color: var(--muted); text-align: center; }
  .welcome { margin: auto; max-width: 480px; padding: 24px; display: grid; justify-items: start; }
  .welcome h1 { font-size: 22px; margin: 14px 0 6px; }
  .welcome p { color: var(--muted); margin: 0 0 16px; }
  .import { display: flex; gap: 8px; width: 100%; }
  .import input { flex: 1; padding: 8px 10px; border: 1px solid var(--border); border-radius: var(--radius); background: var(--bg); }
  .primary { padding: 8px 16px; border-radius: var(--radius); background: var(--accent); color: #fff; font-weight: 600; }
  .primary:disabled { opacity: 0.6; cursor: default; }

  .muted { color: var(--muted); font-weight: 400; }
  .ellipsis { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }

  .toast {
    position: fixed;
    bottom: 18px;
    left: 50%;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: 16px;
    max-width: 70vw;
    padding: 10px 16px;
    border-radius: var(--radius);
    background: var(--text);
    color: var(--panel);
    font-size: 13px;
    box-shadow: 0 6px 24px rgb(0 0 0 / 0.2);
    z-index: 30;
  }
  .toast button { color: var(--amber); font-weight: 700; }
  .toast.error { background: var(--danger); color: #fff; }
</style>
