<script>
  import { api } from "./api.js";

  /** A comma-separated address field that suggests contacts for the address being typed. */
  let { value = $bindable(""), id = undefined, placeholder = "" } = $props();

  let suggestions = $state([]);
  let active = $state(0);
  let timer;

  /** The address currently being typed: everything after the last comma or semicolon. */
  const current = () => value.split(/[,;]/).pop().trim();

  function onInput() {
    clearTimeout(timer);
    const q = current();
    if (q.length < 2) return (suggestions = []);
    timer = setTimeout(async () => {
      suggestions = await api.suggestContacts(q).catch(() => []);
      active = 0;
    }, 120);
  }

  function choose(s) {
    const head = value.slice(0, value.length - value.split(/[,;]/).pop().length);
    const entry = s.name ? `${s.name.replace(/[<>",]/g, "")} <${s.email}>` : s.email;
    value = `${head}${head && !head.endsWith(" ") ? " " : ""}${entry}, `;
    suggestions = [];
  }

  function onKey(event) {
    if (!suggestions.length) return;
    if (event.key === "ArrowDown") active = (active + 1) % suggestions.length;
    else if (event.key === "ArrowUp") active = (active - 1 + suggestions.length) % suggestions.length;
    else if (event.key === "Enter" || event.key === "Tab") choose(suggestions[active]);
    else if (event.key === "Escape") suggestions = [];
    else return;
    event.preventDefault();
    event.stopPropagation();
  }
</script>

<div class="wrap">
  <input {id} {placeholder} bind:value oninput={onInput} onkeydown={onKey} onblur={() => setTimeout(() => (suggestions = []), 150)} autocomplete="off" />
  {#if suggestions.length}
    <ul class="menu" role="listbox">
      {#each suggestions as s, i (s.email)}
        <li role="option" aria-selected={i === active}>
          <button type="button" class:active={i === active} onmousedown={(e) => { e.preventDefault(); choose(s); }}>
            <span class="name">{s.name ?? s.email}</span>
            {#if s.name}<span class="email">{s.email}</span>{/if}
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .wrap { position: relative; flex: 1; min-width: 0; }
  input { width: 100%; padding: 4px 0; border: 0; background: none; color: var(--text); outline: none; font: inherit; }
  .menu {
    position: absolute;
    z-index: 5;
    top: calc(100% + 4px);
    left: -8px;
    width: min(460px, 100%);
    margin: 0;
    padding: 4px;
    list-style: none;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: 0 8px 24px rgb(0 0 0 / 0.18);
  }
  .menu button { display: flex; align-items: baseline; gap: 8px; width: 100%; padding: 6px 8px; border-radius: 6px; text-align: left; font-size: 13px; }
  .menu button.active, .menu button:hover { background: var(--selected); }
  .name { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .email { color: var(--muted); font-size: 12px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
</style>
