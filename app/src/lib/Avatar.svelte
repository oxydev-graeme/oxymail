<script>
  import { accountLabel } from "./api.js";

  /** An account's uploaded icon, or its initial on its colour. */
  let { account, size = 26 } = $props();

  let initial = $derived((accountLabel(account).match(/[a-z0-9]/i)?.[0] ?? "?").toUpperCase());
</script>

{#if account.icon}
  <img class="avatar" src={account.icon} alt="" width={size} height={size} style:width="{size}px" style:height="{size}px" />
{:else}
  <span
    class="avatar initial"
    style:width="{size}px"
    style:height="{size}px"
    style:font-size="{Math.round(size * 0.46)}px"
    style:background={account.color || "var(--faint)"}
  >{initial}</span>
{/if}

<style>
  .avatar { display: inline-grid; place-items: center; flex-shrink: 0; border-radius: 30%; object-fit: cover; }
  .initial { color: #fff; font-weight: 700; line-height: 1; text-shadow: 0 1px 1px rgb(0 0 0 / 0.2); }
</style>
