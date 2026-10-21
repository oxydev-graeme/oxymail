<script>
  import { api, accountLabel, imageToIcon } from "./api.js";
  import Avatar from "./Avatar.svelte";
  import Icon from "./Icon.svelte";
  import SignatureEditor from "./SignatureEditor.svelte";
  import AddAccount from "./AddAccount.svelte";
  import ServerFields from "./ServerFields.svelte";
  import AuthChoice from "./AuthChoice.svelte";
  import { onMount } from "svelte";

  let { accounts, onchanged } = $props();

  const SWATCHES = ["#0f5e73", "#0e7490", "#2f6fed", "#7030a0", "#c0504d", "#c00000", "#e36c09", "#fe9a00", "#00b050", "#4f6128", "#244061", "#69708a"];

  let order = $state([]);
  let editing = $state(null);
  let draft = $state(null);
  let saving = $state(false);
  let dragIndex = $state(null);
  let overIndex = $state(null);
  let message = $state("");
  let adding = $state(false);
  let server = $state(null);
  let newPassword = $state("");
  let serverBusy = $state(false);
  let serverMessage = $state("");
  let serverOk = $state(false);

  $effect(() => {
    order = [...accounts];
  });

  async function saveOrder(next) {
    order = next;
    await api.reorderAccounts(next.map((a) => a.id));
    onchanged();
  }

  function move(i, delta) {
    const j = i + delta;
    if (j < 0 || j >= order.length) return;
    const next = [...order];
    [next[i], next[j]] = [next[j], next[i]];
    saveOrder(next);
  }

  function drop(i) {
    if (dragIndex === null || dragIndex === i) return reset();
    const next = [...order];
    const [moved] = next.splice(dragIndex, 1);
    next.splice(i, 0, moved);
    reset();
    saveOrder(next);
  }

  function reset() {
    dragIndex = null;
    overIndex = null;
  }

  function edit(account) {
    editing = account;
    draft = {
      label: account.label ?? "",
      sender_name: account.sender_name ?? "",
      color: account.color ?? "",
      signature: account.signature ?? "",
      icon: account.icon ?? "",
    };
    message = "";
    connecting = null;
    connectPassword = "";
    connectError = "";
    server = null;
    newPassword = "";
    serverMessage = "";
    if (account.kind === "imap") api.liveConfig(account.id).then((c) => (server = c)).catch((e) => (serverMessage = String(e)));
  }

  async function pickIcon(event) {
    const file = event.currentTarget.files?.[0];
    event.currentTarget.value = "";
    if (!file) return;
    try {
      draft.icon = await imageToIcon(file);
    } catch (e) {
      message = String(e.message ?? e);
    }
  }

  async function save() {
    saving = true;
    try {
      await api.updateAccount(editing.id, draft);
      message = "Saved.";
      onchanged();
    } catch (e) {
      message = `Could not save: ${e}`;
    } finally {
      saving = false;
    }
  }

  let clients = $state({});
  let preferPassword = $state(false);
  const clientReady = (provider) => Boolean(provider && clients[provider]?.client_id);
  let connectOAuth = $derived(Boolean(connecting?.oauth && clientReady(connecting.oauth) && !preferPassword));

  onMount(async () => {
    clients = await api.oauthClients().catch(() => ({}));
  });

  let connecting = $state(null);
  let connectPassword = $state("");
  let connectBusy = $state(false);
  let connectError = $state("");

  async function startConnect() {
    connectError = "";
    connectBusy = true;
    try {
      connecting = await api.connectSuggestion(editing.id);
    } catch (e) {
      connectError = String(e);
    } finally {
      connectBusy = false;
    }
  }

  async function connect() {
    connectError = "";
    connectBusy = true;
    try {
      connecting.imap.port = Number(connecting.imap.port);
      connecting.smtp.port = Number(connecting.smtp.port);
      if (connectOAuth) {
        await api.oauthSignIn(connecting.oauth, connecting.email);
        connecting.auth = connecting.oauth;
        await api.connectArchive(editing.id, connecting, null);
      } else {
        connecting.auth = null;
        await api.connectArchive(editing.id, connecting, connectPassword);
      }
      const id = editing.id;
      connecting = null;
      connectPassword = "";
      await onchanged();
      edit({ ...editing, kind: "imap", id });
      serverOk = true;
      serverMessage = "Connected. Downloading new mail now; your archive is kept.";
    } catch (e) {
      connectError = String(e);
    } finally {
      connectBusy = false;
    }
  }

  async function signInAgain() {
    serverBusy = true;
    serverMessage = "";
    try {
      await api.oauthSignIn(server.auth, server.email);
      await api.updateServerSettings(editing.id, server, null);
      serverOk = true;
      serverMessage = "Signed in again. Settings saved.";
    } catch (e) {
      serverOk = false;
      serverMessage = String(e);
    } finally {
      serverBusy = false;
    }
  }

  async function saveServer() {
    serverBusy = true;
    serverMessage = "";
    try {
      server.imap.port = Number(server.imap.port);
      server.smtp.port = Number(server.smtp.port);
      await api.updateServerSettings(editing.id, server, newPassword || null);
      newPassword = "";
      serverOk = true;
      serverMessage = "Signed in to both servers. Settings saved.";
    } catch (e) {
      serverOk = false;
      serverMessage = String(e);
    } finally {
      serverBusy = false;
    }
  }

  async function remove() {
    const ok = confirm(
      `Remove ${editing.name} from OxyMail?\n\n` +
        (editing.kind === "imap"
          ? "OxyMail stops syncing it and forgets its saved password. Mail on the server is not touched."
          : "Its folders and messages disappear from the app and from search. Nothing is deleted: " +
            "importing the same archive again brings it back with your stars and folders."),
    );
    if (!ok) return;
    await api.removeAccount(editing.id);
    editing = null;
    onchanged();
  }
