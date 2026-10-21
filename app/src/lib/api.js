import { invoke } from "@tauri-apps/api/core";
export { listen } from "@tauri-apps/api/event";

export const api = {
  accounts: () => invoke("accounts"),
  folders: (accountId) => invoke("folders", { accountId }),
  messages: (folderId, offset, limit) => invoke("messages", { folderId, offset, limit }),
  search: (query) => invoke("search", { query }),
  message: (id) => invoke("message", { id }),
  saveAttachment: (id, index) => invoke("save_attachment", { id, index }),
  importArchive: (path) => invoke("import_archive", { path }),
  setFlag: (ids, flag, on) => invoke("set_flag", { ids, flag, on }),
  moveTo: (ids, role) => invoke("move_to", { ids, role }),
  moveToFolder: (ids, folderId) => invoke("move_to_folder", { ids, folderId }),
  remove: (ids) => invoke("delete", { ids }),
  restore: (ids) => invoke("restore", { ids }),
  settings: () => invoke("settings"),
  setSetting: (key, value) => invoke("set_setting", { key, value }),
  dataLocation: () => invoke("data_location"),
  reorderAccounts: (ids) => invoke("reorder_accounts", { ids }),
  updateAccount: (id, update) => invoke("update_account", { id, update }),
  removeAccount: (id) => invoke("remove_account", { id }),
  markFolderRead: (folderId) => invoke("mark_folder_read", { folderId }),
  markAccountRead: (accountId) => invoke("mark_account_read", { accountId }),
  discoverAccount: (email) => invoke("discover_account", { email }),
  addAccount: (config, password, senderName) => invoke("add_account", { config, password, senderName }),
  syncNow: (accountId = null) => invoke("sync_now", { accountId }),
  sendMessage: (accountId, mail) => invoke("send_message", { accountId, mail }),
  liveConfig: (accountId) => invoke("live_config", { accountId }),
  setImagesAllowed: (email, allowed) => invoke("set_images_allowed", { email, allowed }),
  openUrl: (url) => invoke("open_url", { url }),
  openAttachment: (id, index) => invoke("open_attachment", { id, index }),
  thread: (id) => invoke("thread", { id }),
  calendars: () => invoke("calendars"),
  createCalendar: (name, color = null) => invoke("create_calendar", { name, color }),
  updateCalendar: (id, name, color, visible) => invoke("update_calendar", { id, name, color, visible }),
  deleteCalendar: (id) => invoke("delete_calendar", { id }),
  occurrences: (from, to) => invoke("occurrences", { from, to }),
  event: (id) => invoke("event", { id }),
  saveEvent: (event) => invoke("save_event", { event }),
  deleteEvent: (id) => invoke("delete_event", { id }),
  people: (query) => invoke("people", { query }),
  person: (id) => invoke("person", { id }),
  savePerson: (person) => invoke("save_person", { person }),
  deletePerson: (id) => invoke("delete_person", { id }),
  importPim: (paths) => invoke("import_pim", { paths }),
  snooze: (ids, until) => invoke("snooze", { ids, until }),
  snoozed: () => invoke("snoozed"),
  scheduled: () => invoke("scheduled"),
  unschedule: (id) => invoke("unschedule", { id }),
  scheduleSend: (accountId, sendAt, mail, draft) => invoke("schedule_send", { accountId, sendAt, mail, draft }),
  saveDraft: (id, accountId, subject, recipients, body) => invoke("save_draft", { id, accountId, subject, recipients, body }),
  drafts: () => invoke("drafts"),
  deleteDraft: (id) => invoke("delete_draft", { id }),
  suggestContacts: (query) => invoke("suggest_contacts", { query }),
  messagesByRole: (role, offset, limit) => invoke("messages_by_role", { role, offset, limit }),
  unreadByRole: (role) => invoke("unread_by_role", { role }),
  oauthSignIn: (provider, email) => invoke("oauth_sign_in", { provider, email }),
  oauthClients: () => invoke("oauth_clients"),
  setOAuthClient: (provider, clientId, clientSecret) => invoke("set_oauth_client", { provider, clientId, clientSecret }),
  connectSuggestion: (accountId) => invoke("connect_suggestion", { accountId }),
  connectArchive: (accountId, config, password) => invoke("connect_archive", { accountId, config, password }),
  updateServerSettings: (accountId, config, password) =>
    invoke("update_server_settings", { accountId, config, password }),
};

