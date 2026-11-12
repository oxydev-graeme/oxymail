<script>
  import { untrack } from "svelte";
  import { api } from "./api.js";
  import Icon from "./Icon.svelte";

  /**
   * "Make a filter from this message": the sender's address or the subject, filled in, with
   * an action and whether to run it on the mail already in an Inbox.
   */
  let { message, account, ondone, onclose } = $props();

  const ACTIONS = [["spam", "Move to Spam"], ["archive", "Archive"], ["trash", "Delete"], ["read", "Mark as read"], ["star", "Star"]];

  // The starting values, taken once: the fields are the user's to edit from here.
  const { subject: original = "", from_addr: address = "" } = untrack(() => message);
  // Subjects like "RE: Invoice 123 for May" make poor filters; drop the reply prefix.
  const subject = (original ?? "").replace(/^\s*((re|fwd?|aw|sv)\s*:\s*)+/i, "").trim();
  let field = $state(address ? "from" : "subject");
  let fromValue = $state(address ?? "");
  let subjectValue = $state(subject);
  let action = $state("archive");
  let thisAccount = $state(true);
  let applyExisting = $state(true);
  let busy = $state(false);
  let error = $state("");

  const value = $derived(field === "from" ? fromValue : subjectValue);

  async function save() {
    busy = true;
    error = "";
    try {
      const id = await api.saveFilter({ id: 0, account_id: thisAccount ? account.id : null, field, value: value.trim(), action, enabled: true });
      const ran = applyExisting ? await api.runFilter(id) : 0;
      ondone(ran);
    } catch (e) {
      error = String(e);
      busy = false;
    }
  }

  const onKey = (event) => event.key === "Escape" && onclose();
</script>

<svelte:window onkeydown={onKey} />

<div class="backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog" role="dialog" aria-label="New filter">
    <header>
      <span>New filter</span>
      <button class="icon" onclick={onclose} title="Close"><Icon name="x" size={16} /></button>
    </header>
    <div class="body">
      <p class="muted">For new mail like this when it arrives in an Inbox:</p>
      {#if message.from_addr}
        <label class="choice">
          <input type="radio" bind:group={field} value="from" />
          <span>From contains</span>
          <input class="text" bind:value={fromValue} onfocus={() => (field = "from")} />
        </label>
      {/if}
      <label class="choice">
        <input type="radio" bind:group={field} value="subject" />
        <span>Subject contains</span>
        <input class="text" bind:value={subjectValue} onfocus={() => (field = "subject")} />
      </label>
      <div class="row">
        <span class="muted">then</span>
        <select bind:value={action}>{#each ACTIONS as [v, l]}<option value={v}>{l}</option>{/each}</select>
      </div>
      <label class="check"><input type="checkbox" bind:checked={thisAccount} /> Only in {account?.name ?? "this account"}</label>
      <label class="check"><input type="checkbox" bind:checked={applyExisting} /> Also apply it to mail already in an Inbox</label>
      <p class="muted small">Text matches anywhere, in any case; <kbd>*</kbd> stands for anything. Change or remove filters in Settings → Filters.</p>
      {#if error}<p class="error">{error}</p>{/if}
    </div>
    <footer>
      <button onclick={onclose}>Cancel</button>
      <button class="primary" onclick={save} disabled={busy || !value.trim()}>{busy ? "Saving…" : "Create filter"}</button>
    </footer>
  </div>
</div>

<style>
  .backdrop { position: fixed; inset: 0; display: grid; place-items: center; background: rgb(10 14 20 / 0.35); z-index: 26; }
  .dialog {
    width: min(460px, 92vw);
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 12px;
    box-shadow: 0 20px 60px rgb(0 0 0 / 0.3);
    overflow: hidden;
  }
  header { display: flex; align-items: center; justify-content: space-between; padding: 12px 12px 4px 18px; font-weight: 650; }
  .icon { display: grid; place-items: center; padding: 4px; border-radius: 6px; color: var(--muted); }
  .icon:hover { background: var(--hover); color: var(--text); }
  .body { display: flex; flex-direction: column; gap: 10px; padding: 6px 18px 4px; font-size: 13px; }
  .muted { margin: 0; color: var(--muted); }
  .small { font-size: 12px; line-height: 1.5; }
  .choice { display: grid; grid-template-columns: auto auto 1fr; align-items: center; gap: 8px; }
  .row { display: flex; align-items: center; gap: 8px; }
  .text, select { min-width: 0; padding: 6px 8px; border: 1px solid var(--border); border-radius: 8px; background: var(--bg); color: var(--text); font: inherit; color-scheme: inherit; }
  .text:focus, select:focus { outline: none; border-color: var(--accent); }
  input[type="radio"], .check input { width: 15px; height: 15px; accent-color: var(--accent); margin: 0; }
  .check { display: flex; align-items: center; gap: 8px; color: var(--muted); }
  kbd { padding: 0 4px; border: 1px solid var(--border); border-radius: 4px; font-size: 11px; }
  .error { margin: 0; color: var(--danger); }
  footer { display: flex; justify-content: flex-end; gap: 8px; padding: 14px 18px 16px; }
  footer button { padding: 7px 14px; border: 1px solid var(--border); border-radius: 8px; font-size: 13px; }
  footer button:hover { background: var(--hover); }
  .primary { background: var(--accent); border-color: var(--accent); color: #fff; font-weight: 600; }
  .primary:hover { background: var(--accent) !important; filter: brightness(1.08); }
  .primary:disabled { opacity: 0.5; }
</style>
