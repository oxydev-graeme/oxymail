<script>
  import { untrack } from "svelte";
  import { api } from "./api.js";
  import Icon from "./Icon.svelte";

  /** Create or edit an event. `event` is an existing event, or `{ start }` for a new one. */
  let { event, calendars, onsaved, onclose } = $props();

  const pad = (n) => String(n).padStart(2, "0");
  // All-day events are stored as UTC midnights; timed events as instants shown in local time.
  const dateOf = (ts, utc) => {
    const d = new Date(ts * 1000);
    return utc
      ? `${d.getUTCFullYear()}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())}`
      : `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
  };
  const timeOf = (ts) => {
    const d = new Date(ts * 1000);
    return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
  };

  const e = untrack(() => event);
  const allDayInit = Boolean(e.all_day);
  let summary = $state(e.summary ?? "");
  let calendarId = $state(e.calendar_id ?? untrack(() => calendars[0]?.id));
  let allDay = $state(allDayInit);
  let startDate = $state(dateOf(e.start, allDayInit));
  let startTime = $state(allDayInit ? "09:00" : timeOf(e.start));
  let endDate = $state(dateOf(allDayInit ? e.end - 86400 : e.end, allDayInit));
  let endTime = $state(allDayInit ? "10:00" : timeOf(e.end));
  let location = $state(e.location ?? "");
  let description = $state(e.description ?? "");
  const freqOf = (rule) => rule?.match(/FREQ=(\w+)/)?.[1] ?? "";
  let repeat = $state(freqOf(e.rrule));
  const customRule = e.rrule && !/^RRULE:FREQ=\w+$/.test(e.rrule.trim()) ? e.rrule : null;
  let error = $state("");
  let saving = $state(false);

  async function save() {
    error = "";
    if (!summary.trim()) return (error = "Give the event a title.");
    let start, end;
    if (allDay) {
      start = Date.parse(`${startDate}T00:00:00Z`) / 1000;
      end = Date.parse(`${endDate}T00:00:00Z`) / 1000 + 86400;
    } else {
      start = new Date(`${startDate}T${startTime}`).getTime() / 1000;
      end = new Date(`${endDate}T${endTime}`).getTime() / 1000;
    }
    if (!(end > start)) return (error = "The event has to end after it starts.");
    const rrule = repeat ? (customRule && freqOf(customRule) === repeat ? customRule : `RRULE:FREQ=${repeat}`) : null;
    saving = true;
    try {
      const id = await api.saveEvent({
        id: e.id ?? null,
        calendar_id: Number(calendarId),
        summary: summary.trim(),
        location: location.trim() || null,
        description: description.trim() || null,
        start,
        end,
        all_day: allDay,
        rrule,
        attendees: e.attendees ?? [],
      });
      onsaved(id);
    } catch (err) {
      error = String(err);
    } finally {
      saving = false;
    }
  }

  function onKey(event) {
    if (event.key === "Escape") onclose();
    if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) save();
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="backdrop" role="presentation" onclick={(ev) => ev.target === ev.currentTarget && onclose()}>
  <div class="editor" role="dialog" aria-label="Event">
    <header>
      <span>{e.id ? "Edit event" : "New event"}</span>
      <button class="icon" onclick={onclose} title="Close"><Icon name="x" size={16} /></button>
    </header>
    <div class="body">
      <input class="title" bind:value={summary} placeholder="Title" />
      <div class="row">
        <label class="grow">
          <span>Calendar</span>
          <select bind:value={calendarId}>
            {#each calendars as c (c.id)}<option value={c.id}>{c.name}</option>{/each}
          </select>
        </label>
        <label class="check"><input type="checkbox" bind:checked={allDay} /> All day</label>
      </div>
      <div class="row">
        <label class="grow"><span>Starts</span><input type="date" bind:value={startDate} /></label>
        {#if !allDay}<label><span>&nbsp;</span><input type="time" bind:value={startTime} /></label>{/if}
        <label class="grow"><span>Ends</span><input type="date" bind:value={endDate} /></label>
        {#if !allDay}<label><span>&nbsp;</span><input type="time" bind:value={endTime} /></label>{/if}
      </div>
      <label>
        <span>Repeats</span>
        <select bind:value={repeat}>
          <option value="">Doesn't repeat</option>
          <option value="DAILY">Every day</option>
          <option value="WEEKLY">Every week</option>
          <option value="MONTHLY">Every month</option>
          <option value="YEARLY">Every year</option>
        </select>
      </label>
      {#if customRule && repeat === freqOf(customRule)}
        <p class="hint">Keeps its original rule: <code>{customRule.replace("RRULE:", "")}</code></p>
      {/if}
      <label><span>Location</span><input bind:value={location} /></label>
      <label><span>Notes</span><textarea rows="4" bind:value={description}></textarea></label>
      {#if error}<p class="error">{error}</p>{/if}
    </div>
    <footer>
      <span class="hint">Ctrl+Enter saves</span>
      <button class="primary" onclick={save} disabled={saving}>{saving ? "Saving…" : "Save"}</button>
    </footer>
  </div>
</div>

<style>
  .backdrop { position: fixed; inset: 0; display: grid; place-items: center; background: rgb(10 14 20 / 0.4); z-index: 28; }
  .editor { display: flex; flex-direction: column; width: min(560px, 94vw); max-height: 90vh; background: var(--panel); border: 1px solid var(--border); border-radius: 12px; box-shadow: 0 20px 60px rgb(0 0 0 / 0.3); overflow: hidden; }
  header { display: flex; align-items: center; justify-content: space-between; padding: 12px 12px 8px 18px; font-weight: 650; }
  .icon { display: grid; place-items: center; padding: 4px; border-radius: 6px; color: var(--muted); }
  .icon:hover { background: var(--hover); color: var(--text); }
  .body { display: grid; gap: 12px; padding: 4px 18px 16px; overflow-y: auto; }
  .title { font-size: 17px; font-weight: 600; padding: 8px 10px !important; }
  .row { display: flex; flex-wrap: wrap; gap: 10px; align-items: end; }
  .grow { flex: 1; min-width: 130px; }
  label { display: grid; gap: 4px; font-size: 12px; color: var(--muted); }
  label.check { display: flex; align-items: center; gap: 6px; padding-bottom: 8px; color: var(--text); font-size: 13px; }
  input, select, textarea { padding: 7px 9px; border: 1px solid var(--border); border-radius: 8px; background: var(--bg); color: var(--text); font: inherit; font-size: 13px; outline: none; color-scheme: inherit; }
  input:focus, select:focus, textarea:focus { border-color: var(--accent); }
  textarea { resize: vertical; }
  .hint { margin: 0; font-size: 12px; color: var(--muted); }
  code { font-size: 11px; }
  .error { margin: 0; color: var(--danger); font-size: 13px; }
  footer { display: flex; align-items: center; justify-content: space-between; padding: 10px 18px; border-top: 1px solid var(--border); }
  .primary { padding: 7px 18px; border-radius: 8px; background: var(--accent); color: #fff; font-weight: 600; font-size: 13px; }
  .primary:disabled { opacity: 0.6; }
</style>
