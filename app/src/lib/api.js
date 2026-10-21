import { invoke } from "@tauri-apps/api/core";

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
};

export const DEFAULT_SETTINGS = {
  theme: "system",
  density: "comfortable",
  mark_read_on_open: "true",
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
 * Wraps message HTML for a sandboxed iframe. The CSP blocks scripts and every remote
 * request (tracking pixels, remote images, fonts); only inline styles and data: images load.
 */
export function messageDocument(html) {
  return `<!doctype html><html><head><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src data:; style-src 'unsafe-inline'">
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
