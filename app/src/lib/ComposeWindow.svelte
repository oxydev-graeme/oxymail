<script>
  // Compose in a window of its own, so the main window stays free to read and copy from.
  // The main window owns sending (with its undo toast), scheduling and the drafts list; this
  // window asks it for the draft when it opens and hands the finished message back.
  import { onMount } from "svelte";
  import { emitTo, listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import Compose from "./Compose.svelte";

  let { label } = $props();
  let data = $state(null);
  let compose = $state();
  let closing = false;
  const win = getCurrentWindow();

  /** Tells the main window, then closes this one. */
  async function finish(event, payload) {
    closing = true;
    await emitTo("main", event, { label, ...payload }).catch(() => {});
    await win.destroy().catch(() => {});
  }

  onMount(() => {
    let stop = [];
    (async () => {
      stop.push(await listen("compose-window-draft", ({ payload }) => {
        if (payload.label !== label || data) return;
        data = payload;
        const root = document.documentElement;
        if (payload.settings.theme === "system") delete root.dataset.theme;
        else root.dataset.theme = payload.settings.theme;
        getCurrentWebview().setZoom((Number(payload.settings.app_zoom) || 100) / 100).catch(() => {});
      }));
      // The window's own ✕ keeps the message as a draft, like the Close button.
      stop.push(await win.onCloseRequested(async (event) => {
        if (closing || !compose) return;
        event.preventDefault();
        await compose.close();
      }));
      await emitTo("main", "compose-window-ready", { label });
    })();
    return () => stop.forEach((f) => f());
  });

  // Keeps the window's title (taskbar, Alt+Tab) in step with the subject.
  function retitle(subject) {
    win.setTitle(subject ? `${subject} — OxyMail` : "New message — OxyMail").catch(() => {});
  }
</script>

{#if data}
  <Compose
    bind:this={compose}
    windowed
    draft={data.draft}
    accounts={data.accounts}
    settings={data.settings}
    onsubject={retitle}
    onsend={(accountId, mail, reopen) => finish("compose-window-send", { accountId, mail, reopen })}
    onschedule={(accountId, mail, reopen, at) => finish("compose-window-schedule", { accountId, mail, reopen, at })}
    onclose={(kept) => finish("compose-window-closed", { kept })}
  />
{/if}
