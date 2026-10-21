import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

/** Asks GitHub for a newer signed release. Resolves to the update, or null when up to date. */
export async function findUpdate() {
  return (await check()) ?? null;
}

/** Downloads, verifies and installs an update, reporting progress (0–1), then restarts. */
export async function installUpdate(update, onProgress = () => {}) {
  let total = 0;
  let done = 0;
  await update.downloadAndInstall((event) => {
    if (event.event === "Started") total = event.data.contentLength ?? 0;
    if (event.event === "Progress") {
      done += event.data.chunkLength;
      if (total) onProgress(done / total);
    }
  });
  await relaunch();
}
