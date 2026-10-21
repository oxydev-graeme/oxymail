<script>
  /** The OxyMail mark: an O holding an envelope flap, with the amber oxygen bubble. */
  let { size = 24 } = $props();
</script>

<svg class="mark" width={size} height={size} viewBox="0 0 64 64" aria-hidden="true">
  <circle cx="30" cy="35" r="19" fill="none" stroke="var(--brand)" stroke-width="7" />
  <path d="M17.5 28 L30 37.5 L42.5 28" fill="none" stroke="var(--brand)" stroke-width="5.5"
        stroke-linecap="round" stroke-linejoin="round" />
  <circle cx="50" cy="14" r="8.5" fill="var(--amber)" stroke="var(--header)" stroke-width="3.5" />
</svg>

<style>
  .mark { display: block; flex-shrink: 0; }
</style>
