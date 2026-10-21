<script>
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { api, listen, isLive, quotableHtml, escapeHtml, parseMailto, accountLabel, DEFAULT_SETTINGS, shortDate, longDate, formatSize, messageDocument } from "./lib/api.js";
  import Logo from "./lib/Logo.svelte";
  import Icon from "./lib/Icon.svelte";
  import Settings from "./lib/Settings.svelte";
  import Avatar from "./lib/Avatar.svelte";
  import Compose from "./lib/Compose.svelte";
  import MovePicker from "./lib/MovePicker.svelte";
  import TimeMenu from "./lib/TimeMenu.svelte";

  const PAGE = 200;
  const appWindow = getCurrentWindow();

  let settings = $state({ ...DEFAULT_SETTINGS });
  let showSettings = $state(false);

  let accounts = $state([]);
  let accountId = $state(null);
  let folders = $state([]);
  let folder = $state(null);

  let account = $derived(accounts.find((a) => a.id === accountId) ?? null);
  let allInboxUnread = $derived(accounts.reduce((n, a) => n + a.unread, 0));
  let drafts = $state([]);
  let snoozedList = $state([]);
  let scheduledList = $state([]);
  /** Messages waiting for a snooze time from the menu. */
  let snoozing = $state(null);

  async function refreshQueues() {
    snoozedList = await api.snoozed().catch(() => []);
    scheduledList = await api.scheduled().catch(() => []);
    if (folder?.snoozedView || folder?.scheduledView) refreshList();
  }

  const scheduledRows = () =>
    scheduledList.map((x) => ({
      id: `sched-${x.id}`,
      scheduled: x,
      date: x.send_at,
      from_name: x.summary?.split(" — ")[0] ? `To: ${x.summary.split(" — ")[0]}` : "(no recipients)",
      subject: x.summary?.split(" — ").slice(1).join(" — ") || "(no subject)",
      snippet: x.error ? `Not sent: ${x.error}` : `Sends ${new Date(x.send_at * 1000).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" })}`,
      unread: Boolean(x.error),
      starred: false,
      attachments: 0,
    }));

  function openQueue(kind) {
    accountId = null;
    folders = [];
    folder = kind === "snoozed" ? { id: "snoozed", snoozedView: true, name: "Snoozed" } : { id: "scheduled", scheduledView: true, name: "Scheduled" };
    query = "";
    searching = false;
    selectedId = null;
    selectedIds = [];
    view = null;
    list = [];
    listDone = false;
    loadMore();
  }

  const when = (ts) => new Date(ts * 1000).toLocaleString(undefined, { weekday: "short", day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });

  function openSnooze() {
    const msgs = multi ? selectedMsgs : selected ? [selected] : [];
    if (msgs.length) snoozing = msgs;
  }

  async function doSnooze(at) {
    const msgs = snoozing;
    snoozing = null;
    const ids = msgs.map((m) => m.id);
    for (const id of ids) dropFromList(id);
    if (!(await act(() => api.snooze(ids, at)))) return refreshList();
    new Set(msgs.map((m) => m.account_id)).forEach(refreshFolders);
    refreshQueues();
    flash(`Snoozed until ${when(at)}`, async () => {
      await act(() => api.snooze(ids, null));
      new Set(msgs.map((m) => m.account_id)).forEach(refreshFolders);
      refreshQueues();
      if (folder && !searching) refreshList();
    });
  }

  async function unsnooze(m = selected) {
    if (!m) return;
    await act(() => api.snooze([m.id], null));
    m.snoozed_until = null;
    refreshQueues();
    refreshFolders(m.account_id);
    flash("Back in its folder");
  }

  async function scheduleFromCompose(accountId, mail, reopen, at) {
    try {
      await api.scheduleSend(accountId, at, mail, JSON.stringify(reopen));
      if (reopen.draft_id) await api.deleteDraft(reopen.draft_id).catch(() => {});
      composing = null;
      refreshDrafts();
      refreshQueues();
      flash(`Scheduled for ${when(at)}`);
    } catch (e) {
      error = String(e);
    }
  }

  async function editScheduled(x) {
    if (!confirm("Stop this scheduled message and open it for editing?")) return;
    try {
      const draft = await api.unschedule(x.id);
      composing = JSON.parse(draft);
      refreshQueues();
    } catch (e) {
      error = String(e);
    }
  }

  async function refreshDrafts() {
    drafts = await api.drafts().catch(() => []);
    if (folder?.localDrafts) list = draftRows();
  }

  const draftRows = () =>
    drafts.map((d) => ({
      id: `draft-${d.id}`,
      draft: d,
      date: d.updated,
      from_name: d.recipients ? `To: ${d.recipients}` : "(no recipients)",
      subject: d.subject || "(no subject)",
      snippet: "",
      unread: false,
      starred: false,
      attachments: 0,
    }));

  function openDrafts() {
    accountId = null;
    folders = [];
    folder = { id: "drafts", localDrafts: true, name: "Unsent drafts" };
    query = "";
    searching = false;
    selectedId = null;
    selectedIds = [];
    view = null;
    list = draftRows();
    listDone = true;
  }

  function openAllInboxes() {
    accountId = null;
    folders = [];
    openFolder({ id: "all-inboxes", unified: true, name: "All inboxes", role: "inbox", unread: 0 });
  }
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
  /** Every selected message id (Ctrl/Shift-click); one entry for an ordinary selection. */
  let selectedIds = $state([]);
  let anchorId = null;
  let multi = $derived(selectedIds.length > 1);
  let selectedMsgs = $derived(list.filter((m) => selectedIds.includes(m.id)));
  let moving = $state(null);
  let remoteAllowed = $state(false);
  /** Other messages in the open message's conversation. */
  let conversation = $state([]);
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
    refreshDrafts();
    refreshQueues();
    listen("scheduled-changed", refreshQueues);

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
    listen("mail-changed", async ({ payload }) => {
      refreshQueues();
      accounts = await api.accounts();
      if (payload.account_id === accountId) {
        folders = await api.folders(accountId);
        if (folder) folder = folders.find((f) => f.id === folder.id) ?? folder;
        if (!searching) refreshList();
      }
    });
    listen("compose-mailto", ({ payload }) => {
      const m = parseMailto(payload);
      composing = { account_id: sendingAccount(selected), to: m.to, cc: m.cc, subject: m.subject, quote: m.body ? escapeHtml(m.body) : undefined };
    });
    listen("sync-error", ({ payload }) => {
      error = payload.error;
    });
  });

  /** Reloads the visible page of the folder without losing the selection. */
  async function refreshList() {
    if (!folder) return;
    const page = await fetchPage(0, Math.max(list.length, PAGE));
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
      forward_from: view.id,
      forwarded: view.attachments.map(({ index, name, size }) => ({ index, name, size })),
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

  /** A page of the current folder, or of every Inbox for the "All inboxes" view. */
  const fetchPage = (offset, limit) =>
    folder.snoozedView
      ? offset ? Promise.resolve([]) : api.snoozed()
      : folder.scheduledView
      ? Promise.resolve(offset ? [] : scheduledRows())
      : folder.localDrafts
      ? Promise.resolve(offset ? [] : draftRows())
      : folder.unified ? api.messagesByRole("inbox", offset, limit) : api.messages(folder.id, offset, limit);

  async function loadMore() {
    if (loadingList || listDone || searching || !folder) return;
    loadingList = true;
    try {
      const page = await fetchPage(list.length, PAGE);
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
    selectedIds = [id];
    anchorId = id;
    remoteAllowed = false;
    conversation = [];
    try {
      view = await api.message(id);
      api.thread(id).then((t) => (conversation = selectedId === id ? t : conversation)).catch(() => (conversation = []));
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
  async function moveAway(m, action, label, undoable = true) {
    if (!m) return;
    const from = m.folder_id;
    // Take it out of the list straight away; server moves can take a moment.
    dropFromList(m.id);
    const ok = await act(action);
    if (!ok) {
      if (!searching && folder) refreshList();
      return;
    }
    refreshFolders(m.account_id);
    if (!undoable) return flash(label);
    flash(label, async () => {
      await act(() => api.moveToFolder([m.id], from));
      refreshFolders(m.account_id);
      if (folder?.id === from && !searching) openFolder(folder);
    });
  }

  const archive = (m = null) => (m && !selectedIds.includes(m.id) ? moveAway(m, () => api.moveTo([m.id], "archive"), "Archived") : bulkMove((ids) => api.moveTo(ids, "archive"), "Archived"));
  const moveToInbox = (m = null) => (m && !selectedIds.includes(m.id) ? moveAway(m, () => api.moveTo([m.id], "inbox"), "Moved to Inbox") : bulkMove((ids) => api.moveTo(ids, "inbox"), "Moved to Inbox"));
  const restore = (m = null) => (m && !selectedIds.includes(m.id) ? moveAway(m, () => api.restore([m.id]), "Restored") : bulkMove((ids) => api.restore(ids), "Restored"));

  function remove(m = selected) {
    if (multi && (!m || selectedIds.includes(m.id))) {
      const inTrash = selectedMsgs.every((x) => x.folder_role === "trash");
      if (inTrash && !confirm(`Delete ${selectedMsgs.length} messages forever?\n\nLive mail is removed from the server; archive copies on disk are kept.`)) return;
      return bulkMove((ids) => api.remove(ids), inTrash ? "Deleted" : "Moved to Trash:", !inTrash);
    }
    if (!m) return;
    if (m.folder_role === "trash") {
      const live = isLive(accounts.find((a) => a.id === m.account_id));
      const ok = confirm(
        live
          ? "Delete this message forever?\n\nIt is removed from the mail server and can't be undone. " +
              "A copy in your Mailbird archive, if any, is kept on disk."
          : "Remove this message from OxyMail?\n\nIt disappears from every folder and search. " +
              "For archive accounts the original file in your export is kept.",
      );
      if (!ok) return;
      return moveAway(m, () => api.remove([m.id]), live ? "Deleted forever" : "Removed from OxyMail", false);
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

  /** Ctrl/Cmd-click toggles a message in the selection; Shift-click selects a range. */
  function rowClick(event, m) {
    if (m.draft) return (composing = JSON.parse(m.draft.body));
    if (m.scheduled) return editScheduled(m.scheduled);
    if (event.ctrlKey || event.metaKey) {
      selectedIds = selectedIds.includes(m.id) ? selectedIds.filter((x) => x !== m.id) : [...selectedIds, m.id];
      anchorId = m.id;
      if (selectedIds.length === 1) select(selectedIds[0]);
    } else if (event.shiftKey && anchorId !== null) {
      const a = list.findIndex((x) => x.id === anchorId);
      const b = list.findIndex((x) => x.id === m.id);
      const [from, to] = a < b ? [a, b] : [b, a];
      selectedIds = list.slice(from, to + 1).map((x) => x.id);
    } else {
      select(m.id);
    }
  }

  // ---- Bulk actions (also used for a single message) ------------------------

  /** Moves the selected messages away (archive, delete, folder…) with an Undo that puts each back. */
  async function bulkMove(action, label, undoable = true, only = null) {
    const msgs = only ?? (multi ? selectedMsgs : selected ? [selected] : []);
    if (!msgs.length) return;
    const ids = msgs.map((m) => m.id);
    const origins = Object.groupBy ? Object.groupBy(msgs, (m) => m.folder_id) : groupBy(msgs, (m) => m.folder_id);
    const accountsTouched = [...new Set(msgs.map((m) => m.account_id))];
    for (const id of ids) dropFromList(id);
    selectedIds = selectedId ? [selectedId] : [];
    const ok = await act(() => action(ids));
    accountsTouched.forEach(refreshFolders);
    if (!ok) return !searching && folder && refreshList();
    const text = `${label}${ids.length > 1 ? ` ${ids.length} messages` : ""}`;
    if (!undoable) return flash(text);
    flash(text, async () => {
      for (const [folderId, group] of Object.entries(origins)) {
        await act(() => api.moveToFolder(group.map((m) => m.id), Number(folderId)));
      }
      accountsTouched.forEach(refreshFolders);
      if (!searching && folder) refreshList();
    });
  }

  function groupBy(items, key) {
    return items.reduce((out, x) => ((out[key(x)] ??= []).push(x), out), {});
  }

  async function bulkFlag(flag, on) {
    const msgs = multi ? selectedMsgs : selected ? [selected] : [];
    if (!msgs.length) return;
    if (!(await act(() => api.setFlag(msgs.map((m) => m.id), flag, on)))) return;
    for (const m of msgs) flag === "S" ? (m.unread = !on) : (m.starred = on);
    new Set(msgs.map((m) => m.account_id)).forEach(refreshFolders);
  }

  function openMove() {
    const msgs = multi ? selectedMsgs : selected ? [selected] : [];
    const owners = new Set(msgs.map((m) => m.account_id));
    if (!msgs.length) return;
    if (owners.size > 1) return flash("Pick messages from one account to move them to a folder.");
    moving = { accountId: msgs[0].account_id, folderId: msgs.length === 1 ? msgs[0].folder_id : null, count: msgs.length };
  }

  function moveToPicked(f) {
    moving = null;
    bulkMove((ids) => api.moveToFolder(ids, f.id), `Moved to ${f.name}:`);
  }

  // ---- Drag messages onto a folder -------------------------------------------

  function dragRows(event, m) {
    // Dragging a selected message drags the whole selection; any other row drags just itself.
    const ids = selectedIds.includes(m.id) ? selectedIds : [m.id];
    event.dataTransfer.setData("application/x-oxymail", JSON.stringify({ ids, account: m.account_id }));
    event.dataTransfer.effectAllowed = "move";
  }

  let dropTarget = $state(null);

  function dropOnFolder(event, f) {
    dropTarget = null;
    const data = event.dataTransfer.getData("application/x-oxymail");
    if (!data) return;
    const { ids, account } = JSON.parse(data);
    if (account !== f.account_id) return flash("Messages can only be dropped on folders of their own account.");
    const msgs = list.filter((m) => ids.includes(m.id));
    bulkMove((moved) => api.moveToFolder(moved, f.id), `Moved to ${f.name}:`, true, msgs);
  }

  // ---- Reader extras ----------------------------------------------------------

  /** Opens a message from the conversation list, even when it isn't in the visible list. */
  async function openFromThread(t) {
    if (!list.some((m) => m.id === t.id)) list = [t, ...list];
    await select(t.id);
  }

  async function allowSender() {
    if (!view?.from_addr) return;
    await act(() => api.setImagesAllowed(view.from_addr, true));
    view.images_allowed = true;
  }

  function unsubscribe() {
    const web = view.unsubscribe.find((u) => u.startsWith("http"));
    const mail = view.unsubscribe.find((u) => u.startsWith("mailto:"));
    if (web) {
      if (confirm("Open the sender's unsubscribe page in your browser?")) act(() => api.openUrl(web));
    } else if (mail) {
      const m = parseMailto(mail);
      composing = { account_id: sendingAccount(selected), to: m.to, subject: m.subject || "Unsubscribe", note: "This is the sender's unsubscribe address." };
    }
  }

  function dropFromList(id) {
    const i = list.findIndex((m) => m.id === id);
    if (i < 0) return;
    list = list.filter((m) => m.id !== id);
    selectedIds = selectedIds.filter((x) => x !== id);
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
    if (showSettings || composing || snoozing || moving) return;
    if (event.target instanceof HTMLInputElement) {
      if (event.key === "Escape") {
        event.target.blur();
        if (searching) clearSearch();
      }
      return;
    }
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "a" && list.length) {
      event.preventDefault();
      selectedIds = list.map((m) => m.id);
      return;
    }
    if (event.ctrlKey || event.metaKey || event.altKey) return;
    const i = list.findIndex((m) => m.id === selectedId);
    const keys = {
      ArrowDown: () => list[Math.min(i + 1, list.length - 1)] && select(list[Math.min(i + 1, list.length - 1)].id),
      j: () => keys.ArrowDown(),
      ArrowUp: () => list[Math.max(i - 1, 0)] && select(list[Math.max(i - 1, 0)].id),
      k: () => keys.ArrowUp(),
      e: () => (selectedRole === "archive" ? moveToInbox() : archive()),
      Delete: () => remove(),
      "#": () => remove(),
      s: () => (multi ? bulkFlag("F", !selectedMsgs.every((m) => m.starred)) : toggleStar()),
      u: () => (multi ? bulkFlag("S", selectedMsgs.some((m) => m.unread)) : toggleRead()),
      v: () => openMove(),
      b: () => openSnooze(),
      "/": () => searchInput?.focus(),
      c: () => compose(),
      r: () => reply(false),
      a: () => reply(true),
      f: () => forward(),
      Escape: () => (multi ? (selectedIds = selectedId ? [selectedId] : []) : searching && clearSearch()),
    };
    const fn = keys[event.key];
    if (fn) {
      event.preventDefault();
      fn();
    }
  }

  function flash(message, undo = null, ms = 6000) {
    clearTimeout(toastTimer);
    toast = { message, undo };
    toastTimer = setTimeout(() => (toast = null), ms);
  }

  // ---- Sending with undo ----------------------------------------------------

  /** Holds a message for the undo window, then sends it. Undo or failure reopens Compose. */
  function queueSend(accountId, mail, reopen) {
    composing = null;
    const seconds = Number(settings.undo_send_seconds) || 0;
    const send = async () => {
      try {
        flash(await api.sendMessage(accountId, mail));
        if (reopen.draft_id) await api.deleteDraft(reopen.draft_id).catch(() => {});
        refreshDrafts();
      } catch (e) {
        composing = { ...reopen, error: `Not sent: ${e}` };
      }
    };
    if (!seconds) return send();
    const timer = setTimeout(send, seconds * 1000);
    flash(`Sending in ${seconds} seconds…`, () => {
      clearTimeout(timer);
      composing = reopen;
    }, seconds * 1000);
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
      <input
        bind:this={searchInput}
        type="search"
        placeholder="Search all mail"
        title="Filters: from:  to:  subject:  has:attachment  is:unread  is:starred  in:sent  account:oxydev  before:2025-01-01  after:…"
        bind:value={query}
      />
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
      <nav class="accounts unified">
        <div class="account" class:active={folder?.unified && !searching}>
          <button class="account-main" onclick={openAllInboxes}>
            <span class="all-icon"><Icon name="inbox" size={15} /></span>
            <span class="ellipsis">All inboxes</span>
            {#if allInboxUnread > 0}<span class="count">{allInboxUnread.toLocaleString()}</span>{/if}
          </button>
        </div>
      </nav>
      {#if drafts.length}
        <nav class="accounts unified">
          <div class="account" class:active={folder?.localDrafts}>
            <button class="account-main" onclick={openDrafts}>
              <span class="all-icon"><Icon name="draft" size={15} /></span>
              <span class="ellipsis">Unsent drafts</span>
              <span class="count muted-count">{drafts.length}</span>
            </button>
          </div>
        </nav>
      {/if}
      {#if snoozedList.length || scheduledList.length}
        <nav class="accounts unified">
          {#if snoozedList.length}
            <div class="account" class:active={folder?.snoozedView}>
              <button class="account-main" onclick={() => openQueue("snoozed")}>
                <span class="all-icon"><Icon name="clock" size={15} /></span>
                <span class="ellipsis">Snoozed</span>
                <span class="count muted-count">{snoozedList.length}</span>
              </button>
            </div>
          {/if}
          {#if scheduledList.length}
            <div class="account" class:active={folder?.scheduledView}>
              <button class="account-main" onclick={() => openQueue("scheduled")}>
                <span class="all-icon"><Icon name="send" size={15} /></span>
                <span class="ellipsis">Scheduled</span>
                <span class="count" class:muted-count={!scheduledList.some((x) => x.error)}>{scheduledList.length}</span>
              </button>
            </div>
          {/if}
        </nav>
      {/if}
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
          <span>Folders</span>
        </div>
        <nav class="folders">
          {#each folders as f (f.id)}
            <button
              class="folder"
              class:active={f.id === folder?.id && !searching}
              class:drop={dropTarget === f.id}
              onclick={() => openFolder(f)}
              ondragover={(e) => { e.preventDefault(); dropTarget = f.id; }}
              ondragleave={() => dropTarget === f.id && (dropTarget = null)}
              ondrop={(e) => { e.preventDefault(); dropOnFolder(e, f); }}
            >
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
        {#if !searching && !folder?.unified && folder?.unread > 0}
          <button class="mark-read" onclick={markFolderRead} title="Mark every message in this folder as read">
            <Icon name="checks" size={14} /> Mark all read
          </button>
        {/if}
      </div>
      <div class="list" onscroll={onListScroll}>
        {#each list as m (m.id)}
          <div
            class="row"
            class:selected={selectedIds.includes(m.id)}
            class:unread={m.unread}
            draggable={!m.draft}
            ondragstart={(e) => dragRows(e, m)}
            role="listitem"
          >
            <button class="row-main" onclick={(e) => rowClick(e, m)}>
              <div class="row-top">
                <span class="ellipsis sender">{sender(m)}</span>
                <span class="date">{shortDate(m.date)}</span>
              </div>
              <div class="row-subject">
                {#if m.attachments > 0}<span class="clip"><Icon name="clip" size={12} /></span>{/if}
                <span class="ellipsis">{m.subject || "(no subject)"}</span>
              </div>
              {#if settings.density !== "compact"}
                <div class="row-snippet ellipsis">{m.snippet}</div>
              {/if}
              {#if m.snoozed_until}
                <div class="row-folder snoozed-tag"><Icon name="clock" size={11} /> Back {when(m.snoozed_until)}</div>
              {/if}
              {#if searching || folder?.unified}
                {@const a = accounts.find((x) => x.id === m.account_id)}
                <div class="row-folder">
                  {#if a}<span class="acct-dot" style:background={a.color || "var(--faint)"}></span>{accountLabel(a)}{#if searching} · {m.folder_name}{/if}{:else}{m.folder_name}{/if}
                </div>
              {/if}
            </button>
            <div class="row-actions" class:hidden={m.draft}>
              <button class:starred={m.starred} onclick={() => toggleStar(m)} title={m.starred ? "Unstar" : "Star"}>
                <Icon name="star" size={15} />
              </button>
              {#if m.folder_role !== "archive" && m.folder_role !== "trash"}
                <button onclick={() => archive(m)} title="Archive"><Icon name="archive" size={15} /></button>
              {/if}
              <button onclick={() => remove(m)} title={m.folder_role === "trash" ? "Delete forever" : "Delete"}>
                <Icon name="trash" size={15} />
              </button>
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
      {:else if multi}
        <div class="bulk">
          <div class="bulk-count">{selectedIds.length} messages selected</div>
          <div class="bulk-actions">
            {#if selectedMsgs.every((m) => m.folder_role === "trash")}
              <button onclick={() => restore()}><Icon name="restore" size={16} /><span>Restore</span></button>
              <button class="danger" onclick={() => remove()}><Icon name="trash" size={16} /><span>Delete forever</span></button>
            {:else}
              <button onclick={() => archive()}><Icon name="archive" size={16} /><span>Archive</span></button>
              <button onclick={() => remove()}><Icon name="trash" size={16} /><span>Delete</span></button>
            {/if}
            <button onclick={openMove}><Icon name="folder" size={16} /><span>Move to…</span></button>
            <button onclick={openSnooze}><Icon name="clock" size={16} /><span>Snooze</span></button>
            <button onclick={() => bulkFlag("S", true)}><Icon name="open" size={16} /><span>Mark read</span></button>
            <button onclick={() => bulkFlag("S", false)}><Icon name="mail" size={16} /><span>Mark unread</span></button>
            <button onclick={() => bulkFlag("F", !selectedMsgs.every((m) => m.starred))}><Icon name="star" size={16} /><span>{selectedMsgs.every((m) => m.starred) ? "Unstar" : "Star"}</span></button>
          </div>
          <button class="link" onclick={() => (selectedIds = selectedId ? [selectedId] : [])}>Clear selection</button>
          <p class="bulk-hint">Ctrl-click adds or removes a message · Shift-click selects a range · Ctrl+A selects all loaded</p>
        </div>
      {:else if view && selected}
        <div class="toolbar">
          <button onclick={() => reply(false)} title="Reply (r)"><Icon name="reply" size={16} /><span>Reply</span></button>
          <button onclick={() => reply(true)} title="Reply all (a)"><Icon name="replyAll" size={16} /><span>Reply all</span></button>
          <button onclick={forward} title="Forward (f)"><Icon name="forward" size={16} /><span>Forward</span></button>
          <span class="sep"></span>
          {#if selectedRole === "trash"}
            <button onclick={() => restore()}><Icon name="restore" size={16} /><span>Restore</span></button>
            <button class="danger" onclick={() => remove()}><Icon name="trash" size={16} /><span>{selectedLive ? "Delete forever" : "Remove from OxyMail"}</span></button>
          {:else}
            {#if selectedRole === "archive"}
              <button onclick={() => moveToInbox()} title="Move to Inbox (e)"><Icon name="inbox" size={16} /><span>Move to Inbox</span></button>
            {:else}
              <button onclick={() => archive()} title="Archive (e)"><Icon name="archive" size={16} /><span>Archive</span></button>
            {/if}
            <button onclick={() => remove()} title="Delete (Del)"><Icon name="trash" size={16} /><span>Delete</span></button>
          {/if}
          <button class="secondary" onclick={openMove} title="Move to folder (v)"><Icon name="folder" size={16} /><span>Move</span></button>
          {#if selected.snoozed_until}
            <button class="secondary" onclick={() => unsnooze()} title="Bring it back now"><Icon name="clock" size={16} /><span>Unsnooze</span></button>
          {:else}
            <button class="secondary" onclick={openSnooze} title="Snooze (b)"><Icon name="clock" size={16} /><span>Snooze</span></button>
          {/if}
          <span class="sep"></span>
          <button class="secondary" class:starred={selected.starred} onclick={() => toggleStar()} title="Star (s)">
            <Icon name="star" size={16} /><span>{selected.starred ? "Starred" : "Star"}</span>
          </button>
          <button class="secondary" onclick={() => toggleRead()} title={selected.unread ? "Mark read (u)" : "Mark unread (u)"}>
            <Icon name={selected.unread ? "open" : "mail"} size={16} /><span>{selected.unread ? "Mark read" : "Mark unread"}</span>
          </button>
        </div>
        <header class="msg-head">
          <h1>{view.subject || "(no subject)"}</h1>
          <div class="meta">
            <div><span class="label">From</span><span class="value">{view.from.join(", ")}</span></div>
            {#if view.to.length}<div><span class="label">To</span><span class="value">{view.to.join(", ")}</span></div>{/if}
            {#if view.cc.length}<div><span class="label">Cc</span><span class="value">{view.cc.join(", ")}</span></div>{/if}
            <div class="muted date-line">
              {longDate(view.date)}
              {#if view.unsubscribe.length}
                <button class="unsub" onclick={unsubscribe}>Unsubscribe</button>
              {/if}
            </div>
          </div>
          {#each view.notices as notice}<div class="notice">{notice}</div>{/each}
          {#if conversation.length > 1}
            <details class="thread">
              <summary>{conversation.length} messages in this conversation</summary>
              <ol>
                {#each conversation as t (t.id)}
                  <li>
                    <button class:current={t.id === view.id} onclick={() => openFromThread(t)} disabled={t.id === view.id}>
                      <span class="t-from ellipsis">{sender(t)}</span>
                      <span class="t-folder">{t.folder_name}</span>
                      <span class="t-date">{shortDate(t.date)}</span>
                    </button>
                  </li>
                {/each}
              </ol>
            </details>
          {/if}
          {#if view.attachments.length}
            <div class="attachments">
              {#each view.attachments as a (a.index)}
                <div class="chip">
                  <button class="chip-open" onclick={() => act(() => api.openAttachment(view.id, a.index))} title="Open with the default app">
                    <Icon name="clip" size={13} />
                    <span class="ellipsis">{a.name}</span>
                    <span class="muted">{formatSize(a.size)}</span>
                  </button>
                  <button class="chip-save" onclick={() => act(async () => flash(`Saved to ${await api.saveAttachment(view.id, a.index)}`))} title="Save to Downloads">
                    <Icon name="download" size={14} />
                  </button>
                </div>
              {/each}
            </div>
          {/if}
        </header>
        {#if view.has_remote_content && !view.images_allowed && !remoteAllowed}
          <div class="remote-bar">
            <span>Remote images blocked for privacy.</span>
            <button onclick={() => (remoteAllowed = true)}>Load images</button>
            {#if view.from_addr}<button onclick={allowSender} title={`Always load images from ${view.from_addr}`}>Always for this sender</button>{/if}
          </div>
        {/if}
        {#if view.html}
          <iframe
            class="body-frame"
            title="Message"
            sandbox="allow-popups"
            srcdoc={messageDocument(view.html, remoteAllowed || view.images_allowed)}
          ></iframe>
        {:else}
          <pre class="body-text">{view.text ?? ""}</pre>
        {/if}
      {:else}
        <div class="placeholder">
          <Logo size={40} />
          <p>
            {folder?.localDrafts
              ? "Click a draft to keep writing it."
              : folder?.scheduledView
                ? "Click a scheduled message to change or cancel it."
                : "Select a message to read it."}
          </p>
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

{#if snoozing}
  <TimeMenu title={snoozing.length > 1 ? `Snooze ${snoozing.length} messages until…` : "Snooze until…"} onpick={doSnooze} onclose={() => (snoozing = null)} />
{/if}

{#if moving}
  <MovePicker
    accountId={moving.accountId}
    currentFolderId={moving.folderId}
    count={moving.count}
    onpick={moveToPicked}
    onclose={() => (moving = null)}
  />
{/if}

{#if composing}
  <Compose
    draft={composing}
    accounts={liveAccounts}
    onsend={queueSend}
    onschedule={scheduleFromCompose}
    onclose={(kept) => { composing = null; refreshDrafts(); if (kept) flash("Saved to Drafts"); }}
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
  /* Smaller windows: narrower sidebar and list so the reader keeps room. */
  @media (max-width: 1180px) {
    .shell { grid-template-columns: 220px 320px minmax(0, 1fr); }
    .titlebar { grid-template-columns: 220px 320px 1fr auto; }
  }

  /* ---- Sidebar ---- */
  .sidebar { overflow-y: auto; background: var(--sidebar); border-right: 1px solid var(--border); min-width: 0; min-height: 0; padding-bottom: 12px; }
  .section { padding: 12px 16px 6px; font-size: 11px; font-weight: 700; letter-spacing: 0.06em; text-transform: uppercase; color: var(--faint); }
  .folders-head { border-top: 1px solid var(--border); margin-top: 6px; padding-top: 12px; display: flex; }
  .accounts, .folders { padding: 0 8px; }
  .account { position: relative; display: flex; align-items: center; border-radius: 7px; }
  .account:hover { background: var(--hover); }
  .account.active { background: var(--selected); }
  .unified { padding-top: 10px; }
  .unified + .unified { padding-top: 2px; }
  .snoozed-tag { display: inline-flex; align-items: center; gap: 4px; background: color-mix(in srgb, var(--amber) 16%, transparent); color: var(--warn-text); }
  .muted-count { background: var(--hover); color: var(--muted); }
  .all-icon { display: grid; place-items: center; width: 24px; height: 24px; border-radius: 30%; background: var(--accent-soft); color: var(--accent); flex-shrink: 0; }
  .acct-dot { display: inline-block; width: 7px; height: 7px; margin-right: 5px; border-radius: 50%; vertical-align: 1px; }
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
  .row-subject { display: flex; align-items: center; gap: 4px; min-width: 0; font-size: 13px; margin-top: 1px; }
  .clip { flex-shrink: 0; }
  .row-snippet { font-size: 12px; color: var(--muted); margin-top: 2px; padding-right: 20px; }
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
  .row:hover .row-actions.hidden { display: none; }
  .row-actions button { display: grid; place-items: center; width: 28px; height: 26px; border-radius: 5px; color: var(--muted); }
  .row-actions button:hover { background: var(--hover); color: var(--text); }
  .row-actions button.starred, .toolbar button.starred { color: var(--star); }
  .row-actions button.starred :global(path), .toolbar button.starred :global(path), .star-pin :global(path) { fill: currentColor; }
  .star-pin { position: absolute; right: 12px; bottom: 9px; color: var(--star); display: flex; }
  .row:hover .star-pin { display: none; }

  /* ---- Reader ---- */
  .reader { display: flex; flex-direction: column; min-width: 0; min-height: 0; background: var(--panel); container: reader / inline-size; }
  .toolbar { display: flex; flex-wrap: wrap; align-items: center; gap: 2px; padding: 8px 14px; border-bottom: 1px solid var(--border); }
  /* In a narrow reader the toolbar drops its labels and keeps the icons (titles still name them). */
  /* Medium width: the less-used buttons drop their labels first. */
  @container reader (max-width: 900px) {
    .toolbar button.secondary span { display: none; }
    .toolbar button.secondary { padding: 6px 8px; }
  }
  @container reader (max-width: 640px) {
    .toolbar button span { display: none; }
    .toolbar button { padding: 6px 8px; }
  }
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
  .chip .muted { white-space: nowrap; flex-shrink: 0; }
  .chip-open { display: flex; align-items: center; gap: 7px; min-width: 0; padding: 6px 4px 6px 10px; }
  .chip-save { display: grid; place-items: center; padding: 6px 8px; border-left: 1px solid var(--border); color: var(--muted); }
  .chip-save:hover { color: var(--accent); }
  .thread { margin-top: 10px; font-size: 13px; }
  .thread summary { cursor: pointer; color: var(--accent); }
  .thread ol { margin: 6px 0 0; padding: 0; list-style: none; border: 1px solid var(--border); border-radius: 8px; max-height: 220px; overflow-y: auto; }
  .thread li + li { border-top: 1px solid var(--border); }
  .thread li button { display: grid; grid-template-columns: minmax(0, 1fr) auto 70px; gap: 10px; width: 100%; padding: 6px 10px; text-align: left; font-size: 12px; }
  .thread li button:hover:not(:disabled) { background: var(--hover); }
  .thread li button.current { background: var(--selected); font-weight: 600; cursor: default; }
  .t-folder { color: var(--muted); }
  .t-date { color: var(--muted); text-align: right; }
  .date-line { display: flex; align-items: center; gap: 10px; }
  .unsub { padding: 1px 8px; border: 1px solid var(--border); border-radius: 99px; font-size: 12px; color: var(--muted); }
  .unsub:hover { border-color: var(--accent); color: var(--accent); }
  .remote-bar { display: flex; align-items: center; flex-wrap: wrap; gap: 10px; padding: 8px 24px; border-bottom: 1px solid var(--border); background: var(--bg); font-size: 12px; color: var(--muted); }
  .remote-bar button { padding: 3px 10px; border: 1px solid var(--border); border-radius: 6px; font-size: 12px; color: var(--text); }
  .remote-bar button:hover { border-color: var(--accent); color: var(--accent); }
  .folder.drop { background: var(--accent-soft); color: var(--accent); box-shadow: inset 0 0 0 1px var(--accent); }
  .bulk { margin: auto; display: grid; justify-items: center; gap: 14px; padding: 24px; text-align: center; }
  .bulk-count { font-size: 18px; font-weight: 650; }
  .bulk-actions { display: flex; flex-wrap: wrap; justify-content: center; gap: 6px; max-width: 520px; }
  .bulk-actions button { display: flex; align-items: center; gap: 6px; padding: 7px 12px; border: 1px solid var(--border); border-radius: 8px; font-size: 13px; }
  .bulk-actions button:hover { border-color: var(--accent); color: var(--accent); }
  .bulk-actions button.danger:hover { border-color: var(--danger); color: var(--danger); }
  .bulk-hint { margin: 0; font-size: 12px; color: var(--faint); }
  .link { color: var(--accent); font-size: 13px; }
  .chip { display: flex; align-items: center; gap: 0; max-width: 320px; padding: 0 !important; padding: 6px 10px; border: 1px solid var(--border); border-radius: var(--radius); font-size: 12px; }
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
