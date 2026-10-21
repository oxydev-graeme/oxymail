<script>
  import { onMount } from "svelte";
  import { api } from "./api.js";
  import Icon from "./Icon.svelte";

  /** Lists an account's folders to move messages into. */
  let { accountId, currentFolderId = null, count = 1, onpick, onclose } = $props();

  let folders = $state([]);
  let filter = $state("");
  let input;

  let shown = $derived(
    folders.filter((f) => f.id !== currentFolderId && f.name.toLowerCase().includes(filter.trim().toLowerCase())),
  );

  onMount(async () => {
    folders = await api.folders(accountId);
    input?.focus();
  });

  const icon = (role) =>
    ({ inbox: "inbox", sent: "send", drafts: "draft", archive: "archive", spam: "spam", trash: "trash" })[role] ?? "folder";

  function onKey(event) {
    if (event.key === "Escape") onclose();
    if (event.key === "Enter" && shown.length) onpick(shown[0]);
  }
</script>

<div class="backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="picker" role="dialog" aria-label="Move to folder">
    <header>
      <span>Move {count === 1 ? "message" : `${count} messages`} to…</span>
      <button class="icon" onclick={onclose} title="Close"><Icon name="x" size={16} /></button>
    </header>
    <input bind:this={input} bind:value={filter} placeholder="Filter folders" onkeydown={onKey} />
    <ul>
      {#each shown as f (f.id)}
        <li>
          <button onclick={() => onpick(f)}>
            <Icon name={icon(f.role)} size={15} />
            <span class="ellipsis">{f.name}</span>
          </button>
        </li>
      {:else}
        <li class="empty">No folders match.</li>
      {/each}
    </ul>
  </div>
</div>

<style>
  .backdrop { position: fixed; inset: 0; display: grid; place-items: center; background: rgb(10 14 20 / 0.35); z-index: 26; }
  .picker {
    display: flex;
    flex-direction: column;
    width: min(380px, 92vw);
    max-height: min(520px, 80vh);
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 12px;
    box-shadow: 0 20px 60px rgb(0 0 0 / 0.3);
    overflow: hidden;
  }
  header { display: flex; align-items: center; justify-content: space-between; padding: 12px 12px 8px 16px; font-weight: 650; }
  .icon { display: grid; place-items: center; padding: 4px; border-radius: 6px; color: var(--muted); }
  .icon:hover { background: var(--hover); color: var(--text); }
  input { margin: 0 12px 8px; padding: 7px 10px; border: 1px solid var(--border); border-radius: 8px; background: var(--bg); color: var(--text); outline: none; }
  input:focus { border-color: var(--accent); }
  ul { flex: 1; min-height: 0; overflow-y: auto; margin: 0; padding: 0 8px 8px; list-style: none; }
  li button { display: flex; align-items: center; gap: 9px; width: 100%; padding: 7px 8px; border-radius: 6px; font-size: 13px; text-align: left; color: var(--muted); }
  li button:hover { background: var(--selected); color: var(--text); }
  .empty { padding: 12px 8px; color: var(--muted); font-size: 13px; }
  .ellipsis { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>
