<script>
  import { PROVIDER_NAMES } from "./api.js";

  /**
   * Password field, or a note that sign-in happens in the browser when the provider supports
   * OAuth and a client is set up. `preferPassword` switches back to an app password.
   */
  let { provider = null, clientReady = false, password = $bindable(""), preferPassword = $bindable(false), passwordLabel = "Password" } = $props();

  let name = $derived(PROVIDER_NAMES[provider] ?? "");
  let oauth = $derived(Boolean(provider && clientReady && !preferPassword));
</script>

{#if oauth}
  <div class="signin">
    <div><b>Sign in with {name}.</b> Your browser opens so you can approve OxyMail; no password is stored.</div>
    <button type="button" class="link" onclick={() => (preferPassword = true)}>Use an app password instead</button>
  </div>
{:else}
  {#if provider && !clientReady}
    <div class="note">
      {name} sign-in needs a one-time setup in <b>Settings → Sign-in</b>. Until then, use an <b>app password</b>
      {#if provider === "google"}(myaccount.google.com/apppasswords, with 2-step verification on){/if}.
    </div>
  {/if}
  <label class="password">
    <span class="label">{provider ? "App password" : passwordLabel}</span>
    <input type="password" bind:value={password} />
  </label>
  {#if provider && clientReady}
    <button type="button" class="link" onclick={() => (preferPassword = false)}>Sign in with {name} instead</button>
  {/if}
{/if}

<style>
  .signin { display: grid; gap: 6px; padding: 10px 12px; border-radius: 8px; background: var(--accent-soft); font-size: 13px; line-height: 1.5; }
  .note { margin-bottom: 12px; padding: 10px 12px; border-radius: 8px; background: var(--warn-bg); color: var(--warn-text); font-size: 13px; line-height: 1.5; }
  .label { display: block; font-size: 13px; font-weight: 600; margin-bottom: 6px; }
  .password { display: block; }
  .password input { width: 100%; padding: 7px 10px; border: 1px solid var(--border); border-radius: 8px; background: var(--bg); color: var(--text); outline: none; }
  .password input:focus { border-color: var(--accent); }
  .link { justify-self: start; margin-top: 6px; padding: 0; color: var(--accent); font-size: 12px; }
</style>
