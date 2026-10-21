<script>
  /**
   * The OxyMail mark: an M of four ribbon strokes (light blue, orange, pink, purple) on a
   * transparent background, from branding/oxymail-m-logo-transparent.svg in the vault.
   * Gradient and filter ids get a per-instance suffix, since several logos can share a page.
   */
  let { size = 24 } = $props();

  const uid = Math.random().toString(36).slice(2, 8);
  const id = (name) => `oxy-${name}-${uid}`;
  // The drop shadow is drawn for the 512 px source; keep it subtle when the mark is small.
  let shadow = $derived(size < 40 ? { dy: 6, blur: 5, opacity: 0.28 } : { dy: 8, blur: 9, opacity: 0.4 });
</script>

<svg class="mark" width={size} height={size} viewBox="76 76 360 360" aria-hidden="true">
  <defs>
    <linearGradient id={id("c1")} x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#8ADCFF" />
      <stop offset="1" stop-color="#38B6F0" />
    </linearGradient>
    <linearGradient id={id("c2")} x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stop-color="#FFC069" />
      <stop offset="1" stop-color="#FE9A00" />
    </linearGradient>
    <linearGradient id={id("c3")} x1="1" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#F57AB4" />
      <stop offset="1" stop-color="#D63F8C" />
    </linearGradient>
    <linearGradient id={id("c4")} x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#B08CFF" />
      <stop offset="1" stop-color="#7B4DE8" />
    </linearGradient>
    <filter id={id("shadow")} x="-30%" y="-30%" width="160%" height="160%">
      <feDropShadow dx="0" dy={shadow.dy} stdDeviation={shadow.blur} flood-color="#000" flood-opacity={shadow.opacity} />
    </filter>
  </defs>
  <g fill="none" stroke-width="64" stroke-linecap="round">
    <!-- Back layer: the two legs. Front: the diagonals, the pink crossing over the orange. -->
    <rect x="96" y="120" width="64" height="272" rx="32" fill="url(#{id('c1')})" stroke="none" filter="url(#{id('shadow')})" />
    <rect x="352" y="120" width="64" height="272" rx="32" fill="url(#{id('c4')})" stroke="none" filter="url(#{id('shadow')})" />
    <line x1="128" y1="152" x2="256" y2="304" stroke="url(#{id('c2')})" filter="url(#{id('shadow')})" />
    <line x1="384" y1="152" x2="256" y2="304" stroke="url(#{id('c3')})" filter="url(#{id('shadow')})" />
  </g>
</svg>

<style>
  .mark { display: block; flex-shrink: 0; overflow: visible; }
</style>
