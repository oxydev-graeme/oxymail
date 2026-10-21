<script>
  import Icon from "./Icon.svelte";

  /** Pick a moment: quick choices plus any date and time. Used for snooze and send later. */
  let { title, onpick, onclose } = $props();

  const now = new Date();
  const at = (days, hours, minutes = 0) => {
    const d = new Date(now);
    d.setDate(d.getDate() + days);
    d.setHours(hours, minutes, 0, 0);
    return d;
  };
  const daysUntil = (weekday) => ((weekday - now.getDay() + 7) % 7) || 7;

  const options = [
    now.getHours() < 18 && { label: "Later today", when: new Date(now.getTime() + 3 * 3600 * 1000) },
    now.getHours() < 17 && { label: "This evening", when: at(0, 18) },
    { label: "Tomorrow morning", when: at(1, 8) },
    now.getDay() >= 1 && now.getDay() <= 4 && { label: "This weekend", when: at(daysUntil(6), 9) },
    { label: "Next week", when: at(daysUntil(1), 8) },
  ].filter(Boolean);

  const pad = (n) => String(n).padStart(2, "0");
  const local = (d) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
  let custom = $state(local(at(1, 9)));

  const describe = (d) =>
    d.toLocaleString(undefined, { weekday: "short", day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });

  function pick(d) {
    if (d.getTime() <= Date.now()) return;
    onpick(Math.floor(d.getTime() / 1000), d);
  }
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && onclose()} />

<div class="backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="menu" role="dialog" aria-label={title}>
    <header>
      <span>{title}</span>
      <button class="icon" onclick={onclose} title="Close"><Icon name="x" size={16} /></button>
    </header>
    <ul>
      {#each options as o (o.label)}
        <li>
          <button onclick={() => pick(o.when)}>
            <span>{o.label}</span>
            <span class="when">{describe(o.when)}</span>
          </button>
        </li>
      {/each}
    </ul>
    <div class="custom">
      <input type="datetime-local" bind:value={custom} min={local(now)} />
      <button class="primary" onclick={() => pick(new Date(custom))} disabled={new Date(custom).getTime() <= Date.now()}>Set</button>
    </div>
  </div>
</div>

<style>
  .backdrop { position: fixed; inset: 0; display: grid; place-items: center; background: rgb(10 14 20 / 0.35); z-index: 27; }
  .menu { width: min(380px, 92vw); background: var(--panel); border: 1px solid var(--border); border-radius: 12px; box-shadow: 0 20px 60px rgb(0 0 0 / 0.3); overflow: hidden; }
  header { display: flex; align-items: center; justify-content: space-between; padding: 12px 12px 6px 16px; font-weight: 650; }
  .icon { display: grid; place-items: center; padding: 4px; border-radius: 6px; color: var(--muted); }
  .icon:hover { background: var(--hover); color: var(--text); }
  ul { margin: 0; padding: 4px 8px; list-style: none; }
  li button { display: flex; justify-content: space-between; gap: 12px; width: 100%; padding: 8px 10px; border-radius: 6px; font-size: 13px; text-align: left; }
  li button:hover { background: var(--selected); }
  .when { color: var(--muted); font-size: 12px; white-space: nowrap; }
  .custom { display: flex; gap: 8px; padding: 10px 16px 14px; border-top: 1px solid var(--border); }
  .custom input { flex: 1; min-width: 0; padding: 6px 8px; border: 1px solid var(--border); border-radius: 8px; background: var(--bg); color: var(--text); font: inherit; font-size: 13px; color-scheme: inherit; }
  .primary { padding: 6px 14px; border-radius: 8px; background: var(--accent); color: #fff; font-weight: 600; font-size: 13px; }
  .primary:disabled { opacity: 0.5; cursor: default; }
</style>
