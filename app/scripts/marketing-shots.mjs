// Usage: npm run build && node scripts/marketing-shots.mjs dist <out folder>
// The screenshots for getoxymail.com: the same fake backend as screenshots.mjs, with a believable
// (invented) week of a small design studio's mail, at 2x for sharp images. No real people or data.
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
  const colors = ["#0e7490", "#7c3aed", "#d63f8c", "#c2410c", "#15803d"];
  const names = ["hello@fernstudio.example", "maya@fernstudio.example", "maya.reid@gmail.example", "accounts@fernstudio.example", "maya@ridgewayclub.example"];
  const labels = ["Studio", "Maya @ Fern", "Personal", "Accounts", "Ridgeway Club"];
  const accounts = names.map((name, i) => ({ id: i + 1, name, kind: "imap", label: labels[i], sender_name: "Maya Reid",
    color: colors[i], signature: "<b>Maya Reid</b><div>Fern Studio · fernstudio.example</div>", icon: null, has_archive: false, unread: [4, 2, 3, 0, 1][i] }));
  const folderNames = [["Inbox", "inbox"], ["Drafts", "drafts"], ["Sent", "sent"], ["Archive", "archive"], ["Clients", null], ["Receipts", null], ["Spam", "spam"], ["Trash", "trash"]];
  const folders = (aid) => folderNames.map(([name, role], j) => ({ id: aid * 100 + j, account_id: aid, name, role, total: 400 - j * 30, unread: j === 0 ? [4, 2, 3, 0, 1][aid - 1] : 0 }));
  const now = Math.floor(Date.now() / 1000);
  const mail = [
    ["Lerato Mokoena", "Brand refresh: final logo files", "Here they are, in every size you asked for. The SVGs are in the shared folder and I've added a dark version for the app icon.", true, 2, true],
    ["Sam Fischer", "Lunch on Thursday?", "The new place by the river has a table at one. Shall I book it for the three of us?", true, 0, false],
    ["Ridgeway Club", "Saturday fixtures and the barbecue", "Under-11s kick off at nine. Bring a chair for the barbecue afterwards; the club brings the food.", false, 0, false],
    ["Priya Naidoo", "Photos from the weekend", "A few favourites attached, the rest are in the album. The one of the dog on the rocks is my new screensaver.", true, 3, false],
    ["Daniel Okafor", "Re: Website launch checklist", "All green on my side. Happy for you to press go whenever the client signs off on the homepage copy.", false, 0, true],
    ["Fern Studio Calendar", "Invitation: Studio planning, Mon 10:00", "Board room · 1 hour. Agenda: Q1 projects, the new hire, and who's bringing the croissants.", false, 0, false],
    ["Northwind Coffee", "Your order is on its way", "Two bags of the Ethiopian Yirgacheffe are out for delivery today between 14:00 and 17:00.", false, 0, false],
    ["Amara Botha", "Quote for the Harbour Lane project", "Thanks for the detailed brief. My quote is attached; it covers design, two rounds of changes and handover.", false, 1, false],
    ["Tom Lindqvist", "Re: Typeface licence", "Good news: the desktop licence covers the whole studio, so there's nothing extra to buy for the new hire.", false, 0, false],
    ["The Weekly Brief", "Five ideas worth stealing this week", "A calmer onboarding flow, honest pricing pages, and why the best inbox is the one you barely notice.", false, 0, false],
    ["Jo Adeyemi", "Can we move our call?", "Something's come up at school pickup. Would 15:30 work instead of 14:00? Same link.", false, 0, false],
    ["Hollis Print Co.", "Proofs ready for approval", "The business card proofs are ready. Please check the spot colour on the back before we print 500.", false, 1, false],
  ];
  const msgs = (fid) => Array.from({ length: 36 }, (_, k) => {
    const [from_name, subject, snippet, unread, attachments, important] = mail[k % mail.length];
    const round = Math.floor(k / mail.length);
    return { id: fid * 1000 + k, folder_id: fid, folder_name: "Inbox", folder_role: "inbox", account_id: Math.floor(fid / 100),
      date: now - (k * 5400 + 900), important, from_name, from_addr: from_name.toLowerCase().replace(/[^a-z]+/g, ".") + "@example.com",
      subject: round ? `Re: ${subject}` : subject, snippet, unread: round === 0 && unread, starred: k === 4, attachments, thread_key: `t${k % mail.length}` };
  });
  const view = (id) => ({ id, account_id: 1, message_id: "brand@fern", references: [], reply_to: [], subject: "Brand refresh: final logo files",
    from: ["Lerato Mokoena <lerato@mokoena-design.example>"], to: ["hello@fernstudio.example"], cc: ["sam@fernstudio.example"],
    date: now - 900,
    html: `<div style="font-family:Georgia,serif;font-size:16px;line-height:1.65;color:#1f2933;max-width:620px">
      <p>Hi Maya,</p>
      <p>Here they are, in every size you asked for. The SVGs are in the shared folder, and I've added a dark version of the mark for the app icon, since the navy disappeared on dark docks.</p>
      <p>Two small things I'd still like your eye on:</p>
      <ul><li>the spacing between the mark and the wordmark at small sizes (I've tightened it below 32&nbsp;px), and</li><li>whether the amber in the favicon is too warm next to the browser's own colours.</li></ul>
      <p>If you're happy, I'll send the brand sheet to the printers on Friday.</p>
      <p>Warmly,<br>Lerato</p></div>`,
    text: null, attachments: [{ index: 0, name: "Fern-logo-final.zip", content_type: "application/zip", size: 2480000 }, { index: 1, name: "Brand-sheet.pdf", content_type: "application/pdf", size: 812000 }],
    notices: [], from_addr: "lerato@mokoena-design.example", has_remote_content: false, images_allowed: true, unsubscribe: [] });
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
    suggest_contacts: () => [{ email: "lerato@mokoena-design.example", name: "Lerato Mokoena" }, { email: "lindiwe@example.com", name: "Lindiwe Khumalo" }],
    thread: ({ id }) => [
      { id: 900001, folder_id: 102, folder_name: "Sent", account_id: 1, date: now - 90000, from_name: "Jane Smith", subject: "Tax Invoice - INV0000123", snippet: "", unread: false, starred: false, attachments: 0 },
      { id, folder_id: 100, folder_name: "Inbox", account_id: 1, date: now - 3600, from_name: "Maria from Coastline Appliances", subject: "RE: Tax Invoice - INV0000123", snippet: "", unread: false, starred: false, attachments: 1 },
    ],
    snoozed: () => msgs(100).slice(0, 3).map((m, k) => ({ ...m, snoozed_until: now + (k + 1) * 86400 })),
    scheduled: () => [{ id: 1, account_id: 1, send_at: now + 7200, mail: "{}", draft: "{}", summary: "amara@botha.example — Harbour Lane quote", error: null }],
    drafts: () => [],
    calendars: () => [
      { id: 1, name: "Studio", color: "#0e7490", visible: true, events: 120 },
      { id: 2, name: "Family", color: "#7c3aed", visible: true, events: 40 },
      { id: 3, name: "Birthdays", color: "#c2410c", visible: true, events: 250 },
      { id: 4, name: "Studio holidays", color: "#15803d", visible: false, events: 30 },
    ],
    occurrences: () => {
      const day = (offset, h, m = 0) => { const d = new Date(); d.setDate(d.getDate() + offset); d.setHours(h, m, 0, 0); return Math.floor(d / 1000); };
      const allDay = (offset) => { const d = new Date(); d.setDate(d.getDate() + offset); return Math.floor(Date.UTC(d.getFullYear(), d.getMonth(), d.getDate()) / 1000); };
      const ev = (id, cal, color, summary, start, end, all_day = false, recurring = false) => ({ event_id: id, calendar_id: cal, color, summary, location: id === 2 ? "Board room" : null, start, end, all_day, recurring, cancelled: false });
      return [
        ev(1, 3, "#c2410c", "Studio offsite", allDay(3), allDay(4), true),
        ev(2, 1, "#0e7490", "Studio stand-up", day(0, 9), day(0, 9, 30), false, true),
        ev(3, 1, "#0e7490", "Client check-in", day(0, 13), day(0, 14), false, true),
        ev(4, 2, "#7c3aed", "Swimming lessons", day(0, 17), day(0, 18), false, true),
        ev(5, 1, "#0e7490", "Harbour Lane kick-off", day(1, 10), day(1, 11)),
        ev(6, 2, "#7c3aed", "Dinner with the Smiths", day(2, 19), day(2, 21)),
        ev(7, 1, "#0e7490", "Send invoices", day(-2, 8), day(-2, 9)),
        ev(8, 1, "#0e7490", "Proofs with Hollis Print", day(0, 15), day(0, 16)),
      ];
    },
    event: ({ id }) => ({ id, calendar_id: 1, summary: "Studio stand-up", description: "Agenda in the shared folder.", location: "Teams", start: 0, end: 1800, all_day: false, rrule: "RRULE:FREQ=WEEKLY;BYDAY=TU", organizer: "Daniel Hart <daniel@hartdigital.example>", attendees: [{ name: "Jane Smith", email: "g@x", status: "ACCEPTED" }, { name: null, email: "team@hartdigital.example", status: "NEEDS-ACTION" }], status: null }),
    people: () => [
      { id: 1, full_name: "Lerato Mokoena", org: "Mokoena Design", email: "lerato@mokoena-design.example", phone: "+1 555 0141", photo: null },
      { id: 2, full_name: "Sam Fischer", org: "Fern Studio", email: "sam@fernstudio.example", phone: null, photo: null },
      { id: 3, full_name: "Priya Naidoo", org: null, email: "priya@example.com", phone: "+1 555 0188", photo: null },
      { id: 4, full_name: "Daniel Okafor", org: "Okafor Digital", email: "daniel@okafor.example", phone: null, photo: null },
      { id: 5, full_name: "Amara Botha", org: "Botha & Co.", email: "amara@botha.example", phone: "+1 555 0102", photo: null },
    ],
    person: () => ({ id: 1, full_name: "Lerato Mokoena", first_name: "Lerato", last_name: "Mokoena", org: "Mokoena Design", title: "Brand designer", notes: "Did the Fern rebrand. Prefers calls before 11.", birthday: null, photo: null, groups: "Clients", fields: [{ kind: "email", label: "Work", value: "lerato@mokoena-design.example" }, { kind: "phone", label: "Mobile", value: "+1 555 0141" }, { kind: "url", label: "Website", value: "https://mokoena-design.example" }] }),
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
const open = async (p) => { await p.click(".row-main"); await p.waitForTimeout(300); };
const shots = [
  ["inbox-dark", 1440, 900, "dark", async (p) => { await p.click("button:has-text('All inboxes')"); await open(p); }],
  ["inbox-light", 1440, 900, "light", async (p) => { await p.click("button:has-text('All inboxes')"); await open(p); }],
  ["conversations", 1440, 900, "dark", open, { group_conversations: "true" }],
  ["snooze", 1440, 900, "dark", async (p) => { await open(p); await p.keyboard.press("b"); }],
  ["filter", 1440, 900, "dark", async (p) => { await open(p); await p.click(".toolbar button:has-text('Filter')"); }],
  ["calendar", 1440, 900, "dark", async (p) => { await p.click(".modes button:has-text('Calendar')"); await p.waitForTimeout(200); }],
  ["contacts", 1440, 900, "light", async (p) => { await p.click(".modes button:has-text('Contacts')"); await p.click("li button >> nth=0"); }],
  ["compose", 1440, 900, "dark", async (p) => { await open(p); await p.click("button:has-text('Reply all')"); await p.waitForTimeout(200); }],
  ["below", 1440, 900, "light", open, { reading_pane: "below" }],
  ["settings-general", 1440, 900, "dark", async (p) => { await p.click('button[title="Settings"]'); }],
];
for (const [name, w, h, theme, act, extra] of shots) {
  const page = await browser.newPage({ viewport: { width: w, height: h }, colorScheme: theme, deviceScaleFactor: 2 });
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
  // The site serves WebP; convert with: cwebp -q 82 in.png -o out.webp
  await page.close();
}
await browser.close();
server.close();
