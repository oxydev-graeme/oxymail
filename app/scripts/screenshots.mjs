// Usage: npm run build && npm run shots   (writes PNGs to ./screenshots, which git ignores)
// Renders the built OxyMail UI in headless Chromium with a fake Tauri backend and saves
// screenshots of the main states at a few window sizes.
import { chromium } from "playwright";
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { extname, join } from "node:path";

const dist = process.argv[2];
const out = process.argv[3];
const types = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".png": "image/png", ".svg": "image/svg+xml" };
const server = createServer(async (req, res) => {
  const path = join(dist, req.url === "/" ? "index.html" : req.url.split("?")[0]);
  try {
    res.writeHead(200, { "content-type": types[extname(path)] ?? "application/octet-stream" });
    res.end(await readFile(path));
  } catch {
    res.writeHead(404).end();
  }
}).listen(4317);

const mock = () => {
  const colors = ["#3f3151", "#92d050", "#c00000", "#244061", "#00b050", "#974806", "#4f6128", "#205867", "#7030a0", "#002060"];
  const names = ["hello@brightledger.example", "brightledger@gmail.example", "jane@northwind.example", "jane.smith@gmail.example",
    "jsmith@gmail.example", "jane@smithhome.example", "studio@larkspur.example", "hello@larkspur.example", "jane@larkspur.example",
    "accounts@larkspur.example", "larkspur.web@gmail.example", "franchise.web@gmail.example", "jane@atomicstudio.example",
    "fieldops@gmail.example", "accounts@fieldops.example", "notes.later@gmail.example", "jane@harbourcreative.example", "team@larkspur.example"];
  const accounts = names.map((name, i) => ({ id: i + 1, name, kind: i === 8 ? "imap" : "archive", label: null, sender_name: "Jane Smith",
    color: colors[i % colors.length], signature: i === 8 ? "<b>Jane Smith</b><div>Mobile: 061 000 0000</div>" : null, icon: null, has_archive: true, unread: i % 4 === 0 ? 12 * i + 3 : 0 }));
  const folderNames = [["Inbox", "inbox"], ["Drafts", "drafts"], ["Sent", "sent"], ["Archive", "archive"], ["Clients / Sunridge Solar", null], ["Invoices", null], ["Spam", "spam"], ["Trash", "trash"]];
  const folders = (aid) => folderNames.map(([name, role], j) => ({ id: aid * 100 + j, account_id: aid, name, role, total: 500 - j * 40, unread: j === 0 ? 7 : 0 }));
  const now = Math.floor(Date.now() / 1000);
  const msgs = (fid) => Array.from({ length: 60 }, (_, k) => ({ id: fid * 1000 + k, folder_id: fid, folder_name: "Inbox", folder_role: "inbox", account_id: Math.floor(fid / 100),
    date: now - k * 7200, important: k % 3 === 0, from_name: ["Maria from Coastline Appliances", "Asana", "Larkspur Studio", "Daniel Hart", "Google Workspace"][k % 5], from_addr: "x@example.com",
    subject: ["RE: Tax Invoice - INV0000123 for the period ending September and the rest of a long subject line", "New comment on Home page [Final Revisions]", "INV-00042 invoice viewed", "Sunridge Solar rebuild — next steps", "Security alert"][k % 5],
    snippet: "Hi Jane, please find attached the updated invoice as discussed on our call this morning. Let me know if anything needs changing before month end.",
    unread: k % 3 === 0, starred: k % 7 === 0, attachments: k % 4 === 0 ? 2 : 0,
    // Every message shares a conversation with the ones 5 and 10 rows further down.
    thread_key: `t${k % 15}` }));
  const view = (id) => ({ id, account_id: 9, message_id: "abc@x", references: [], reply_to: [], subject: "RE: Tax Invoice - INV0000123",
    from: ["Maria <maria@coastline-appliances.example>"], to: ["jane@larkspur.example"], cc: ["accounts@coastline-appliances.example", "someone.else.with.a.long.address@example.com"],
    date: now - 3600, html: "<p>Hi Jane,</p>" + "<p>Please find attached the updated invoice. ".repeat(4) + "</p>".repeat(1) + Array.from({ length: 40 }, (_, i) => `<p>Line ${i + 1} of a long email body to test scrolling.</p>`).join("") + '<table width="900" border="1"><tr><td>Wide table cell</td><td>Another cell that is wide</td></tr></table>',
    text: null, attachments: [{ index: 0, name: "Invoice-0000146.pdf", content_type: "application/pdf", size: 81234 }, { index: 1, name: "Statement September 2026 with a very long file name.xlsx", content_type: "application/vnd", size: 22111 }],
    notices: ["Attachment not available: SWIFT_PO#000000000000.zip (4533 bytes)"],
    from_addr: "maria@coastline-appliances.example", has_remote_content: true, images_allowed: false,
    unsubscribe: ["https://example.com/unsubscribe"] });
  const settings = { theme: window.__THEME__ ?? "system", density: "comfortable", mark_read_on_open: "true", ...(window.__SETTINGS__ ?? {}) };
  const handlers = {
    accounts: () => accounts, folders: ({ accountId }) => folders(accountId), messages: ({ folderId, offset }) => (offset ? [] : msgs(folderId)),
    message: ({ id }) => view(id), settings: () => settings, set_setting: () => null, data_location: () => "/home/jane/.local/share/oxymail/oxymail.db",
    search: () => [], set_flag: () => null,
    move_to_folder: (args) => { (window.__calls ??= []).push(["move_to_folder", args]); return null; },
    live_config: () => ({ email: "jane@larkspur.example", username: "jane@larkspur.example", source: "the provider's autoconfig", oauth: null,
      imap: { host: "mail.larkspur.example", port: 993, security: "tls" }, smtp: { host: "mail.larkspur.example", port: 465, security: "tls" } }),
    messages_by_role: () => msgs(100).map((m, k) => ({ ...m, account_id: (k % 6) + 1 })),
    filters: () => [
      { id: 1, account_id: null, field: "subject", value: "***SPAM***", action: "spam", enabled: true },
      { id: 2, account_id: null, field: "subject", value: "*casino*", action: "spam", enabled: true },
      { id: 3, account_id: 9, field: "subject", value: "[Wordfence Alert]", action: "archive", enabled: true },
      { id: 4, account_id: null, field: "from", value: "newsletter@", action: "read", enabled: false },
    ],
    autostart: () => true,
    save_filter: () => 5, run_filter: () => 4,
    suggest_contacts: () => [{ email: "peter@riverbend.example", name: "Peter Mabaso" }, { email: "plang@meridian.example", name: "Peter Lang" }, { email: "peter.another@example.com", name: null }],
    thread: ({ id }) => [
      { id: 900001, folder_id: 102, folder_name: "Sent", account_id: 1, date: now - 90000, from_name: "Jane Smith", subject: "Tax Invoice - INV0000123", snippet: "", unread: false, starred: false, attachments: 0 },
      { id, folder_id: 100, folder_name: "Inbox", account_id: 1, date: now - 3600, from_name: "Maria from Coastline Appliances", subject: "RE: Tax Invoice - INV0000123", snippet: "", unread: false, starred: false, attachments: 1 },
    ],
    snoozed: () => msgs(100).slice(0, 3).map((m, k) => ({ ...m, snoozed_until: now + (k + 1) * 86400 })),
    scheduled: () => [{ id: 1, account_id: 9, send_at: now + 7200, mail: "{}", draft: "{}", summary: "daniel@hartdigital.example — Sunridge Solar proposal", error: null }],
    drafts: () => [{ id: 1, account_id: 9, subject: "Quote for Sunridge Solar", recipients: "daniel@hartdigital.example", body: "{}", updated: now - 600 }],
    calendars: () => [
      { id: 1, name: "Calendar", color: "#0e7490", visible: true, events: 120 },
      { id: 2, name: "Family", color: "#7c3aed", visible: true, events: 40 },
      { id: 3, name: "Holidays in South Africa", color: "#c2410c", visible: true, events: 250 },
      { id: 4, name: "Team OOO Calendar", color: "#15803d", visible: false, events: 30 },
    ],
    occurrences: () => {
      const day = (offset, h, m = 0) => { const d = new Date(); d.setDate(d.getDate() + offset); d.setHours(h, m, 0, 0); return Math.floor(d / 1000); };
      const allDay = (offset) => { const d = new Date(); d.setDate(d.getDate() + offset); return Math.floor(Date.UTC(d.getFullYear(), d.getMonth(), d.getDate()) / 1000); };
      const ev = (id, cal, color, summary, start, end, all_day = false, recurring = false) => ({ event_id: id, calendar_id: cal, color, summary, location: id === 2 ? "Teams" : null, start, end, all_day, recurring, cancelled: false });
      return [
        ev(1, 3, "#c2410c", "Heritage Day", allDay(3), allDay(4), true),
        ev(2, 1, "#0e7490", "Weekly Web Team Huddle", day(0, 9), day(0, 9, 30), false, true),
        ev(3, 1, "#0e7490", "Weekly client check-in", day(0, 13), day(0, 14), false, true),
        ev(4, 2, "#7c3aed", "Swimming lessons", day(0, 17), day(0, 18), false, true),
        ev(5, 1, "#0e7490", "Sunridge Solar proposal review", day(1, 10), day(1, 11)),
        ev(6, 2, "#7c3aed", "Dinner with the Smiths", day(2, 19), day(2, 21)),
        ev(7, 1, "#0e7490", "Invoice run", day(-2, 8), day(-2, 9)),
        ev(8, 1, "#0e7490", "Another meeting", day(0, 15), day(0, 16)),
      ];
    },
    event: ({ id }) => ({ id, calendar_id: 1, summary: "Weekly Web Team Huddle", description: "Agenda in the shared doc.", location: "Teams", start: 0, end: 1800, all_day: false, rrule: "RRULE:FREQ=WEEKLY;BYDAY=TU", organizer: "Daniel Hart <daniel@hartdigital.example>", attendees: [{ name: "Jane Smith", email: "g@x", status: "ACCEPTED" }, { name: null, email: "team@hartdigital.example", status: "NEEDS-ACTION" }], status: null }),
    people: () => [
      { id: 1, full_name: "Peter Mabaso", org: "Riverbend", email: "peter@riverbend.example", phone: "082 555 0101", photo: null },
      { id: 2, full_name: "Lerato Dlamini", org: null, email: null, phone: "082 555 0102", photo: null },
      { id: 3, full_name: "Mia Smith", org: null, email: null, phone: "072 555 0103", photo: null },
      { id: 4, full_name: "Maria Botha", org: "Coastline Appliances", email: "maria@coastline-appliances.example", phone: null, photo: null },
    ],
    person: () => ({ id: 1, full_name: "Peter Mabaso", first_name: "Peter", last_name: "Mabaso", org: "Riverbend", title: "Director", notes: "Met at the Cape Town expo.", birthday: "1980-05-04", photo: null, groups: "Clients", fields: [{ kind: "email", label: "Home", value: "peter@riverbend.example" }, { kind: "phone", label: "Mobile", value: "082 555 0101" }, { kind: "url", label: "Home Page", value: "https://riverbend.example" }] }),
    "plugin:app|version": () => "0.2.0",
    "plugin:updater|check": () => (window.__UPDATE__ ? { rid: 1, currentVersion: "0.2.0", version: "0.3.1", date: null, body: "New branding", rawJson: {} } : null),
    oauth_clients: () => ({ google: { client_id: "1234-abc.apps.googleusercontent.com", has_secret: true }, microsoft: { client_id: "", has_secret: false } }),
    connect_suggestion: () => ({ email: "hello@larkspur.example", username: "hello@larkspur.example", oauth: null,
      source: "your Mailbird settings (POP3 there; OxyMail uses IMAP on the same server)",
      imap: { host: "mail.larkspur.example", port: 993, security: "tls" }, smtp: { host: "mail.larkspur.example", port: 465, security: "tls" } }),
    discover_account: ({ email }) => ({ email, username: email, source: "Mozilla's ISP database (via MX)", oauth: "google",
      imap: { host: "imap.gmail.com", port: 993, security: "tls" }, smtp: { host: "smtp.gmail.com", port: 465, security: "tls" } }), "plugin:event|listen": () => 1, "plugin:event|unlisten": () => null,
  };
  let cb = 0;
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: "main" }, currentWebview: { windowLabel: "main", label: "main" } },
    transformCallback: () => ++cb,
    unregisterCallback: () => {},
    invoke: async (cmd, args) => (handlers[cmd] ? handlers[cmd](args ?? {}) : null),
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
};

