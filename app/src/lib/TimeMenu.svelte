<script>
  import { untrack } from "svelte";
  import Icon from "./Icon.svelte";

  /**
   * Pick a moment: quick choices plus any date and time. Used for snooze and send later.
   * The quick choices follow the weekly schedule in Settings → Snoozing.
   */
  let { title, onpick, onclose, settings = {} } = $props();

  const now = new Date();
  // Read once: the menu is only open for a moment, and its choices shouldn't shift under the pointer.
  const schedule = untrack(() => ({ ...settings }));
  const time = (key, fallback) => {
    const [h, m] = String(schedule[key] || fallback).split(":").map(Number);
    return [Number.isFinite(h) ? h : 8, Number.isFinite(m) ? m : 0];
  };
  const [dayH, dayM] = time("day_starts", "08:00");
  const [endH, endM] = time("workday_ends", "18:00");
  const [weekendH, weekendM] = time("weekend_starts", "09:00");
  const weekendDay = Number(schedule.weekend_starts_day ?? 6);
  const weekDay = Number(schedule.week_starts_day ?? 1);
  const laterHours = Number(schedule.later_today_hours) || 3;
  const somedayMonths = Number(schedule.someday_months) || 3;

  const at = (days, hours, minutes = 0) => {
    const d = new Date(now);
    d.setDate(d.getDate() + days);
    d.setHours(hours, minutes, 0, 0);
    return d;
  };
  const daysUntil = (weekday) => ((weekday - now.getDay() + 7) % 7) || 7;
  const someday = new Date(now);
  someday.setMonth(someday.getMonth() + somedayMonths);
  someday.setHours(dayH, dayM, 0, 0);
  const later = new Date(now.getTime() + laterHours * 3600 * 1000);
  const evening = at(0, endH, endM);
  const weekend = at(daysUntil(weekendDay), weekendH, weekendM);
  // "This weekend" only while the weekend is still ahead this week.
  const weekendAhead = daysUntil(weekendDay) < daysUntil(weekDay) && now.getDay() !== weekendDay;

  const options = [
    later.toDateString() === now.toDateString() && { label: "Later today", when: later },
    // Once the working day is at least an hour off.
    evening.getTime() - now.getTime() >= 3600 * 1000 && { label: "This evening", when: evening },
    { label: "Tomorrow", when: at(1, dayH, dayM) },
    weekendAhead && { label: "This weekend", when: weekend },
    { label: "Next week", when: at(daysUntil(weekDay), dayH, dayM) },
    { label: "Someday", when: someday },
  ].filter(Boolean);

  const pad = (n) => String(n).padStart(2, "0");
  const local = (d) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
  let custom = $state(local(at(1, dayH, dayM)));

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
