<script>
  import { onMount } from "svelte";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { api, accountLabel } from "./api.js";
  import Icon from "./Icon.svelte";

  /** Settings → Filters: rules run on new Inbox mail as it arrives, like Mailbird's. */
  let { accounts } = $props();

  const FIELDS = [["subject", "Subject"], ["from", "From"], ["to", "To"], ["any", "Subject, From or To"]];
  const ACTIONS = [["spam", "Move to Spam"], ["archive", "Archive"], ["trash", "Delete"], ["read", "Mark as read"], ["star", "Star"]];
  const blank = () => ({ id: 0, account_id: null, field: "subject", value: "", action: "spam", enabled: true });

  let filters = $state([]);
  let draft = $state(blank());
  let error = $state("");
  let mailbirdPath = $state("");
  let importing = $state(false);
  let importResult = $state("");

  const live = $derived(accounts.filter((a) => a.kind === "imap"));
  const accountName = (id) => (id == null ? "All accounts" : accountLabel(accounts.find((a) => a.id === id) ?? { name: "Removed account" }));

  async function load() {
    try {
      filters = await api.filters();
    } catch (e) {
      error = String(e);
    }
  }
  onMount(load);

  async function save(f) {
    error = "";
    try {
      await api.saveFilter($state.snapshot(f));
      await load();
      return true;
    } catch (e) {
      error = String(e);
      return false;
    }
  }

  async function add() {
    if (await save(draft)) draft = blank();
  }

  async function remove(f) {
    await api.deleteFilter(f.id).catch((e) => (error = String(e)));
    load();
  }

  async function pickMailbird() {
    const picked = await openDialog({ directory: true, defaultPath: mailbirdPath || undefined }).catch(() => null);
    if (typeof picked === "string") mailbirdPath = picked;
  }

  async function importMailbird() {
    importing = true;
    importResult = "";
    try {
      const r = await api.importMailbirdFilters(mailbirdPath);
      importResult = `Added ${r.added} filters` + (r.skipped ? `; ${r.skipped} skipped (already here, for an account OxyMail doesn't have, or of a kind it can't read).` : ".");
      load();
    } catch (e) {
      importResult = `Couldn't read the filters: ${e}`;
    } finally {
      importing = false;
    }
  }
</script>

<h2>Filters</h2>
<p class="hint lead">
  Run on new mail as it arrives in an Inbox, from the top down; the first that matches decides. Text matches anywhere, in
  any case, and <kbd>*</kbd> stands for anything. Mail a filter moves away or marks read gets no notification.
</p>

<div class="list">
  {#each filters as f (f.id)}
    <div class="filter" class:off={!f.enabled}>
      <input type="checkbox" title={f.enabled ? "On" : "Off"} checked={f.enabled} onchange={(e) => save({ ...f, enabled: e.currentTarget.checked })} />
      <div class="what">
        <div><span class="muted">{FIELDS.find(([v]) => v === f.field)?.[1]} contains</span> <b>{f.value}</b></div>
        <div class="muted">→ {ACTIONS.find(([v]) => v === f.action)?.[1]} · {accountName(f.account_id)}</div>
      </div>
      <button class="icon" title="Delete this filter" onclick={() => remove(f)}><Icon name="trash" size={15} /></button>
    </div>
  {:else}
    <p class="empty">No filters yet.</p>
  {/each}
</div>

<div class="add">
  <div class="label">New filter</div>
  <div class="row">
    <select bind:value={draft.field}>{#each FIELDS as [v, l]}<option value={v}>{l}</option>{/each}</select>
    <span class="muted">contains</span>
    <input bind:value={draft.value} placeholder="Text to look for" onkeydown={(e) => e.key === "Enter" && add()} />
  </div>
  <div class="row">
    <select bind:value={draft.action}>{#each ACTIONS as [v, l]}<option value={v}>{l}</option>{/each}</select>
    <span class="muted">in</span>
    <select bind:value={draft.account_id}>
      <option value={null}>All accounts</option>
      {#each live as a (a.id)}<option value={a.id}>{accountLabel(a)}</option>{/each}
    </select>
    <button class="primary" onclick={add} disabled={!draft.value.trim()}>Add</button>
  </div>
  {#if error}<p class="error">{error}</p>{/if}
</div>

<div class="add">
  <div class="label">Bring over Mailbird's filters</div>
  <div class="row">
    <input bind:value={mailbirdPath} placeholder="Mailbird's Store folder, or an export folder" />
    <button onclick={pickMailbird}>Choose…</button>
    <button class="primary" onclick={importMailbird} disabled={importing || !mailbirdPath.trim()}>{importing ? "Reading…" : "Import"}</button>
  </div>
  <p class="hint">Mailbird is only read. Filters already here are skipped, so this is safe to run again.</p>
  {#if importResult}<p class="result">{importResult}</p>{/if}
</div>

<style>
  h2 { margin: 0 0 18px; font-size: 17px; }
  .lead { margin: -6px 0 16px; }
  .list { display: flex; flex-direction: column; border: 1px solid var(--border); border-radius: 10px; max-height: 300px; overflow-y: auto; }
  .filter { display: flex; align-items: center; gap: 12px; padding: 9px 12px; border-bottom: 1px solid var(--border); font-size: 13px; }
  .filter:last-child { border-bottom: 0; }
  .filter.off .what { opacity: 0.5; }
  .filter input { width: 16px; height: 16px; accent-color: var(--accent); flex-shrink: 0; }
  .what { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px; overflow-wrap: anywhere; }
  .muted { color: var(--muted); font-size: 12px; }
  .icon { display: grid; place-items: center; padding: 6px; border-radius: 6px; color: var(--muted); }
  .icon:hover { background: var(--hover); color: var(--danger); }
  .empty { margin: 0; padding: 14px; color: var(--muted); font-size: 13px; }
  .add { margin-top: 22px; }
  .label { font-size: 13px; font-weight: 600; margin-bottom: 8px; }
  .row { display: flex; align-items: center; gap: 8px; margin-bottom: 8px; }
  .row input { flex: 1; min-width: 0; }
  input:not([type]), select { padding: 6px 8px; border: 1px solid var(--border); border-radius: 8px; background: var(--bg); color: var(--text); font: inherit; font-size: 13px; color-scheme: inherit; }
  button:not(.icon) { padding: 6px 12px; border: 1px solid var(--border); border-radius: 8px; font-size: 13px; white-space: nowrap; }
  .primary { background: var(--accent); border-color: var(--accent) !important; color: #fff; font-weight: 600; }
  .primary:disabled { opacity: 0.5; }
  .hint { margin: 6px 0 0; font-size: 12px; color: var(--muted); line-height: 1.5; }
  .result { font-size: 13px; margin: 8px 0 0; }
  .error { color: var(--danger); font-size: 13px; margin: 4px 0 0; }
  kbd { padding: 0 4px; border: 1px solid var(--border); border-radius: 4px; font-size: 11px; }
</style>
