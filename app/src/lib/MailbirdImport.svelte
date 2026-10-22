<script>
  import { onMount } from "svelte";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { api, listen } from "./api.js";

  /** Called with the result once the mail, contacts and calendars are in. */
  let { ondone = () => {} } = $props();

  let storePath = $state("");
  let outPath = $state("");
  let busy = $state(false);
  let progress = $state(null);
  let result = $state("");
  let failed = $state(false);

  onMount(() => {
    api.defaultExportFolder().then((p) => (outPath ||= p));
    const unlisten = listen("mailbird-progress", ({ payload }) => (progress = payload));
    return () => unlisten.then((f) => f());
  });

  async function choose(which) {
    const picked = await openDialog({ directory: true, defaultPath: which === "store" ? storePath || undefined : outPath || undefined }).catch(() => null);
    if (typeof picked !== "string") return;
    if (which === "store") storePath = picked;
    else outPath = picked;
  }

  const stageText = (p) => {
    if (!p) return "Starting…";
    if (p.stage === "export") return `Exporting ${p.done.toLocaleString()} of ${p.total.toLocaleString()} messages…`;
    if (p.stage === "import") return p.done ? `Indexing ${p.done.toLocaleString()} messages…` : "Indexing messages…";
    return "Importing contacts and calendars…";
  };

  async function run() {
    result = "";
    failed = false;
    if (!(await api.isMailbirdStore(storePath))) {
      failed = true;
      result = "That folder has no Store.db in it. Choose the Mailbird Store folder, or a copy of it.";
      return;
    }
    busy = true;
    progress = null;
    try {
      const r = await api.migrateMailbird(storePath, outPath);
      const e = r.export;
      const gaps = [];
      if (e.missing_bodies) gaps.push(`${e.missing_bodies.toLocaleString()} messages whose body Mailbird never downloaded`);
      if (e.missing_attachments) gaps.push(`${e.missing_attachments.toLocaleString()} attachments it never downloaded`);
      const blocked = [];
      if (e.blocked_attachments) blocked.push(`${e.blocked_attachments.toLocaleString()} attachments (marked in their messages)`);
      if (e.unwritable.length) blocked.push(`${e.unwritable.length.toLocaleString()} messages (listed in report.json in the export folder)`);
      result =
        `Brought over ${(e.messages - e.unwritable.length).toLocaleString()} messages from ${e.accounts} accounts, ` +
        `${r.pim.people.toLocaleString()} contacts and ${r.pim.events.toLocaleString()} calendar events.` +
        (gaps.length ? ` Mailbird's copy was missing ${gaps.join(" and ")}; those messages are marked.` : "") +
        (blocked.length ? ` Your antivirus blocked ${blocked.join(" and ")}. They are still in the Mailbird store.` : "");
      ondone(r);
    } catch (err) {
      failed = true;
      result = `Nothing was changed in your Mailbird store. ${err}`;
    } finally {
      busy = false;
    }
  }
</script>

<div class="mailbird">
  <label for="mb-store">Mailbird store</label>
  <div class="row">
    <input id="mb-store" bind:value={storePath} placeholder="A copy of your Mailbird Store folder" disabled={busy} />
    <button class="secondary" onclick={() => choose("store")} disabled={busy}>Choose…</button>
  </div>
  <p class="hint">
    On Windows it's <code>%LOCALAPPDATA%\Mailbird\Store</code>. Close Mailbird first, including the tray icon, and
    preferably use a copy. OxyMail only reads it.
  </p>

  <label for="mb-out">Save the export to</label>
  <div class="row">
    <input id="mb-out" bind:value={outPath} disabled={busy} />
    <button class="secondary" onclick={() => choose("out")} disabled={busy}>Choose…</button>
  </div>
  <p class="hint">
    An empty folder with room for about as much as the Mailbird store. Your mail is saved here as ordinary Maildir,
    contacts and calendar files, and OxyMail reads your archive from it, so keep this folder where it is.
  </p>

  <div class="actions">
    <button class="primary" onclick={run} disabled={busy || !storePath.trim() || !outPath.trim()}>
      {busy ? "Bringing over…" : "Bring over Mailbird"}
    </button>
    {#if busy}<span class="status">{stageText(progress)}</span>{/if}
  </div>
  {#if busy && progress?.stage === "export" && progress.total}
    <progress max={progress.total} value={progress.done}></progress>
  {/if}
  {#if result}<p class="result" class:failed>{result}</p>{/if}
</div>

<style>
  .mailbird { display: grid; gap: 6px; width: 100%; }
  label { font-size: 13px; font-weight: 600; margin-top: 6px; }
  .row { display: flex; gap: 8px; }
  .row input { flex: 1; min-width: 0; padding: 7px 10px; border: 1px solid var(--border); border-radius: var(--radius); background: var(--bg); }
  .hint { margin: 0 0 4px; font-size: 12px; color: var(--muted); line-height: 1.5; }
  code { font-size: 12px; user-select: text; }
  .actions { display: flex; gap: 12px; align-items: center; margin-top: 6px; }
  .primary { padding: 7px 14px; border-radius: var(--radius); background: var(--accent); color: #fff; font-weight: 600; font-size: 13px; }
  .primary:disabled { opacity: 0.6; cursor: default; }
  .secondary { padding: 7px 12px; border: 1px solid var(--border); border-radius: var(--radius); background: var(--panel); font-size: 13px; }
  .secondary:disabled { opacity: 0.6; cursor: default; }
  .status { font-size: 13px; color: var(--muted); }
  progress { width: 100%; height: 6px; accent-color: var(--accent); }
  .result { font-size: 13px; margin: 6px 0 0; line-height: 1.5; }
  .result.failed { color: var(--danger); }
</style>
