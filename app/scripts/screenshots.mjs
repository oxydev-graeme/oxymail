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
  const names = ["hello@oxyaccounting.co.za", "oxyaccweb@gmail.com", "gbosworth@topfiremedia.com", "graemebosworth99@gmail.com",
    "itsreubenite@gmail.com", "reuben@reubenite.co.za", "studio@oxydev.co", "hello@oxydev.co.za", "graeme@oxydev.co.za",
    "accounts@oxydev.co.za", "oxydev.web@gmail.com", "franchisewebdevelopment@gmail.com", "graeme@atomiccreative.co.uk",
    "practexerp@gmail.com", "accounts@practex.co.za", "toolbecontinued@gmail.com", "graeme.bosworth@windsorcreative.org.nz", "junia@oxydev.co.za"];
  const accounts = names.map((name, i) => ({ id: i + 1, name, kind: i === 8 ? "imap" : "archive", label: null, sender_name: "Graeme Bosworth",
    color: colors[i % colors.length], signature: i === 8 ? "<b>Graeme Bosworth</b><div>Mobile: 061 000 0000</div>" : null, icon: null, has_archive: true, unread: i % 4 === 0 ? 12 * i + 3 : 0 }));
  const folderNames = [["Inbox", "inbox"], ["Drafts", "drafts"], ["Sent", "sent"], ["Archive", "archive"], ["Clients / Bay Solar", null], ["Invoices", null], ["Spam", "spam"], ["Trash", "trash"]];
  const folders = (aid) => folderNames.map(([name, role], j) => ({ id: aid * 100 + j, account_id: aid, name, role, total: 500 - j * 40, unread: j === 0 ? 7 : 0 }));
  const now = Math.floor(Date.now() / 1000);
  const msgs = (fid) => Array.from({ length: 60 }, (_, k) => ({ id: fid * 1000 + k, folder_id: fid, folder_name: "Inbox", folder_role: "inbox", account_id: Math.floor(fid / 100),
    date: now - k * 7200, from_name: ["Monica from Euro Appliances", "Asana", "OxyDev", "Darin Halford", "Google Workspace"][k % 5], from_addr: "x@example.com",
    subject: ["RE: Tax Invoice - INV0000146 for the period ending September and the rest of a long subject line", "New comment on Home page [Final Revisions]", "INV-00050 invoice viewed", "Bay Solar rebuild — next steps", "Security alert"][k % 5],
    snippet: "Hi Graeme, please find attached the updated invoice as discussed on our call this morning. Let me know if anything needs changing before month end.",
    unread: k % 3 === 0, starred: k % 7 === 0, attachments: k % 4 === 0 ? 2 : 0 }));
  const view = (id) => ({ id, account_id: 9, message_id: "abc@x", references: [], reply_to: [], subject: "RE: Tax Invoice - INV0000146",
    from: ["Monica <monica@euroappliances.co.za>"], to: ["graeme@oxydev.co.za"], cc: ["accounts@euroappliances.co.za", "someone.else.with.a.long.address@example.com"],
    date: now - 3600, html: "<p>Hi Graeme,</p>" + "<p>Please find attached the updated invoice. ".repeat(4) + "</p>".repeat(1) + Array.from({ length: 40 }, (_, i) => `<p>Line ${i + 1} of a long email body to test scrolling.</p>`).join("") + '<table width="900" border="1"><tr><td>Wide table cell</td><td>Another cell that is wide</td></tr></table>',
    text: null, attachments: [{ index: 0, name: "Invoice-0000146.pdf", content_type: "application/pdf", size: 81234 }, { index: 1, name: "Statement September 2026 with a very long file name.xlsx", content_type: "application/vnd", size: 22111 }],
    notices: ["Attachment not available: SWIFT_PO#110026171505.zip (4533 bytes)"],
    from_addr: "monica@euroappliances.co.za", has_remote_content: true, images_allowed: false,
    unsubscribe: ["https://example.com/unsubscribe"] });
  const settings = { theme: window.__THEME__ ?? "system", density: "comfortable", mark_read_on_open: "true" };
  const handlers = {
    accounts: () => accounts, folders: ({ accountId }) => folders(accountId), messages: ({ folderId, offset }) => (offset ? [] : msgs(folderId)),
    message: ({ id }) => view(id), settings: () => settings, set_setting: () => null, data_location: () => "/home/graeme/.local/share/oxymail/oxymail.db",
    search: () => [], set_flag: () => null,
    move_to_folder: (args) => { (window.__calls ??= []).push(["move_to_folder", args]); return null; },
    live_config: () => ({ email: "graeme@oxydev.co.za", username: "graeme@oxydev.co.za", source: "the provider's autoconfig", oauth: null,
      imap: { host: "mail.oxydev.co.za", port: 993, security: "tls" }, smtp: { host: "mail.oxydev.co.za", port: 465, security: "tls" } }),
    messages_by_role: () => msgs(100).map((m, k) => ({ ...m, account_id: (k % 6) + 1 })),
    suggest_contacts: () => [{ email: "paul@sarongi.co.za", name: "Paul McCree" }, { email: "plarkin@topfiremedia.com", name: "Paul Larkin" }, { email: "paul.another@example.com", name: null }],
    oauth_clients: () => ({ google: { client_id: "1234-abc.apps.googleusercontent.com", has_secret: true }, microsoft: { client_id: "", has_secret: false } }),
    connect_suggestion: () => ({ email: "hello@oxydev.co.za", username: "hello@oxydev.co.za", oauth: null,
      source: "your Mailbird settings (POP3 there; OxyMail uses IMAP on the same server)",
      imap: { host: "mail.oxydev.co.za", port: 993, security: "tls" }, smtp: { host: "mail.oxydev.co.za", port: 465, security: "tls" } }),
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
    await p.locator(".accounts li", { hasText: "graeme@oxydev.co.za" }).locator("button.edit").click();
    await p.waitForTimeout(200); await p.locator(".content").evaluate((el) => (el.scrollTop = 520));
  }],
  ["connect-archive", 1360, 860, "dark", async (p) => {
    await p.click('button[title="Settings"]'); await p.click('.dialog nav button:text-is("Accounts")');
    await p.locator(".accounts li", { hasText: "hello@oxydev.co.za" }).locator("button.edit").click();
    await p.click('.dialog button:text-is("Set up sending and receiving")');
    await p.waitForTimeout(200); await p.locator(".content").evaluate((el) => (el.scrollTop = 560));
  }],
  ["signin-settings", 1360, 860, "dark", async (p) => {
    await p.click('button[title="Settings"]'); await p.click('.dialog nav button:text-is("Sign-in")');
    await p.click('.dialog summary >> nth=0');
  }],
  ["all-inboxes", 1360, 860, "dark", async (p) => { await p.click("button:has-text('All inboxes')"); await p.click(".row-main"); }],
  ["compose-autocomplete", 1360, 860, "dark", async (p) => {
    await p.click(".compose-btn"); await p.fill("#compose-to", "pau"); await p.waitForTimeout(400);
  }],
  ["forward", 1360, 860, "dark", async (p) => { await p.click(".row-main"); await p.click("button:has-text('Forward')"); }],
  ["settings-reading", 1360, 860, "light", async (p) => { await p.click('button[title="Settings"]'); await p.click('.dialog nav button:text-is("Reading & sending")'); }],
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
  ["shortcuts", 1360, 860, "dark", async (p) => { await p.click('button[title="Settings"]'); await p.click('.dialog nav button:text-is("Shortcuts & search")'); await p.locator(".content").evaluate((el) => (el.scrollTop = 400)); }],
  ["add-account-gmail", 1360, 860, "dark", async (p) => {
    await p.click('button[title="Settings"]'); await p.click('.dialog nav button:text-is("Accounts")');
    await p.click('.dialog button:text-is("Add account")'); await p.fill('input[type="email"]', "hello@oxyaccounting.co.za");
    await p.click('.dialog button:text-is("Continue")'); await p.click('.dialog button:text-is("Edit server settings")');
  }],
  ["compose", 1360, 860, "dark", async (p) => { await p.click(".row-main"); await p.click("button:has-text('Reply all')"); }],
];
for (const [name, w, h, theme, act] of shots) {
  const page = await browser.newPage({ viewport: { width: w, height: h }, colorScheme: theme });
  await page.addInitScript(`window.__THEME__=${JSON.stringify(theme)};(${mock.toString()})()`);
  page.on("pageerror", (e) => console.log(name, "PAGE ERROR", e.message));
  await page.goto("http://localhost:4317/");
  await page.waitForSelector(".row-main");
  await act(page);
  await page.waitForTimeout(400);
  // Report anything wider than the window and any scroll container that cannot scroll.
  const report = await page.evaluate(() => {
    const out = [];
    if (document.documentElement.scrollWidth > innerWidth) out.push(`page overflows horizontally by ${document.documentElement.scrollWidth - innerWidth}px`);
    for (const sel of [".sidebar", ".list", ".content"]) {
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
