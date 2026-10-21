<script>
  import { api } from "./api.js";
  import Icon from "./Icon.svelte";
  import EventEditor from "./EventEditor.svelte";

  /** Month view with a day agenda. `calendars` comes from the sidebar list. */
  let { calendars, onchanged } = $props();

  const today = new Date();
  today.setHours(0, 0, 0, 0);
  let cursor = $state(new Date(today.getFullYear(), today.getMonth(), 1));
  let selectedDay = $state(new Date(today));
  let occurrences = $state([]);
  let openEvent = $state(null);
  let editing = $state(null);

  const dayKey = (d) => `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
  const sameDay = (a, b) => dayKey(a) === dayKey(b);

  /** Monday-first 6-week grid around the month. */
  let grid = $derived.by(() => {
    const first = new Date(cursor);
    const offset = (first.getDay() + 6) % 7;
    const start = new Date(first.getFullYear(), first.getMonth(), 1 - offset);
    return Array.from({ length: 42 }, (_, i) => new Date(start.getFullYear(), start.getMonth(), start.getDate() + i));
  });

  /** Local days each occurrence touches. All-day dates are calendar dates, so read them in UTC. */
  function daysOf(o) {
    const out = [];
    const toLocal = (ts) => {
      const d = new Date(ts * 1000);
      return o.all_day ? new Date(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate()) : new Date(d.getFullYear(), d.getMonth(), d.getDate());
    };
    const first = toLocal(o.start);
    const last = toLocal(o.all_day ? o.end - 1 : Math.max(o.start, o.end - 1));
    for (let d = new Date(first); d <= last && out.length < 62; d.setDate(d.getDate() + 1)) out.push(dayKey(d));
    return out;
  }

  let byDay = $derived.by(() => {
    const map = {};
    for (const o of occurrences) for (const k of daysOf(o)) (map[k] ??= []).push(o);
    return map;
  });

  let dayEvents = $derived(byDay[dayKey(selectedDay)] ?? []);

  $effect(() => {
    // Reload when the month or the calendar list (visibility, colours) changes.
    calendars;
    load(grid[0], grid[41]);
  });

  async function load(from, to) {
    const end = new Date(to);
    end.setDate(end.getDate() + 1);
    occurrences = await api.occurrences(Math.floor(from.getTime() / 1000) - 86400, Math.floor(end.getTime() / 1000) + 86400).catch(() => []);
  }

  function move(months) {
    cursor = new Date(cursor.getFullYear(), cursor.getMonth() + months, 1);
  }

  function goToday() {
    cursor = new Date(today.getFullYear(), today.getMonth(), 1);
    selectedDay = new Date(today);
  }

  async function show(o) {
    openEvent = { ...(await api.event(o.event_id)), occurrence: o };
  }

  function newEvent(day = selectedDay) {
    if (!calendars.length) return;
    const start = new Date(day);
    start.setHours(9, 0, 0, 0);
    editing = { calendar_id: calendars.find((c) => c.visible)?.id ?? calendars[0].id, start: start.getTime() / 1000, end: start.getTime() / 1000 + 3600 };
  }

  async function remove(ev) {
    const what = ev.rrule ? "Delete every occurrence of this repeating event?" : "Delete this event?";
    if (!confirm(what)) return;
    await api.deleteEvent(ev.id);
    openEvent = null;
    onchanged();
  }

  const time = (ts) => new Date(ts * 1000).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
  const range = (o) => (o.all_day ? "All day" : `${time(o.start)} – ${time(o.end)}`);
  const monthTitle = (d) => d.toLocaleDateString(undefined, { month: "long", year: "numeric" });
  const weekdays = Array.from({ length: 7 }, (_, i) => new Date(2024, 0, 1 + i).toLocaleDateString(undefined, { weekday: "short" }));
  const repeats = (rule) =>
    ({ DAILY: "Repeats daily", WEEKLY: "Repeats weekly", MONTHLY: "Repeats monthly", YEARLY: "Repeats yearly" })[rule?.match(/FREQ=(\w+)/)?.[1]] ?? "Repeats";
</script>

<div class="cal-wrap">
<div class="calendar">
  <section class="month">
    <header>
      <h2>{monthTitle(cursor)}</h2>
      <div class="nav">
        <button onclick={goToday}>Today</button>
        <button class="icon" onclick={() => move(-1)} title="Previous month"><Icon name="chevronLeft" size={16} /></button>
        <button class="icon" onclick={() => move(1)} title="Next month"><Icon name="chevronRight" size={16} /></button>
        <button class="primary" onclick={() => newEvent()} disabled={!calendars.length} title="New event"><Icon name="plus" size={14} /><span class="label">New event</span></button>
      </div>
    </header>
    <div class="weekdays">{#each weekdays as w}<span>{w}</span>{/each}</div>
    <div class="grid">
      {#each grid as day (dayKey(day))}
        {@const events = byDay[dayKey(day)] ?? []}
        <div
          class="day"
          class:other={day.getMonth() !== cursor.getMonth()}
          class:today={sameDay(day, today)}
          class:selected={sameDay(day, selectedDay)}
          role="button"
          tabindex="0"
          onclick={() => (selectedDay = day)}
          ondblclick={() => newEvent(day)}
          onkeydown={(e) => e.key === "Enter" && (selectedDay = day)}
        >
          <span class="num">{day.getDate()}</span>
          {#each events.slice(0, 3) as o (o.event_id + ":" + o.start)}
            <button class="chip" class:allday={o.all_day} class:cancelled={o.cancelled} style:--c={o.color} onclick={(e) => { e.stopPropagation(); selectedDay = day; show(o); }} title={o.summary}>
              {#if !o.all_day}<span class="t">{time(o.start)}</span>{/if}<span class="s">{o.summary || "(no title)"}</span>
            </button>
          {/each}
          {#if events.length > 3}<span class="more">+{events.length - 3} more</span>{/if}
        </div>
      {/each}
    </div>
  </section>

  <aside class="agenda">
    <h3>{selectedDay.toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "long" })}</h3>
    {#each dayEvents as o (o.event_id + ":" + o.start)}
      <button class="item" class:active={openEvent?.occurrence === o} style:--c={o.color} onclick={() => show(o)}>
        <span class="bar"></span>
        <span class="info">
          <span class="s" class:cancelled={o.cancelled}>{o.summary || "(no title)"}</span>
          <span class="when">{range(o)}{#if o.location}{" · "}{o.location}{/if}</span>
        </span>
      </button>
    {:else}
      <p class="empty">Nothing on this day. Double-click a day to add an event.</p>
    {/each}

    {#if openEvent}
      <div class="detail" style:--c={calendars.find((c) => c.id === openEvent.calendar_id)?.color}>
        <h4>{openEvent.summary || "(no title)"}</h4>
        <p class="when">{range(openEvent.occurrence)}{#if openEvent.rrule}{" · "}{repeats(openEvent.rrule)}{/if}</p>
        <p class="cal"><span class="dot"></span>{calendars.find((c) => c.id === openEvent.calendar_id)?.name}</p>
        {#if openEvent.location}<p><b>Where</b> {openEvent.location}</p>{/if}
        {#if openEvent.organizer}<p><b>Organiser</b> {openEvent.organizer}</p>{/if}
        {#if openEvent.attendees.length}
          <p><b>Guests</b></p>
          <ul>{#each openEvent.attendees as a}<li>{a.name || a.email}{#if a.status}<span class="muted">{" · "}{a.status.toLowerCase().replace("-", " ")}</span>{/if}</li>{/each}</ul>
        {/if}
        {#if openEvent.description}<p class="notes">{openEvent.description}</p>{/if}
        <div class="actions">
          <button onclick={() => (editing = openEvent)}>Edit</button>
          <button class="danger" onclick={() => remove(openEvent)}>Delete</button>
        </div>
      </div>
    {/if}
  </aside>
</div>

</div>

{#if editing}
  <EventEditor
    event={editing}
    {calendars}
    onclose={() => (editing = null)}
    onsaved={async (id) => {
      editing = null;
      onchanged();
      openEvent = openEvent ? { ...(await api.event(id)), occurrence: openEvent.occurrence } : null;
    }}
  />
{/if}

<style>
  .cal-wrap { height: 100%; min-height: 0; container: calendar / inline-size; }
  .calendar { display: grid; grid-template-columns: minmax(0, 1fr) 300px; height: 100%; min-height: 0; background: var(--panel); }
  h2 { white-space: nowrap; }
  @container calendar (max-width: 820px) {
    .calendar { grid-template-columns: minmax(0, 1fr) 220px; }
    .nav .label { display: none; }
    h2 { font-size: 16px; }
  }
  .month { display: flex; flex-direction: column; min-width: 0; min-height: 0; border-right: 1px solid var(--border); }
  header { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 12px 16px; border-bottom: 1px solid var(--border); }
  h2 { margin: 0; font-size: 18px; }
  .nav { display: flex; align-items: center; gap: 6px; }
  .nav button { display: flex; align-items: center; gap: 6px; padding: 6px 12px; border: 1px solid var(--border); border-radius: 8px; font-size: 13px; }
  .nav button.icon { padding: 6px 8px; }
  .nav button:hover:not(:disabled) { border-color: var(--accent); color: var(--accent); }
  .nav .primary { background: var(--accent); border-color: var(--accent); color: #fff; font-weight: 600; }
  .nav .primary:hover { color: #fff !important; background: var(--brand); }
  .weekdays { display: grid; grid-template-columns: repeat(7, minmax(0, 1fr)); padding: 6px 0; border-bottom: 1px solid var(--border); }
  .weekdays span { text-align: center; font-size: 11px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.05em; color: var(--faint); }
  .grid { flex: 1; display: grid; grid-template-columns: repeat(7, minmax(0, 1fr)); grid-template-rows: repeat(6, minmax(0, 1fr)); min-height: 0; }
  .day { display: flex; flex-direction: column; gap: 2px; min-width: 0; min-height: 0; padding: 4px 5px; border-right: 1px solid var(--border); border-bottom: 1px solid var(--border); overflow: hidden; cursor: pointer; outline: none; }
  .day:nth-child(7n) { border-right: 0; }
  .day:hover { background: var(--bg); }
  .day.other { background: color-mix(in srgb, var(--bg) 60%, transparent); }
  .day.other .num { color: var(--faint); }
  .day.selected { box-shadow: inset 0 0 0 2px var(--accent); }
  .num { align-self: flex-end; min-width: 22px; padding: 1px 5px; border-radius: 99px; font-size: 12px; font-weight: 600; text-align: center; }
  .today .num { background: var(--accent); color: #fff; }
  .chip { display: flex; gap: 4px; min-width: 0; padding: 1px 5px; border-radius: 4px; border-left: 3px solid var(--c); background: color-mix(in srgb, var(--c) 14%, transparent); font-size: 11px; text-align: left; white-space: nowrap; }
  .chip.allday { background: var(--c); color: #fff; border-left-color: transparent; }
  .chip .t { color: var(--muted); flex-shrink: 0; }
  .chip.allday .t { color: #fff; }
  .chip .s { overflow: hidden; text-overflow: ellipsis; }
  .cancelled .s, .s.cancelled { text-decoration: line-through; opacity: 0.7; }
  .more { font-size: 11px; color: var(--muted); padding-left: 4px; }

  .agenda { display: flex; flex-direction: column; gap: 6px; min-height: 0; overflow-y: auto; padding: 14px; }
  h3 { margin: 0 0 6px; font-size: 14px; }
  .item { display: flex; gap: 10px; padding: 8px; border-radius: 8px; text-align: left; }
  .item:hover, .item.active { background: var(--hover); }
  .bar { width: 4px; border-radius: 2px; background: var(--c); flex-shrink: 0; }
  .info { display: grid; gap: 2px; min-width: 0; }
  .info .s { font-size: 13px; font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .when { font-size: 12px; color: var(--muted); }
  .empty { font-size: 13px; color: var(--muted); }
  .detail { margin-top: 8px; padding: 12px; border: 1px solid var(--border); border-top: 4px solid var(--c); border-radius: 8px; font-size: 13px; user-select: text; }
  .detail h4 { margin: 0 0 4px; font-size: 15px; }
  .detail p { margin: 4px 0; }
  .detail ul { margin: 2px 0 6px; padding-left: 18px; }
  .cal { display: flex; align-items: center; gap: 6px; color: var(--muted); }
  .dot { width: 9px; height: 9px; border-radius: 50%; background: var(--c); }
  .notes { white-space: pre-wrap; padding-top: 6px; border-top: 1px solid var(--border); color: var(--muted); }
  .muted { color: var(--muted); }
  .actions { display: flex; gap: 8px; margin-top: 10px; }
  .actions button { padding: 5px 12px; border: 1px solid var(--border); border-radius: 6px; font-size: 12px; }
  .actions button:hover { border-color: var(--accent); color: var(--accent); }
  .actions .danger:hover { border-color: var(--danger); color: var(--danger); }
</style>
