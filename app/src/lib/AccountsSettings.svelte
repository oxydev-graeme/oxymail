<script>
  import { api, accountLabel } from "./api.js";
  import Icon from "./Icon.svelte";
  import SignatureEditor from "./SignatureEditor.svelte";

  let { accounts, onchanged } = $props();

  const SWATCHES = ["#0f5e73", "#0e7490", "#2f6fed", "#7030a0", "#c0504d", "#c00000", "#e36c09", "#fe9a00", "#00b050", "#4f6128", "#244061", "#69708a"];

  let order = $state([]);
  let editing = $state(null);
  let draft = $state(null);
  let saving = $state(false);
  let dragIndex = $state(null);
  let overIndex = $state(null);
  let message = $state("");

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
    };
    message = "";
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

  async function remove() {
    const ok = confirm(
      `Remove ${editing.name} from OxyMail?\n\n` +
        "Its folders and messages disappear from the app and from search. Nothing is deleted: " +
        "importing the same archive again brings it back with your stars and folders.",
    );
    if (!ok) return;
    await api.removeAccount(editing.id);
    editing = null;
    onchanged();
  }
</script>

{#if editing}
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
    <p class="hint">Added to new messages and replies from this account once sending arrives.</p>
  </div>

  <div class="actions">
    <button class="primary" onclick={save} disabled={saving}>{saving ? "Saving…" : "Save"}</button>
    {#if message}<span class="muted">{message}</span>{/if}
    <span class="grow"></span>
    <button class="danger" onclick={remove}><Icon name="trash" size={15} /> Remove account</button>
  </div>
{:else}
  <h2>Accounts</h2>
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
        <span class="dot" style:background={account.color || "var(--faint)"}></span>
        <span class="names">
          <span class="ellipsis">{accountLabel(account)}</span>
          {#if account.label}<span class="ellipsis muted small">{account.name}</span>{/if}
        </span>
        {#if account.signature}<span class="tag" title="Has a signature">Signature</span>{/if}
        <span class="badge">{account.kind === "archive" ? "Archive" : account.kind}</span>
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
  .top { margin-bottom: 12px; }
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
  .dot { width: 10px; height: 10px; border-radius: 50%; flex-shrink: 0; }
  .names { display: grid; flex: 1; min-width: 0; }
  .small { font-size: 11px; }
  .badge, .tag { font-size: 11px; padding: 1px 8px; border-radius: 99px; flex-shrink: 0; }
  .badge { background: var(--accent-soft); color: var(--accent); }
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
