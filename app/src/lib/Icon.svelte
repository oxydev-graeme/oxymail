<script>
  /** Stroke icons on a 24px grid. */
  let { name, size = 18 } = $props();

  const paths = {
    archive: "M3 7h18M5 7v11a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V7M4 4h16v3H4zM10 12h4",
    trash: "M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13M10 11v6M14 11v6",
    star: "M12 3.5l2.6 5.3 5.9.9-4.3 4.1 1 5.8L12 16.9l-5.2 2.7 1-5.8L3.5 9.7l5.9-.9z",
    mail: "M3 6h18v12H3zM3 7l9 6 9-6",
    open: "M3 10l9-6 9 6v9H3zM3 10l9 6 9-6",
    inbox: "M3 13h5l1.5 3h5L16 13h5M5 5h14l2 8v6H3v-6z",
    restore: "M4 12a8 8 0 1 0 2.4-5.7M4 4v4h4",
    settings:
      "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z",
    search: "M11 18a7 7 0 1 0 0-14 7 7 0 0 0 0 14zM20 20l-4-4",
    minimize: "M6 12h12",
    maximize: "M6 6h12v12H6z",
    close: "M6 6l12 12M18 6L6 18",
    clip: "M20 11.5l-8.2 8.2a5 5 0 0 1-7.1-7.1l8.5-8.5a3.3 3.3 0 0 1 4.7 4.7l-8.5 8.5a1.7 1.7 0 0 1-2.4-2.4l7.8-7.8",
    x: "M7 7l10 10M17 7L7 17",
    send: "M21 3L10 14M21 3l-7 18-4-7-7-4z",
    draft: "M14 3H6v18h12V7zM14 3v4h4M9 13h6M9 17h4",
    folder: "M3 6h6l2 2h10v11H3z",
    spam: "M12 3l9 16H3zM12 10v4M12 17h.01",
    check: "M4 12l5 5L20 6",
    checks: "M2 12l5 5L18 6M12 16l1 1L22 8",
    pen: "M4 20h4L19 9l-4-4L4 16zM14 6l4 4",
    refresh: "M20 11a8 8 0 0 0-14.6-4.6L4 8M4 4v4h4M4 13a8 8 0 0 0 14.6 4.6L20 16M20 20v-4h-4",
    reply: "M10 8L4 13l6 5M4 13h10a6 6 0 0 1 6 6",
    replyAll: "M13 8l-5 5 5 5M8 8l-5 5 5 5M8 13h7a6 6 0 0 1 6 6",
    forward: "M14 8l6 5-6 5M20 13H10a6 6 0 0 0-6 6",
    plus: "M12 5v14M5 12h14",
  };
</script>

<svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"
     stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
  <path d={paths[name]} />
</svg>
