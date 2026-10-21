<script>
  import { onMount } from "svelte";
  import { api } from "./api.js";

  let clients = $state(null);
  let drafts = $state({ google: { id: "", secret: "" }, microsoft: { id: "", secret: "" } });
  let saved = $state({});
  let errors = $state({});

  onMount(load);

  async function load() {
    clients = await api.oauthClients();
    for (const p of ["google", "microsoft"]) drafts[p] = { id: clients[p].client_id, secret: "" };
  }

  async function save(provider) {
    errors[provider] = "";
    try {
      const secret = drafts[provider].secret.trim();
      await api.setOAuthClient(provider, drafts[provider].id, secret ? secret : null);
      saved[provider] = true;
      await load();
      setTimeout(() => (saved[provider] = false), 2500);
    } catch (e) {
      errors[provider] = String(e);
    }
  }
</script>

<h2>Sign-in</h2>
<p class="hint top">
  Google and Microsoft only let apps sign in to mail with their own registration. Create one once (free), paste
  its details here, and accounts on that provider can use <b>Sign in with Google / Microsoft</b> instead of app
  passwords. Secrets are kept in your system's secure storage.
</p>

{#if clients}
  <section>
    <h3>Google <span class:ready={clients.google.client_id} class="state">{clients.google.client_id ? "Set up" : "Not set up"}</span></h3>
    <label><span class="label">Client ID</span><input bind:value={drafts.google.id} placeholder="…apps.googleusercontent.com" /></label>
    <label>
      <span class="label">Client secret</span>
      <input type="password" bind:value={drafts.google.secret} placeholder={clients.google.has_secret ? "Saved — leave blank to keep" : ""} />
    </label>
    <div class="row">
      <button class="primary" onclick={() => save("google")}>Save</button>
      {#if saved.google}<span class="ok">Saved.</span>{/if}
      {#if errors.google}<span class="bad">{errors.google}</span>{/if}
    </div>
    <details>
      <summary>How to get these</summary>
      <ol>
        <li>Go to console.cloud.google.com and create a project (e.g. “OxyMail”).</li>
        <li>APIs &amp; Services → OAuth consent screen: choose <b>External</b>, fill in the app name and your email.</li>
        <li>Under <b>Test users</b>, add every Gmail / Workspace address you'll use.</li>
        <li>Credentials → Create credentials → OAuth client ID → Application type <b>Desktop app</b>.</li>
        <li>Copy the client ID and client secret into the fields above.</li>
        <li>
          While the consent screen is in <b>Testing</b>, Google ends sign-ins after 7 days. Set it to
          <b>In production</b> to stop that (Google shows an “unverified app” warning the first time you sign in).
        </li>
      </ol>
    </details>
  </section>

  <section>
    <h3>Microsoft <span class:ready={clients.microsoft.client_id} class="state">{clients.microsoft.client_id ? "Set up" : "Not set up"}</span></h3>
    <label><span class="label">Application (client) ID</span><input bind:value={drafts.microsoft.id} placeholder="00000000-0000-…" /></label>
    <div class="row">
      <button class="primary" onclick={() => save("microsoft")}>Save</button>
      {#if saved.microsoft}<span class="ok">Saved.</span>{/if}
      {#if errors.microsoft}<span class="bad">{errors.microsoft}</span>{/if}
    </div>
    <details>
      <summary>How to get this</summary>
      <ol>
        <li>Go to entra.microsoft.com → App registrations → New registration.</li>
        <li>Supported account types: <b>any organisational directory and personal Microsoft accounts</b>.</li>
        <li>Redirect URI: platform <b>Public client/native</b>, value <code>http://localhost</code>.</li>
        <li>API permissions → Add: Office 365 Exchange Online → <b>IMAP.AccessAsUser.All</b> and <b>SMTP.Send</b>.</li>
        <li>Copy the Application (client) ID above. No secret is needed.</li>
      </ol>
    </details>
  </section>
{/if}

<style>
  h2 { margin: 0 0 6px; font-size: 17px; }
  h3 { display: flex; align-items: center; gap: 10px; margin: 0; font-size: 15px; }
  .hint { margin: 0; font-size: 12px; color: var(--muted); line-height: 1.5; }
  .top { margin-bottom: 18px; }
  section { padding: 16px 0; border-top: 1px solid var(--border); display: grid; gap: 12px; }
  .label { display: block; font-size: 13px; font-weight: 600; margin-bottom: 6px; }
  input { width: 100%; padding: 7px 10px; border: 1px solid var(--border); border-radius: 8px; background: var(--bg); color: var(--text); outline: none; }
  input:focus { border-color: var(--accent); }
  .row { display: flex; align-items: center; gap: 12px; flex-wrap: wrap; }
  .primary { padding: 7px 16px; border-radius: 8px; background: var(--accent); color: #fff; font-weight: 600; font-size: 13px; }
  .state { padding: 1px 8px; border-radius: 99px; font-size: 11px; font-weight: 600; background: var(--hover); color: var(--muted); }
  .state.ready { background: var(--accent-soft); color: var(--accent); }
  .ok { color: var(--accent); font-size: 13px; }
  .bad { color: var(--danger); font-size: 13px; user-select: text; }
  details { font-size: 13px; color: var(--muted); }
  summary { cursor: pointer; color: var(--accent); }
  ol { margin: 8px 0 0; padding-left: 20px; line-height: 1.6; }
  code { font-size: 12px; }
</style>
