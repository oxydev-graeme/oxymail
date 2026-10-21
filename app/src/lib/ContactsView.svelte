<script>
  import { onMount } from "svelte";
  import { api, imageToIcon } from "./api.js";
  import Icon from "./Icon.svelte";

  /** Address book: search, view, edit. `oncompose(address)` writes to someone; `onsearch(q)` shows their mail. */
  let { oncompose, onsearch } = $props();

  let query = $state("");
  let people = $state([]);
  let current = $state(null);
  let editing = $state(null);
  let error = $state("");
  let timer;

  onMount(load);

  async function load() {
    people = await api.people(query).catch(() => []);
  }

  function onQuery() {
    clearTimeout(timer);
    timer = setTimeout(load, 150);
  }

  async function open(id) {
    editing = null;
    current = await api.person(id);
  }

  function startNew() {
    current = null;
    editing = { full_name: "", org: "", title: "", notes: "", birthday: "", photo: null, fields: [{ kind: "email", label: "", value: "" }, { kind: "phone", label: "", value: "" }] };
  }

  async function save() {
    error = "";
    try {
      const id = await api.savePerson({ ...editing, fields: editing.fields.filter((f) => f.value.trim()) });
      await load();
      await open(id);
    } catch (e) {
      error = String(e);
    }
  }

  async function remove() {
    if (!confirm(`Delete ${current.full_name} from your contacts?`)) return;
    await api.deletePerson(current.id);
    current = null;
    load();
  }

  async function pickPhoto(event) {
    const file = event.currentTarget.files?.[0];
    event.currentTarget.value = "";
    if (file) editing.photo = await imageToIcon(file, 192);
  }

  const initials = (name) =>
    name.split(/\s+/).filter(Boolean).slice(0, 2).map((w) => w[0].toUpperCase()).join("") || "?";
  const kinds = { email: "Email", phone: "Phone", url: "Website", address: "Address", im: "Chat" };
  const href = (f) => (f.kind === "url" ? (f.value.startsWith("http") ? f.value : `https://${f.value}`) : null);
</script>