/** Sidebar name for an account: its label, else its address. */
export const accountLabel = (a) => a.label || a.name;

/**
 * Reads an image file and returns it as a square PNG data URI, centre-cropped and scaled to
 * `size` pixels, so account icons stay small in the database.
 */
export function imageToIcon(file, size = 96) {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(reader.error);
    reader.onload = () => {
      const img = new Image();
      img.onerror = () => reject(new Error("That file isn't an image OxyMail can read."));
      img.onload = () => {
        const side = Math.min(img.width, img.height);
        const canvas = document.createElement("canvas");
        canvas.width = canvas.height = size;
        const ctx = canvas.getContext("2d");
        ctx.imageSmoothingQuality = "high";
        ctx.drawImage(img, (img.width - side) / 2, (img.height - side) / 2, side, side, 0, 0, size, size);
        resolve(canvas.toDataURL("image/png"));
      };
      img.src = reader.result;
    };
    reader.readAsDataURL(file);
  });
}

export const DEFAULT_SETTINGS = {
  theme: "system",
  density: "comfortable",
  mark_read_on_open: "true",
  notifications: "true",
  undo_send_seconds: "8",
};

/** Today: "14:05". This year: "21 Oct". Older: "21 Oct 2023". */
export function shortDate(ts) {
  if (!ts) return "";
  const d = new Date(ts * 1000);
  const now = new Date();
  if (d.toDateString() === now.toDateString()) {
    return d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
  }
  const opts = { day: "numeric", month: "short" };
  if (d.getFullYear() !== now.getFullYear()) opts.year = "numeric";
  return d.toLocaleDateString(undefined, opts);
}

export function longDate(ts) {
  if (!ts) return "";
  return new Date(ts * 1000).toLocaleString(undefined, { dateStyle: "full", timeStyle: "short" });
}

export function formatSize(bytes) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/**
 * Wraps message HTML for a sandboxed iframe. The CSP blocks scripts and, unless `allowRemote`,
 * every remote request (tracking pixels, remote images, fonts); inline styles and data: images
 * always load.
 */
export function messageDocument(html, allowRemote = false) {
  const csp = allowRemote
    ? "default-src 'none'; img-src data: https: http:; style-src 'unsafe-inline' https: http:; font-src data: https:"
    : "default-src 'none'; img-src data:; style-src 'unsafe-inline'";
  return `<!doctype html><html><head><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="${csp}">
<base target="_blank">
<style>
  html { color-scheme: light; }
  body { margin: 0; padding: 20px 24px; font: 14px/1.5 system-ui, sans-serif; color: #1d2433; background: #fff;
         overflow-wrap: anywhere; }
  img { max-width: 100%; height: auto; }
  table { max-width: 100%; }
  pre { white-space: pre-wrap; }
</style></head><body>${html}</body></html>`;
}

export const isLive = (account) => account?.kind === "imap";

/** Reads a File as base64 without the data: prefix. */
export function fileToBase64(file) {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(reader.error);
    reader.onload = () => resolve(String(reader.result).split(",", 2)[1] ?? "");
    reader.readAsDataURL(file);
  });
}

/** Message HTML made safe to quote inside the app: no head, styles, scripts or embeds. */
export function quotableHtml(html) {
  const doc = new DOMParser().parseFromString(html, "text/html");
  doc.querySelectorAll("style, script, link, meta, title, iframe, object, embed, form").forEach((n) => n.remove());
  // Remote images would load (and report the read) inside Compose; keep only embedded ones.
  doc.querySelectorAll("img").forEach((img) => {
    if (!/^(data|cid):/i.test(img.getAttribute("src") ?? "")) img.remove();
  });
  doc.querySelectorAll("[background]").forEach((n) => n.removeAttribute("background"));
  return doc.body.innerHTML;
}

export const escapeHtml = (text) =>
  text.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);

export const PROVIDER_NAMES = { google: "Google", microsoft: "Microsoft" };

/** `mailto:a@b?subject=x&cc=y` → { to, cc, subject, body }. */
export function parseMailto(url) {
  const [address, query = ""] = url.replace(/^mailto:/i, "").split("?");
  const params = new URLSearchParams(query);
  return {
    to: decodeURIComponent(address),
    cc: params.get("cc") ?? "",
    subject: params.get("subject") ?? "",
    body: params.get("body") ?? "",
  };
}