const browser = await chromium.launch();
const shots = [
  ["main-1360-dark", 1360, 860, "dark", async (p) => { await p.click(".row-main"); }],
  ["main-1360-light", 1360, 860, "light", async (p) => { await p.click(".row-main"); }],
  ["main-900", 900, 600, "dark", async (p) => { await p.click(".row-main"); }],
  // Sidebar dragged narrower and the message list wider; the list divider stays highlighted mid-drag.
  ["columns-resized", 1360, 860, "dark", async (p) => {
    await p.click(".row-main");
    await p.mouse.move(250, 400); await p.mouse.down(); await p.mouse.move(200, 400, { steps: 4 }); await p.mouse.up();
    await p.mouse.move(580, 400); await p.mouse.down(); await p.mouse.move(760, 400, { steps: 4 });
  }],
  // A narrow window with a wide list asked for: the reader still keeps its minimum width.
  ["columns-900-clamped", 900, 600, "dark", async (p) => {
    await p.click(".row-main");
    await p.mouse.move(540, 300); await p.mouse.down(); await p.mouse.move(890, 300, { steps: 4 }); await p.mouse.up();
  }],
  ["scrolled", 1360, 860, "dark", async (p) => {
    await p.click(".row-main");
    await p.mouse.move(120, 700); await p.mouse.wheel(0, 2000);
    await p.mouse.move(500, 600); await p.mouse.wheel(0, 3000);
    await p.waitForTimeout(300);
  }],
  ["settings-accounts", 1360, 860, "dark", async (p) => { await p.click('button[title="Settings"]'); await p.click('.dialog nav button:text-is("Accounts")'); }],
  ["settings-900", 900, 600, "dark", async (p) => { await p.click('button[title="Settings"]'); await p.click('.dialog nav button:text-is("Accounts")'); }],
  ["edit-live-account", 1360, 860, "dark", async (p) => {
    await p.click('button[title="Settings"]'); await p.click('.dialog nav button:text-is("Accounts")');
    await p.locator(".accounts li", { hasText: "jane@larkspur.example" }).locator("button.edit").click();
    await p.waitForTimeout(200); await p.locator(".content").evaluate((el) => (el.scrollTop = 520));
  }],
  ["connect-archive", 1360, 860, "dark", async (p) => {
    await p.click('button[title="Settings"]'); await p.click('.dialog nav button:text-is("Accounts")');
    await p.locator(".accounts li", { hasText: "hello@larkspur.example" }).locator("button.edit").click();
    await p.click('.dialog button:text-is("Set up sending and receiving")');
    await p.waitForTimeout(200); await p.locator(".content").evaluate((el) => (el.scrollTop = 560));
  }],
  ["signin-settings", 1360, 860, "dark", async (p) => {
    await p.click('button[title="Settings"]'); await p.click('.dialog nav button:text-is("Sign-in")');
    await p.click('.dialog summary >> nth=0');
  }],
  ["all-inboxes", 1360, 860, "dark", async (p) => { await p.click("button:has-text('All inboxes')"); await p.click(".row-main"); }],
  ["compose-autocomplete", 1360, 860, "dark", async (p) => {
    await p.click(".compose-btn"); await p.fill("#compose-to", "pet"); await p.waitForTimeout(400);
  }],
  ["forward", 1360, 860, "dark", async (p) => { await p.click(".row-main"); await p.click("button:has-text('Forward')"); }],
  ["settings-reading", 1360, 860, "light", async (p) => { await p.click('button[title="Settings"]'); await p.click('.dialog nav button:text-is("Reading & sending")'); }],
  ["settings-filters", 1360, 860, "dark", async (p) => { await p.click('button[title="Settings"]'); await p.click('.dialog nav button:text-is("Filters")'); }],
  ["settings-general", 1360, 860, "dark", async (p) => { await p.click('button[title="Settings"]'); }],
  ["settings-composing", 1360, 860, "dark", async (p) => { await p.click('button[title="Settings"]'); await p.click('.dialog nav button:text-is("Composing")'); }],
  ["settings-snoozing", 1360, 860, "dark", async (p) => { await p.click('button[title="Settings"]'); await p.click('.dialog nav button:text-is("Snoozing")'); }],
  ["settings-appearance", 1360, 860, "light", async (p) => { await p.click('button[title="Settings"]'); await p.click('.dialog nav button:text-is("Appearance")'); }],
  ["reader-spam-filter", 1360, 860, "dark", async (p) => { await p.click(".row-main"); }],
  ["filter-from-message", 1360, 860, "dark", async (p) => { await p.click(".row-main"); await p.click(".toolbar button:has-text('Filter')"); }],
  ["filters-run-now", 1360, 860, "dark", async (p) => {
    await p.click('button[title="Settings"]'); await p.click('.dialog nav button:text-is("Filters")');
    await p.locator(".filter").first().locator("button:has-text('Run now')").click(); await p.waitForTimeout(200);
  }],
  ["conversations", 1360, 860, "dark", async (p) => { await p.click(".row-main"); }, { group_conversations: "true" }],
  ["reading-pane-below", 1360, 860, "dark", async (p) => { await p.click(".row-main"); }, { reading_pane: "below" }],
  ["reading-pane-below-resized", 1360, 860, "light", async (p) => {
    await p.click(".row-main");
    const y = await p.locator(".list-pane").evaluate((el) => el.getBoundingClientRect().bottom);
    await p.mouse.move(800, y); await p.mouse.down(); await p.mouse.move(800, y - 150, { steps: 4 });
  }, { reading_pane: "below" }],
  ["settings-appearance-pane", 1360, 860, "dark", async (p) => { await p.click('button[title="Settings"]'); await p.click('.dialog nav button:text-is("Appearance")'); }],
  ["multi-select", 1360, 860, "dark", async (p) => {
    const rows = p.locator(".row-main");
    await rows.nth(0).click(); await rows.nth(2).click({ modifiers: ["Control"] }); await rows.nth(4).click({ modifiers: ["Control"] });
    console.log("CHECK selected rows:", await p.locator(".row.selected").count(), "(expect 3)");
  }],
  ["move-picker", 1360, 860, "dark", async (p) => { await p.click(".row-main"); await p.click("button:has-text('Move')"); }],
  ["drag-to-folder", 1360, 860, "dark", async (p) => {
    const rows = p.locator(".row-main");
    await rows.nth(0).click();
    await p.locator(".row").nth(3).dragTo(p.locator(".folder", { hasText: "Invoices" }));
    await p.waitForTimeout(300);
    const calls = await p.evaluate(() => window.__calls ?? []);
    const moved = calls.find((c) => c[0] === "move_to_folder");
    console.log("CHECK drag moved ids:", JSON.stringify(moved?.[1]?.ids), "to folder", moved?.[1]?.folderId, "(expect the 4th row, not the open one)");
  }],
  ["conversation", 1360, 860, "dark", async (p) => { await p.click(".row-main"); await p.click("summary:has-text('conversation')"); }],
  ["drafts", 1360, 860, "dark", async (p) => { await p.click("button:has-text('Unsent drafts')"); }],
  ["snooze-menu", 1360, 860, "dark", async (p) => { await p.click(".row-main"); await p.keyboard.press("b"); }],
  ["snoozed-view", 1360, 860, "dark", async (p) => { await p.click("button:has-text('Snoozed')"); await p.click(".row-main"); }],
  ["scheduled-view", 1360, 860, "light", async (p) => { await p.click("button:has-text('Scheduled')"); }],
  ["send-later", 1360, 860, "dark", async (p) => { await p.click(".compose-btn"); await p.fill("#compose-to", "daniel@hartdigital.example"); await p.click("button:has-text('Send later')"); }],
  ["calendar", 1360, 860, "dark", async (p) => { await p.click(".modes button:has-text('Calendar')"); await p.waitForTimeout(200); await p.locator(".agenda .item").first().click(); }],
  ["calendar-light-900", 900, 600, "light", async (p) => { await p.click(".modes button:has-text('Calendar')"); }],
  ["event-editor", 1360, 860, "dark", async (p) => { await p.click(".modes button:has-text('Calendar')"); await p.click("button:has-text('New event')"); }],
  ["contacts", 1360, 860, "dark", async (p) => { await p.click(".modes button:has-text('Contacts')"); await p.click("li button >> nth=0"); }],
  ["contact-edit", 1360, 860, "light", async (p) => { await p.click(".modes button:has-text('Contacts')"); await p.click("li button >> nth=0"); await p.click(".card button:has-text('Edit')"); }],
  ["about-update", 1360, 860, "dark", async (p) => {
    await p.evaluate(() => (window.__UPDATE__ = true));
    await p.click('button[title="Settings"]'); await p.click('.dialog nav button:text-is("About")');
    await p.click("button:has-text('Check for updates')"); await p.waitForTimeout(300);
  }],
  ["about", 1360, 860, "dark", async (p) => { await p.click('button[title="Settings"]'); await p.click('.dialog nav button:text-is("About")'); }],
  ["shortcuts", 1360, 860, "dark", async (p) => { await p.click('button[title="Settings"]'); await p.click('.dialog nav button:text-is("Shortcuts & search")'); await p.locator(".content").evaluate((el) => (el.scrollTop = 400)); }],
  ["add-account-gmail", 1360, 860, "dark", async (p) => {
    await p.click('button[title="Settings"]'); await p.click('.dialog nav button:text-is("Accounts")');
    await p.click('.dialog button:text-is("Add account")'); await p.fill('input[type="email"]', "hello@brightledger.example");
    await p.click('.dialog button:text-is("Continue")'); await p.click('.dialog button:text-is("Edit server settings")');
  }],
  ["compose", 1360, 860, "dark", async (p) => { await p.click(".row-main"); await p.click("button:has-text('Reply all')"); }],
  ["compose-moved", 1360, 860, "dark", async (p) => {
    await p.click(".compose-btn");
    const h = await p.locator(".compose header").boundingBox();
    await p.mouse.move(h.x + 200, h.y + 15); await p.mouse.down(); await p.mouse.move(h.x - 80, h.y - 60, { steps: 8 }); await p.mouse.up();
    const g = await p.locator(".grip.se").boundingBox();
    await p.mouse.move(g.x + 6, g.y + 6); await p.mouse.down(); await p.mouse.move(g.x - 200, g.y - 150, { steps: 8 }); await p.mouse.up();
  }],
  ["compose-maximized", 1360, 860, "dark", async (p) => { await p.click(".compose-btn"); await p.click(".compose header button[title='Maximise']"); }],
];
for (const [name, w, h, theme, act, extra] of shots) {
  const page = await browser.newPage({ viewport: { width: w, height: h }, colorScheme: theme });
  await page.addInitScript(`window.__THEME__=${JSON.stringify(theme)};window.__SETTINGS__=${JSON.stringify(extra ?? {})};(${mock.toString()})()`);
  page.on("pageerror", (e) => console.log(name, "PAGE ERROR", e.message));
  await page.goto("http://localhost:4317/");
  await page.waitForSelector(".row-main");
  await act(page);
  await page.waitForTimeout(400);
  // Report anything wider than the window and any scroll container that cannot scroll.
  const report = await page.evaluate(() => {
    const out = [];
    if (document.documentElement.scrollWidth > innerWidth) out.push(`page overflows horizontally by ${document.documentElement.scrollWidth - innerWidth}px`);
    for (const sel of [".sidebar", ".list-pane .list", ".content"]) {
      const el = document.querySelector(sel);
      if (el) out.push(`${sel}: client ${el.clientHeight} / scroll ${el.scrollHeight} / scrollTop ${el.scrollTop}`);
    }
    return out;
  });
  console.log(name, report.join(" | "));
  await page.screenshot({ path: `${out}/${name}.png` });
  await page.close();
}
await browser.close();
server.close();