<div class="contacts">
  <section class="list">
    <header>
      <input type="search" placeholder="Search contacts" bind:value={query} oninput={onQuery} />
      <button class="primary" onclick={startNew} title="New contact"><Icon name="plus" size={15} /></button>
    </header>
    <div class="count">{people.length.toLocaleString()} {people.length === 1 ? "contact" : "contacts"}</div>
    <ul>
      {#each people as p (p.id)}
        <li>
          <button class:active={current?.id === p.id} onclick={() => open(p.id)}>
            {#if p.photo}<img src={p.photo} alt="" class="avatar" />{:else}<span class="avatar">{initials(p.full_name)}</span>{/if}
            <span class="text">
              <span class="name">{p.full_name}</span>
              <span class="sub">{p.email ?? p.phone ?? p.org ?? ""}</span>
            </span>
          </button>
        </li>
      {:else}
        <li class="empty">{query ? "Nobody matches." : "No contacts yet. Import a .vcf in Settings → Import, or add one with +."}</li>
      {/each}
    </ul>
  </section>

  <section class="detail">
    {#if editing}
      <div class="form">
        <div class="photo-row">
          {#if editing.photo}<img src={editing.photo} alt="" class="big" />{:else}<span class="big">{initials(editing.full_name)}</span>{/if}
          <label class="upload">Photo… <input type="file" accept="image/*" onchange={pickPhoto} /></label>
          {#if editing.photo}<button class="link" onclick={() => (editing.photo = null)}>Remove</button>{/if}
        </div>
        <label><span>Name</span><input bind:value={editing.full_name} /></label>
        <div class="two">
          <label><span>Company</span><input bind:value={editing.org} /></label>
          <label><span>Job title</span><input bind:value={editing.title} /></label>
        </div>
        {#each editing.fields as f, i}
          <div class="field-row">
            <select bind:value={f.kind}>{#each Object.entries(kinds) as [k, label]}<option value={k}>{label}</option>{/each}</select>
            <input class="label-in" bind:value={f.label} placeholder="Label" />
            <input class="value-in" bind:value={f.value} placeholder={kinds[f.kind]} />
            <button class="icon" onclick={() => (editing.fields = editing.fields.filter((_, j) => j !== i))} title="Remove"><Icon name="x" size={14} /></button>
          </div>
        {/each}
        <button class="link" onclick={() => (editing.fields = [...editing.fields, { kind: "email", label: "", value: "" }])}>+ Add email, phone, website…</button>
        <label><span>Birthday</span><input bind:value={editing.birthday} placeholder="YYYY-MM-DD" /></label>
        <label><span>Notes</span><textarea rows="3" bind:value={editing.notes}></textarea></label>
        {#if error}<p class="error">{error}</p>{/if}
        <div class="actions">
          <button class="primary wide" onclick={save}>Save</button>
          <button onclick={() => (editing = null)}>Cancel</button>
        </div>
      </div>
    {:else if current}
      <div class="card">
        {#if current.photo}<img src={current.photo} alt="" class="big" />{:else}<span class="big">{initials(current.full_name)}</span>{/if}
        <h2>{current.full_name}</h2>
        {#if current.title || current.org}<p class="muted">{[current.title, current.org].filter(Boolean).join(" · ")}</p>{/if}
        <div class="actions">
          {#if current.fields.some((f) => f.kind === "email")}
            <button class="primary" onclick={() => oncompose(current.fields.find((f) => f.kind === "email").value, current.full_name)}><Icon name="pen" size={14} /> Email</button>
            <button onclick={() => onsearch(`from:${current.fields.find((f) => f.kind === "email").value}`)}><Icon name="search" size={14} /> Show emails</button>
          {/if}
          <button onclick={() => (editing = structuredClone($state.snapshot(current)))}>Edit</button>
          <button class="danger" onclick={remove}>Delete</button>
        </div>
        <dl>
          {#each current.fields as f}
            <dt>{f.label || kinds[f.kind] || f.kind}</dt>
            <dd>
              {#if f.kind === "email"}<button class="link" onclick={() => oncompose(f.value, current.full_name)}>{f.value}</button>
              {:else if href(f)}<button class="link" onclick={() => api.openUrl(href(f))}>{f.value}</button>
              {:else}{f.value}{/if}
            </dd>
          {/each}
          {#if current.birthday}<dt>Birthday</dt><dd>{current.birthday}</dd>{/if}
          {#if current.groups}<dt>Groups</dt><dd>{current.groups}</dd>{/if}
        </dl>
        {#if current.notes}<p class="notes">{current.notes}</p>{/if}
      </div>
    {:else}
      <div class="placeholder"><Icon name="contacts" size={36} /><p>Pick a contact, or add one with +.</p></div>
    {/if}
  </section>
</div>

<style>
  .contacts { display: grid; grid-template-columns: 340px minmax(0, 1fr); height: 100%; min-height: 0; background: var(--panel); }
  .list { display: flex; flex-direction: column; min-height: 0; border-right: 1px solid var(--border); }
  header { display: flex; gap: 8px; padding: 12px; }
  header input { flex: 1; padding: 7px 10px; border: 1px solid var(--border); border-radius: 8px; background: var(--bg); color: var(--text); outline: none; }
  header input:focus { border-color: var(--accent); }
  .count { padding: 0 16px 6px; font-size: 12px; color: var(--muted); }
  ul { flex: 1; min-height: 0; overflow-y: auto; margin: 0; padding: 0 8px 8px; list-style: none; }
  li button { display: flex; align-items: center; gap: 10px; width: 100%; padding: 7px 8px; border-radius: 8px; text-align: left; }
  li button:hover { background: var(--hover); }
  li button.active { background: var(--selected); }
  .avatar { display: grid; place-items: center; width: 34px; height: 34px; border-radius: 50%; background: var(--accent-soft); color: var(--accent); font-size: 13px; font-weight: 700; flex-shrink: 0; object-fit: cover; }
  .text { display: grid; min-width: 0; }
  .name { font-size: 13px; font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sub { font-size: 12px; color: var(--muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .empty { padding: 16px 8px; font-size: 13px; color: var(--muted); }
  .detail { min-height: 0; overflow-y: auto; padding: 28px 32px; }
  .card { max-width: 560px; user-select: text; }
  .big { display: grid; place-items: center; width: 84px; height: 84px; border-radius: 50%; background: var(--accent-soft); color: var(--accent); font-size: 30px; font-weight: 700; object-fit: cover; }
  h2 { margin: 12px 0 2px; font-size: 22px; }
  .muted { margin: 0; color: var(--muted); }
  .actions { display: flex; flex-wrap: wrap; gap: 8px; margin: 16px 0; }
  .actions button { display: flex; align-items: center; gap: 6px; padding: 6px 12px; border: 1px solid var(--border); border-radius: 8px; font-size: 13px; }
  .actions button:hover { border-color: var(--accent); color: var(--accent); }
  .actions .danger:hover { border-color: var(--danger); color: var(--danger); }
  .primary { background: var(--accent) !important; border-color: var(--accent) !important; color: #fff !important; font-weight: 600; }
  header .primary { display: grid; place-items: center; width: 34px; border-radius: 8px; }
  .wide { padding: 7px 22px !important; }
  dl { display: grid; grid-template-columns: 110px minmax(0, 1fr); gap: 8px 16px; margin: 0; font-size: 13px; }
  dt { color: var(--muted); text-transform: capitalize; }
  dd { margin: 0; overflow-wrap: anywhere; }
  .link { padding: 0; color: var(--accent); font-size: 13px; text-align: left; }
  .notes { white-space: pre-wrap; margin-top: 16px; padding-top: 12px; border-top: 1px solid var(--border); color: var(--muted); font-size: 13px; }
  .form { display: grid; gap: 12px; max-width: 560px; }
  .form label { display: grid; gap: 4px; font-size: 12px; color: var(--muted); }
  .form input, .form select, .form textarea { padding: 7px 9px; border: 1px solid var(--border); border-radius: 8px; background: var(--bg); color: var(--text); font: inherit; font-size: 13px; outline: none; }
  .form input:focus, .form textarea:focus, .form select:focus { border-color: var(--accent); }
  .two { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; }
  .field-row { display: grid; grid-template-columns: 110px 110px minmax(0, 1fr) 28px; gap: 6px; align-items: center; }
  .icon { display: grid; place-items: center; height: 28px; border-radius: 6px; color: var(--muted); }
  .icon:hover { background: var(--hover); color: var(--danger); }
  .photo-row { display: flex; align-items: center; gap: 14px; }
  .upload { position: relative; padding: 6px 12px; border: 1px solid var(--border); border-radius: 8px; font-size: 13px; color: var(--text) !important; cursor: pointer; }
  .upload input { position: absolute; inset: 0; opacity: 0; cursor: pointer; }
  .error { margin: 0; color: var(--danger); font-size: 13px; }
  .placeholder { display: grid; justify-items: center; gap: 6px; margin-top: 18vh; color: var(--muted); }
</style>