</script>

{#if adding}
  <AddAccount oncancel={() => (adding = false)} onadded={() => { adding = false; onchanged(); }} />
{:else if editing}
  <button class="back" onclick={() => (editing = null)}>‹ All accounts</button>
  <h2>{editing.name}</h2>

  <div class="grid">
    <label>
      <span class="label">Label in sidebar</span>
      <input bind:value={draft.label} placeholder={editing.name} />
    </label>
    <label>
      <span class="label">Your name when sending</span>
      <input bind:value={draft.sender_name} placeholder="e.g. Graeme Bosworth" />
    </label>
  </div>

  <div class="field">
    <span class="label">Icon</span>
    <div class="icon-row">
      <Avatar account={{ ...editing, ...draft, icon: draft.icon || null, color: draft.color || null }} size={48} />
      <label class="upload">
        Upload image…
        <input type="file" accept="image/png,image/jpeg,image/webp,image/gif,image/svg+xml" onchange={pickIcon} />
      </label>
      {#if draft.icon}<button class="link" onclick={() => (draft.icon = "")}>Use initial instead</button>{/if}
    </div>
    <p class="hint">Square images look best. They're cropped to a square and stored at 96 × 96.</p>
  </div>

  <div class="field">
    <span class="label">Colour</span>
    <div class="swatches">
      {#each SWATCHES as c}
        <button class="swatch" class:on={draft.color.toLowerCase() === c} style:background={c} onclick={() => (draft.color = c)} title={c}></button>
      {/each}
      <input type="color" value={draft.color || "#0f5e73"} oninput={(e) => (draft.color = e.currentTarget.value)} title="Custom colour" />
      {#if draft.color}<button class="link" onclick={() => (draft.color = "")}>None</button>{/if}
    </div>
  </div>

  <div class="field">
    <span class="label">Signature</span>
    <SignatureEditor bind:value={draft.signature} />
    <p class="hint">Added to new messages and replies from this account.</p>
  </div>

  {#if editing.kind === "archive"}
    <div class="field server">
      <h3>Connect to mail server</h3>
      {#if !connecting}
        <p class="hint">
          This account was imported from Mailbird and is read-only. Connect it to send and receive here. Your
          archived mail stays, and new mail lands in the same folders.
        </p>
        <div class="server-actions">
          <button class="ghost" onclick={startConnect} disabled={connectBusy}>
            {connectBusy ? "Looking up settings…" : "Set up sending and receiving"}
          </button>
        </div>
      {:else}
        <div class="found">Filled in from {connecting.source}. Check them, then add the password.</div>
        <ServerFields bind:config={connecting} />
        <div class="auth">
          <AuthChoice provider={connecting.oauth} clientReady={clientReady(connecting.oauth)} bind:password={connectPassword} bind:preferPassword />
        </div>
        <div class="server-actions">
          <button class="primary" onclick={connect} disabled={connectBusy || (!connectOAuth && !connectPassword)}>
            {connectBusy ? (connectOAuth ? "Waiting for the browser…" : "Checking sign-in…") : connectOAuth ? "Sign in and connect" : "Connect"}
          </button>
          <button class="ghost" onclick={() => (connecting = null)}>Cancel</button>
        </div>
      {/if}
      {#if connectError}<p class="bad">{connectError}</p>{/if}
    </div>
  {:else if editing.kind === "imap"}
    <div class="field server">
      <h3>Server settings</h3>
      {#if server}
        <ServerFields bind:config={server} />
        {#if server.auth}
          <div class="signed-in">
            Signed in with {server.auth === "google" ? "Google" : "Microsoft"}.
            <button class="link" onclick={signInAgain} disabled={serverBusy}>Sign in again</button>
          </div>
        {:else}
          <label class="password">
            <span class="label">Password</span>
            <input type="password" bind:value={newPassword} placeholder="Unchanged — saved in your keyring" />
          </label>
        {/if}
        <div class="server-actions">
          <button class="ghost" onclick={saveServer} disabled={serverBusy}>
            {serverBusy ? "Testing sign-in…" : "Test and save server settings"}
          </button>
          {#if serverMessage}<span class:ok={serverOk} class:bad={!serverOk}>{serverMessage}</span>{/if}
        </div>
      {:else if serverMessage}
        <p class="bad">{serverMessage}</p>
      {/if}
    </div>
  {/if}

  <div class="actions">
    <button class="primary" onclick={save} disabled={saving}>{saving ? "Saving…" : "Save details"}</button>
    {#if message}<span class="muted">{message}</span>{/if}
    <span class="grow"></span>
    <button class="danger" onclick={remove}><Icon name="trash" size={15} /> Remove account</button>
  </div>
{:else}
  <div class="head">
    <h2>Accounts</h2>
    <button class="primary" onclick={() => (adding = true)}>Add account</button>
  </div>
  <p class="hint top">Drag to reorder. The sidebar follows this order.</p>
  <ul class="accounts">
    {#each order as account, i (account.id)}
      <li
        draggable="true"
        class:dragging={dragIndex === i}
        class:over={overIndex === i && dragIndex !== i}
        ondragstart={(e) => { dragIndex = i; e.dataTransfer.effectAllowed = "move"; e.dataTransfer.setData("text/plain", String(i)); }}
        ondragover={(e) => { e.preventDefault(); overIndex = i; }}
        ondrop={(e) => { e.preventDefault(); drop(i); }}
        ondragend={reset}
      >
        <span class="grip" aria-hidden="true">⋮⋮</span>
        <Avatar {account} size={22} />
        <span class="names">
          <span class="ellipsis">{accountLabel(account)}</span>
          {#if account.label}<span class="ellipsis muted small">{account.name}</span>{/if}
        </span>
        {#if account.signature}<span class="tag" title="Has a signature">Signature</span>{/if}
        <span class="badge" class:live={account.kind === "imap"}>
          {account.kind === "archive" ? "Archive" : account.has_archive ? "Live + archive" : "Live"}
        </span>
        <span class="order">
          <button onclick={() => move(i, -1)} disabled={i === 0} title="Move up">▲</button>
          <button onclick={() => move(i, 1)} disabled={i === order.length - 1} title="Move down">▼</button>
        </span>
        <button class="edit" onclick={() => edit(account)}>Edit</button>
      </li>
    {/each}
  </ul>
  <p class="hint">
    Archive accounts are read from your Mailbird export. Starring, archiving and deleting only change OxyMail's
    index; the files on disk are never modified.
  </p>
{/if}

<style>
  h2 { margin: 0 0 6px; font-size: 17px; }
  .hint.top { margin-bottom: 12px; }
  .hint { margin: 6px 0 0; font-size: 12px; color: var(--muted); line-height: 1.5; }
  .label { display: block; font-size: 13px; font-weight: 600; margin-bottom: 6px; }
  .field { margin-top: 18px; }
  .accounts { list-style: none; margin: 0; padding: 0; border: 1px solid var(--border); border-radius: 8px; }
  .accounts li {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 8px 6px 6px;
    font-size: 13px;
    border-bottom: 1px solid var(--border);
    background: var(--panel);
  }
  .accounts li:last-child { border-bottom: 0; }
  .accounts li.dragging { opacity: 0.4; }
  .accounts li.over { box-shadow: inset 0 2px 0 var(--accent); }
  .grip { cursor: grab; color: var(--faint); letter-spacing: -3px; padding: 0 4px; }
  .icon-row { display: flex; align-items: center; gap: 14px; }
  .upload { position: relative; padding: 6px 12px; border: 1px solid var(--border); border-radius: 8px; font-size: 13px; cursor: pointer; }
  .upload:hover { border-color: var(--accent); color: var(--accent); }
  .upload input { position: absolute; inset: 0; opacity: 0; cursor: pointer; }
  .names { display: grid; flex: 1; min-width: 0; }
  .small { font-size: 11px; }
  .badge, .tag { font-size: 11px; padding: 1px 8px; border-radius: 99px; flex-shrink: 0; }
  .badge { background: var(--accent-soft); color: var(--accent); }
  .badge.live { background: color-mix(in srgb, var(--amber) 18%, transparent); color: var(--warn-text); }
  .head { display: flex; align-items: center; justify-content: space-between; }
  .server { padding-top: 16px; border-top: 1px solid var(--border); }
  h3 { margin: 0 0 12px; font-size: 15px; }
  .password { display: block; margin-top: 14px; }
  .password input { width: 100%; padding: 7px 10px; border: 1px solid var(--border); border-radius: 8px; background: var(--bg); color: var(--text); outline: none; }
  .password input:focus { border-color: var(--accent); }
  .server-actions { display: flex; align-items: center; gap: 12px; margin-top: 12px; flex-wrap: wrap; }
  .ghost { padding: 7px 14px; border-radius: 8px; border: 1px solid var(--border); font-size: 13px; }
  .ghost:hover:not(:disabled) { border-color: var(--accent); color: var(--accent); }
  .ok { color: var(--accent); font-size: 13px; }
  .auth { margin-top: 14px; }
  .signed-in { display: flex; align-items: center; gap: 10px; margin-top: 14px; padding: 10px 12px; border-radius: 8px; background: var(--accent-soft); font-size: 13px; }
  .found { margin-bottom: 12px; padding: 10px 12px; border-radius: 8px; background: var(--accent-soft); font-size: 13px; }
  .bad { color: var(--danger); font-size: 13px; user-select: text; }
  .tag { border: 1px solid var(--border); color: var(--muted); }
  .order { display: flex; gap: 1px; }
  .order button { width: 22px; height: 22px; border-radius: 4px; font-size: 9px; color: var(--muted); }
  .order button:hover:not(:disabled) { background: var(--hover); color: var(--text); }
  .order button:disabled { opacity: 0.3; cursor: default; }
  .edit { padding: 4px 10px; border-radius: 6px; border: 1px solid var(--border); font-size: 12px; }
  .edit:hover { border-color: var(--accent); color: var(--accent); }
  .back { color: var(--accent); font-size: 13px; padding: 0; margin-bottom: 10px; }
  .grid { display: grid; grid-template-columns: 1fr 1fr; gap: 14px; margin-top: 14px; }
  .grid input {
    width: 100%;
    padding: 7px 10px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg);
    outline: none;
  }
  .grid input:focus { border-color: var(--accent); }
  .swatches { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; }
  .swatch { width: 22px; height: 22px; border-radius: 50%; border: 2px solid transparent; }
  .swatch.on { border-color: var(--text); box-shadow: inset 0 0 0 2px var(--panel); }
  .swatches input[type="color"] { width: 28px; height: 24px; padding: 0; border: 0; background: none; cursor: pointer; }
  .link { color: var(--accent); font-size: 12px; }
  .actions { display: flex; align-items: center; gap: 12px; margin-top: 20px; }
  .grow { flex: 1; }
  .primary { padding: 7px 16px; border-radius: 8px; background: var(--accent); color: #fff; font-weight: 600; font-size: 13px; }
  .primary:disabled { opacity: 0.6; }
  .danger { display: flex; align-items: center; gap: 6px; padding: 6px 10px; border-radius: 6px; font-size: 13px; color: var(--danger); }
  .danger:hover { background: var(--hover); }
  .muted { color: var(--muted); font-size: 12px; }
  .ellipsis { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>
