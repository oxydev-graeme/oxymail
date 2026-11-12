<script>
  import { onMount } from "svelte";
  import { api, COMPOSE_FONTS } from "./api.js";
  import Icon from "./Icon.svelte";
  import Logo from "./Logo.svelte";
  import AccountsSettings from "./AccountsSettings.svelte";
  import SignInSettings from "./SignInSettings.svelte";
  import MailbirdImport from "./MailbirdImport.svelte";
  import FiltersSettings from "./FiltersSettings.svelte";

  let { settings, accounts, onchange, onclose, onimported, onaccountschanged, oncheckupdates, update = null, updating = null, oninstallupdate } = $props();
  let version = $state("");

  let section = $state("general");
  /** Whether OxyMail starts with the computer: kept by the system, not in settings. */
  let autostart = $state(null);
  let importPath = $state("~/MailbirdExport/mail");
  let importing = $state(false);
  let importResult = $state("");
  let dataPath = $state("");
  let pimPath = $state("~/MailbirdExport");
  let pimBusy = $state(false);
  let pimResult = $state("");

  async function runPimImport() {
    pimBusy = true;
    pimResult = "";
    try {
      const r = await api.importPim([pimPath]);
      pimResult = `Imported ${r.calendars} calendars with ${r.events.toLocaleString()} events, and ${r.people.toLocaleString()} new contacts.`;
      onimported();
    } catch (e) {
      pimResult = `Import failed: ${e}`;
    } finally {
      pimBusy = false;
    }
  }

  onMount(async () => {
    version = await import("@tauri-apps/api/app").then((m) => m.getVersion()).catch(() => "");
    dataPath = await api.dataLocation();
    autostart = await api.autostart().catch(() => null);
    isDefault = await api.defaultMailApp().catch(() => null);
  });

  /** true, false, or null where OxyMail can't tell (the option is then hidden). */
  let isDefault = $state(null);
  async function makeDefault() {
    try {
      await api.makeDefaultMailApp();
      isDefault = await api.defaultMailApp();
    } catch (e) {
      alertText = `Couldn't make OxyMail the default: ${e}`;
    }
  }

  async function setAutostart(on) {
    try {
      await api.setAutostart(on);
      autostart = on;
    } catch (e) {
      autostart = await api.autostart().catch(() => null);
      alertText = `Couldn't change start-up: ${e}`;
    }
  }
  let alertText = $state("");

  const on = (key) => settings[key] !== "false";
  const toggle = (key) => (e) => onchange(key, String(e.currentTarget.checked));
  const WEEKDAYS = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

  const sections = [
    ["general", "General"],
    ["appearance", "Appearance"],
    ["reading", "Reading & sending"],
    ["composing", "Composing"],
    ["snoozing", "Snoozing"],
    ["filters", "Filters"],
    ["accounts", "Accounts"],
    ["signin", "Sign-in"],
    ["import", "Import"],
    ["shortcuts", "Shortcuts & search"],
    ["about", "About"],
  ];

  const shortcuts = [
    ["↓ / j", "Next message"],
    ["↑ / k", "Previous message"],
    ["e", "Archive"],
    ["Delete / #", "Delete (move to Trash)"],
    ["s", "Star or unstar"],
    ["u", "Mark read or unread"],
    ["/", "Search"],
    ["v", "Move to folder"],
    ["b", "Snooze"],
    ["Ctrl+A", "Select all loaded messages"],
    ["Ctrl / Shift + click", "Add to selection / select a range"],
    ["c, r, a, f", "New, reply, reply all, forward"],
    ["Esc", "Clear search or close this panel"],
  ];

  async function runImport() {
    importing = true;
    importResult = "";
    try {
      const stats = await api.importArchive(importPath);
      importResult = `Added ${stats.added.toLocaleString()} messages (${stats.skipped.toLocaleString()} already imported).`;
      onimported();
    } catch (e) {
      importResult = `Import failed: ${e}`;
    } finally {
      importing = false;
    }
  }

  function onKey(event) {
    if (event.key === "Escape") onclose();
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog" role="dialog" aria-label="Settings">
    <nav>
      <div class="title">Settings</div>
      {#each sections as [id, label]}
        <button class:active={section === id} onclick={() => (section = id)}>{label}</button>
      {/each}
    </nav>
    <div class="pane">
      <div class="topbar">
        <button class="close" onclick={onclose} title="Close (Esc)"><Icon name="x" /></button>
      </div>
      <div class="content">

      {#if section === "general"}
        <h2>General</h2>
        <label class="toggle">
          <input type="checkbox" checked={autostart === true} disabled={autostart === null} onchange={(e) => setAutostart(e.currentTarget.checked)} />
          <span>Start OxyMail when I sign in to the computer</span>
        </label>
        <label class="toggle sub" class:off={!autostart}>
          <input type="checkbox" checked={on("start_minimized")} disabled={!autostart} onchange={toggle("start_minimized")} />
          <span>Start in the tray, without opening the window</span>
        </label>
        <label class="toggle">
          <input type="checkbox" checked={settings.quit_on_close === "true"} onchange={toggle("quit_on_close")} />
          <span>Quit OxyMail when the window is closed</span>
        </label>
        <p class="hint push">
          Otherwise closing the window keeps OxyMail in the tray, checking mail and notifying you. Quit from the tray
          icon's menu.
        </p>
        {#if isDefault !== null}
          <div class="field">
            <div class="label">Default email app</div>
            {#if isDefault}
              <p class="hint">OxyMail opens email links (mailto:) from your browser and other apps.</p>
            {:else}
              <div class="row"><button class="primary" onclick={makeDefault}>Make OxyMail the default</button></div>
              <p class="hint">So email links in your browser and other apps open a new message here.</p>
            {/if}
          </div>
        {/if}
        {#if alertText}<p class="result">{alertText}</p>{/if}
        <h2 class="second">Notifications</h2>
        <label class="toggle">
          <input type="checkbox" checked={on("notifications")} onchange={toggle("notifications")} />
          <span>Desktop notification when new mail arrives in an Inbox</span>
        </label>
        <label class="toggle sub" class:off={!on("notifications")}>
          <input type="checkbox" checked={on("new_mail_sound")} disabled={!on("notifications")} onchange={toggle("new_mail_sound")} />
          <span>Play the new-mail sound</span>
        </label>
        <label class="toggle">
          <input type="checkbox" checked={on("unread_badge")} onchange={toggle("unread_badge")} />
          <span>Show the unread count on the taskbar and tray icon</span>
        </label>
        <p class="hint push">Counts unread mail in the Inboxes of your live accounts. Notifications for mail you've since read, here or on another device, are taken away.</p>
      {:else if section === "appearance"}
        <h2>Appearance</h2>
        <div class="field">
          <div class="label">Theme</div>
          <div class="segmented">
            {#each [["system", "System"], ["light", "Light"], ["dark", "Dark"]] as [value, label]}
              <button class:on={settings.theme === value} onclick={() => onchange("theme", value)}>{label}</button>
            {/each}
          </div>
        </div>
        <div class="field">
          <div class="label">Message list</div>
          <div class="segmented">
            {#each [["comfortable", "Comfortable"], ["compact", "Compact"]] as [value, label]}
              <button class:on={settings.density === value} onclick={() => onchange("density", value)}>{label}</button>
            {/each}
          </div>
          <p class="hint">Compact hides the preview line and fits more messages on screen.</p>
        </div>
        <label class="toggle">
          <input type="checkbox" checked={on("important_marker")} onchange={toggle("important_marker")} />
          <span>Show Gmail's important marker <span class="important-sample"></span> in the message list</span>
        </label>
        <div class="field">
          <div class="label">Text size</div>
          <div class="segmented">
            {#each ["90", "100", "110", "125", "150"] as value}
              <button class:on={(settings.app_zoom || "100") === value} onclick={() => onchange("app_zoom", value)}>{value}%</button>
            {/each}
          </div>
          <p class="hint">Everything in OxyMail. Ctrl and + or − change it too.</p>
        </div>
        <div class="field">
          <div class="label">Email text size</div>
          <div class="segmented">
            {#each ["90", "100", "106", "115", "125", "150"] as value}
              <button class:on={(settings.email_zoom || "100") === value} onclick={() => onchange("email_zoom", value)}>{value}%</button>
            {/each}
          </div>
          <p class="hint">Only the messages you read, on top of the text size above.</p>
        </div>
        <div class="field">
          <div class="label">Column widths</div>
          <p class="hint">Drag the borders between the sidebar, the message list and the reader. Double-click a border to put it back.</p>
        </div>
      {:else if section === "reading"}
        <h2>Reading &amp; sending</h2>
        <div class="field">
          <div class="label">Mark messages as read</div>
          <div class="segmented">
            {#each [["true", "Immediately"], ["2", "After 2 s"], ["5", "After 5 s"], ["10", "After 10 s"], ["false", "Never"]] as [value, label]}
              <button class:on={(settings.mark_read_on_open ?? "true") === value} onclick={() => onchange("mark_read_on_open", value)}>{label}</button>
            {/each}
          </div>
          <p class="hint">With a delay, a message only counts as read if you stay on it that long. Copies in other folders, such as Important, change with it.</p>
        </div>
        <label class="toggle">
          <input type="checkbox" checked={on("select_next")} onchange={toggle("select_next")} />
          <span>Open the next message after archiving, deleting or moving one</span>
        </label>
        <div class="field">
          <div class="label">Undo send</div>
          <div class="segmented">
            {#each [["0", "Off"], ["5", "5 s"], ["8", "8 s"], ["15", "15 s"], ["30", "30 s"]] as [value, label]}
              <button class:on={(settings.undo_send_seconds ?? "8") === value} onclick={() => onchange("undo_send_seconds", value)}>{label}</button>
            {/each}
          </div>
          <p class="hint">How long OxyMail waits before sending, so you can take a message back.</p>
        </div>
        <div class="field">
          <div class="label">Remote content</div>
          <p class="hint">
            Remote images, fonts and scripts are always blocked, so senders can't tell when you read their
            email. Images included in the message itself still show.
          </p>
        </div>
      {:else if section === "composing"}
        <h2>Composing</h2>
        <div class="field">
          <div class="label">Font for new messages</div>
          <div class="row">
            <select value={settings.compose_font ?? ""} onchange={(e) => onchange("compose_font", e.currentTarget.value)}>
              {#each COMPOSE_FONTS as [value, label]}<option {value}>{label}</option>{/each}
            </select>
            <select value={settings.compose_size ?? ""} onchange={(e) => onchange("compose_size", e.currentTarget.value)}>
              <option value="">Default size</option>
              {#each ["9", "10", "11", "12", "14", "16"] as size}<option value={size}>{size} pt</option>{/each}
            </select>
          </div>
          <div class="preview" style:font-family={settings.compose_font || null} style:font-size={settings.compose_size ? `${settings.compose_size}pt` : null}>This is how your messages will look.</div>
          <p class="hint">Sent with the message, so people see the same font.</p>
        </div>
        <label class="toggle">
          <input type="checkbox" checked={settings.show_cc_bcc === "true"} onchange={toggle("show_cc_bcc")} />
          <span>Always show the Cc and Bcc fields</span>
        </label>
      {:else if section === "snoozing"}
        <h2>Your week</h2>
        <p class="hint push">Snooze and Send later offer times from this schedule.</p>
        <div class="grid">
          <span>My day starts at</span>
          <input type="time" value={settings.day_starts || "08:00"} onchange={(e) => onchange("day_starts", e.currentTarget.value)} />
          <span>My workday ends at</span>
          <input type="time" value={settings.workday_ends || "18:00"} onchange={(e) => onchange("workday_ends", e.currentTarget.value)} />
          <span>My week starts on</span>
          <select value={settings.week_starts_day ?? "1"} onchange={(e) => onchange("week_starts_day", e.currentTarget.value)}>
            {#each WEEKDAYS as day, i}<option value={String(i)}>{day}</option>{/each}
          </select>
          <span>My weekend starts on</span>
          <select value={settings.weekend_starts_day ?? "6"} onchange={(e) => onchange("weekend_starts_day", e.currentTarget.value)}>
            {#each WEEKDAYS as day, i}<option value={String(i)}>{day}</option>{/each}
          </select>
          <span>My weekend starts at</span>
          <input type="time" value={settings.weekend_starts || "09:00"} onchange={(e) => onchange("weekend_starts", e.currentTarget.value)} />
        </div>
        <h2 class="second">Snooze choices</h2>
        <div class="grid">
          <span>Later today</span>
          <select value={settings.later_today_hours ?? "3"} onchange={(e) => onchange("later_today_hours", e.currentTarget.value)}>
            {#each ["1", "2", "3", "4", "6"] as h}<option value={h}>In {h} hour{h === "1" ? "" : "s"}</option>{/each}
          </select>
          <span>Someday</span>
          <select value={settings.someday_months ?? "3"} onchange={(e) => onchange("someday_months", e.currentTarget.value)}>
            {#each ["1", "2", "3", "6", "12"] as m}<option value={m}>In {m === "12" ? "a year" : `${m} month${m === "1" ? "" : "s"}`}</option>{/each}
          </select>
        </div>
      {:else if section === "filters"}
        <FiltersSettings {accounts} />
      {:else if section === "accounts"}
        <AccountsSettings {accounts} onchanged={onaccountschanged} />
      {:else if section === "signin"}
        <SignInSettings />
      {:else if section === "import"}
        <h2>Import</h2>
        <div class="field">
          <div class="label">Bring over Mailbird</div>
          <MailbirdImport ondone={onimported} />
        </div>
        <div class="field">
          <div class="label">Import an archive</div>
          <div class="row">
            <input bind:value={importPath} />
            <button class="primary" onclick={runImport} disabled={importing}>
              {importing ? "Importing…" : "Import"}
            </button>
          </div>
          <p class="hint">
            A Maildir folder laid out as account / folder / cur, such as the Mailbird export. Messages already
            imported are skipped, and a removed account comes back with its stars and folders.
          </p>
          {#if importResult}<p class="result">{importResult}</p>{/if}
        </div>
        <div class="field">
          <div class="label">Calendars and contacts</div>
          <div class="row">
            <input bind:value={pimPath} />
            <button class="primary" onclick={runPimImport} disabled={pimBusy}>{pimBusy ? "Importing…" : "Import"}</button>
          </div>
          <p class="hint">A .ics or .vcf file, or a folder of them, such as the Mailbird export. Importing again updates what's there.</p>
          {#if pimResult}<p class="result">{pimResult}</p>{/if}
        </div>
        <div class="field">
          <div class="label">Index location</div>
          <code>{dataPath}</code>
        </div>
      {:else if section === "shortcuts"}
        <h2>Keyboard shortcuts</h2>
        <table>
          <tbody>
            {#each shortcuts as [keys, action]}
              <tr><td><kbd>{keys}</kbd></td><td>{action}</td></tr>
            {/each}
          </tbody>
        </table>
        <h2 class="second">Search filters</h2>
        <table>
          <tbody>
            {#each [["from:maria", "Sender name or address"], ["to:accounts@", "Recipient"], ["subject:invoice", "Words in the subject"], ['"tax invoice"', "An exact phrase"], ["has:attachment", "Only messages with attachments"], ["is:unread  is:read  is:starred", "By status"], ["in:sent  in:archive", "Folder (by role or name)"], ["account:work", "Accounts whose address contains this"], ["before:2025-01-01  after:2024-06-30", "By date"]] as [example, meaning]}
              <tr><td><kbd>{example}</kbd></td><td>{meaning}</td></tr>
            {/each}
          </tbody>
        </table>
        <p class="hint">Filters combine with plain words, e.g. <kbd>from:maria has:attachment invoice</kbd>.</p>
      {:else}
        <div class="about">
          <Logo size={56} />
          <div class="wordmark"><b>Oxy</b>Mail</div>
          <p class="tagline">Email with room to breathe.</p>
          <p class="hint">Version {version || "…"} · GPL-3.0</p>
          <div class="update-row">
            {#if update}
              <button class="primary" onclick={oninstallupdate} disabled={updating !== null}>
                {updating === null ? `Install ${update.version} and restart` : `Updating… ${Math.round(updating * 100)}%`}
              </button>
            {:else}
              <button class="primary" onclick={oncheckupdates}>Check for updates</button>
            {/if}
          </div>
          <label class="toggle centered">
            <input
              type="checkbox"
              checked={settings.auto_update !== "false"}
              onchange={(e) => onchange("auto_update", String(e.currentTarget.checked))}
            />
            <span>Check for updates when OxyMail starts</span>
          </label>
          <p class="hint">Updates come from GitHub releases and are only installed if their signature matches.</p>
          <p class="hint">github.com/oxydev-graeme/oxymail</p>
        </div>
      {/if}
      </div>
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    display: grid;
    place-items: center;
    background: rgb(10 14 20 / 0.45);
    z-index: 20;
  }
  .dialog {
    display: grid;
    grid-template-columns: 180px minmax(0, 1fr);
    grid-template-rows: minmax(0, 1fr);
    width: min(760px, 92vw);
    height: min(700px, 88vh);
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 12px;
    box-shadow: 0 20px 60px rgb(0 0 0 / 0.3);
    overflow: hidden;
  }
  nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 16px 10px;
    background: var(--sidebar);
    border-right: 1px solid var(--border);
  }
  .title { padding: 0 10px 10px; font-weight: 700; }
  nav button {
    padding: 7px 10px;
    border-radius: 6px;
    text-align: left;
    font-size: 13px;
    color: var(--muted);
  }
  nav button:hover { background: var(--border); color: var(--text); }
  nav button.active { background: var(--selected); color: var(--text); font-weight: 600; }
  /* The close button lives in its own bar above the scrolling content, so section headers
     (and their buttons) can never sit underneath it, and it stays put while scrolling. */
  .pane { display: flex; flex-direction: column; min-width: 0; min-height: 0; }
  .topbar { display: flex; justify-content: flex-end; padding: 10px 10px 0; flex-shrink: 0; }
  .content { flex: 1; min-height: 0; padding: 0 28px 24px; overflow-y: auto; }
  .close { display: grid; place-items: center; padding: 4px; border-radius: 6px; color: var(--muted); }
  .close:hover { background: var(--bg); color: var(--text); }
  h2 { margin: 0 0 18px; font-size: 17px; }
  .field { margin-bottom: 20px; }
  .label { font-size: 13px; font-weight: 600; margin-bottom: 8px; }
  .hint { margin: 6px 0 0; font-size: 12px; color: var(--muted); line-height: 1.5; }
  .result { font-size: 13px; margin: 8px 0 0; }
  .segmented {
    display: inline-flex;
    padding: 3px;
    gap: 2px;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 8px;
  }
  .segmented button { padding: 5px 14px; border-radius: 6px; font-size: 13px; color: var(--muted); }
  .segmented button.on { background: var(--panel); color: var(--text); font-weight: 600; box-shadow: 0 1px 2px rgb(0 0 0 / 0.12); }
  .toggle { display: flex; gap: 10px; align-items: center; margin-bottom: 20px; font-size: 13px; cursor: pointer; }
  .toggle input { width: 16px; height: 16px; accent-color: var(--accent); }
  .row { display: flex; gap: 8px; }
  .row input { flex: 1; padding: 7px 10px; border: 1px solid var(--border); border-radius: 8px; background: var(--bg); }
  .primary { padding: 7px 14px; border-radius: 8px; background: var(--accent); color: #fff; font-weight: 600; font-size: 13px; }
  .primary:disabled { opacity: 0.6; }
  code { font-size: 12px; color: var(--muted); word-break: break-all; }
  table { border-collapse: collapse; font-size: 13px; }
  h2.second { margin-top: 26px; }
  td { padding: 6px 16px 6px 0; }
  kbd {
    padding: 2px 7px;
    border: 1px solid var(--border);
    border-bottom-width: 2px;
    border-radius: 5px;
    background: var(--bg);
    font: 12px ui-monospace, monospace;
  }
  .update-row { margin: 14px 0 10px; }
  .important-sample { display: inline-block; width: 12px; height: 9px; margin: 0 2px; background: var(--amber); clip-path: polygon(0 0, 68% 0, 100% 50%, 68% 100%, 0 100%, 28% 50%); }
  .toggle.sub { margin-left: 26px; margin-top: -10px; }
  .toggle.off { opacity: 0.5; cursor: default; }
  .hint.push { margin: -12px 0 20px; }
  .grid { display: grid; grid-template-columns: minmax(0, 1fr) 180px; gap: 10px 16px; align-items: center; max-width: 460px; font-size: 13px; }
  .grid input, .grid select, .row select { padding: 6px 8px; border: 1px solid var(--border); border-radius: 8px; background: var(--bg); color: var(--text); font: inherit; font-size: 13px; color-scheme: inherit; }
  .row select { flex: 1; }
  .preview { margin-top: 10px; padding: 12px 14px; border: 1px solid var(--border); border-radius: 8px; background: var(--bg); }
  .toggle.centered { justify-content: center; margin-bottom: 6px; }
  .about { display: grid; justify-items: center; text-align: center; padding-top: 40px; }
  .wordmark { margin-top: 12px; font-size: 26px; letter-spacing: -0.03em; font-weight: 450; }
  .wordmark b { font-weight: 800; }
  .tagline { margin: 4px 0 18px; color: var(--muted); }
</style>
