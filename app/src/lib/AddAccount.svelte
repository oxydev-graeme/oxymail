<script>
  import { api } from "./api.js";
  import ServerFields from "./ServerFields.svelte";

  /** Focuses the first empty field when each step appears. */
  function focusFirst(node) {
    node.querySelector("input:not([value]), input")?.focus();
  }

  let { onadded, oncancel } = $props();

  let step = $state("email");
  let email = $state("");
  let password = $state("");
  let senderName = $state("");
  let config = $state(null);
  let busy = $state(false);
  let error = $state("");
  let showAdvanced = $state(false);

  async function detect(event) {
    event.preventDefault();
    error = "";
    busy = true;
    try {
      config = await api.discoverAccount(email);
      step = "connect";
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function connect(event) {
    event.preventDefault();
    error = "";
    busy = true;
    try {
      config.imap.port = Number(config.imap.port);
      config.smtp.port = Number(config.smtp.port);
      await api.addAccount(config, password, senderName || null);
      onadded();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  const providerName = { google: "Google", microsoft: "Microsoft" };
</script>

<button class="back" onclick={oncancel}>‹ All accounts</button>
<h2>Add an account</h2>

{#if step === "email"}
  <form onsubmit={detect} use:focusFirst>
    <label>
      <span class="label">Email address</span>
      <input type="email" bind:value={email} placeholder="you@example.com" required />
    </label>
    <p class="hint">OxyMail looks up the server settings for you.</p>
    {#if error}<p class="error">{error}</p>{/if}
    <div class="actions">
      <button class="primary" disabled={busy || !email.includes("@")}>{busy ? "Looking up…" : "Continue"}</button>
    </div>
  </form>
{:else}
  <form onsubmit={connect} use:focusFirst>
    <div class="found">
      <div><b>{config.imap.host}</b> found via {config.source}.</div>
      <div class="muted">
        Incoming IMAP {config.imap.port} ({config.imap.security === "tls" ? "TLS" : "STARTTLS"}) · Outgoing SMTP
        {config.smtp.port} ({config.smtp.security === "tls" ? "TLS" : "STARTTLS"})
      </div>
    </div>

    {#if config.oauth}
      <div class="note">
        {providerName[config.oauth] ?? "This provider"} normally uses its own sign-in, which OxyMail doesn't support
        yet. Use an <b>app password</b> instead: turn on 2-step verification, create an app password
        {#if config.oauth === "google"}at myaccount.google.com/apppasswords{/if}, and paste it below.
      </div>
    {/if}

    <div class="grid">
      <label>
        <span class="label">{config.oauth ? "App password" : "Password"}</span>
        <input type="password" bind:value={password} required />
      </label>
      <label>
        <span class="label">Your name when sending</span>
        <input bind:value={senderName} placeholder="e.g. Graeme Bosworth" />
      </label>
    </div>
    <p class="hint">The password is kept in your desktop keyring, not in OxyMail's files.</p>

    <button type="button" class="link" onclick={() => (showAdvanced = !showAdvanced)}>
      {showAdvanced ? "Hide" : "Edit"} server settings
    </button>
    {#if showAdvanced}
      <div class="advanced"><ServerFields bind:config /></div>
    {/if}

    {#if error}<p class="error">{error}</p>{/if}
    <div class="actions">
      <button type="button" class="ghost" onclick={() => (step = "email")}>Back</button>
      <button class="primary" disabled={busy || !password}>{busy ? "Checking sign-in…" : "Add account"}</button>
    </div>
    <p class="hint">
      OxyMail downloads the newest 1,000 messages per folder, then checks for new mail every three minutes. It only
      reads from the server; changing read and starred status is the one thing it writes back.
    </p>
  </form>
{/if}

<style>
  h2 { margin: 0 0 14px; font-size: 17px; }
  .back { color: var(--accent); font-size: 13px; padding: 0; margin-bottom: 10px; }
  .label { display: block; font-size: 13px; font-weight: 600; margin-bottom: 6px; }
  .hint { margin: 6px 0 0; font-size: 12px; color: var(--muted); line-height: 1.5; }
  .muted { color: var(--muted); font-size: 12px; }
  input {
    width: 100%;
    padding: 7px 10px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg);
    color: var(--text);
    outline: none;
  }
  input:focus { border-color: var(--accent); }
  .grid { display: grid; grid-template-columns: 1fr 1fr; gap: 14px; margin-top: 14px; }
  .found { padding: 10px 12px; border-radius: 8px; background: var(--accent-soft); font-size: 13px; }
  .note { margin-top: 12px; padding: 10px 12px; border-radius: 8px; background: var(--warn-bg); color: var(--warn-text); font-size: 13px; line-height: 1.5; }
  .error { margin: 12px 0 0; padding: 8px 10px; border-radius: 8px; background: color-mix(in srgb, var(--danger) 12%, transparent); color: var(--danger); font-size: 13px; user-select: text; }
  .actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 16px; }
  .primary { padding: 7px 16px; border-radius: 8px; background: var(--accent); color: #fff; font-weight: 600; font-size: 13px; }
  .primary:disabled { opacity: 0.6; cursor: default; }
  .ghost { padding: 7px 14px; border-radius: 8px; border: 1px solid var(--border); font-size: 13px; }
  .link { margin-top: 12px; padding: 0; color: var(--accent); font-size: 13px; }
  .advanced { margin-top: 10px; }
</style>
