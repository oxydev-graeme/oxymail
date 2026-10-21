<script>
  /**
   * A small rich-text editor for HTML signatures, with a raw-HTML mode. The app's CSP blocks
   * scripts and remote images, so pasted HTML can't run code; embedded data: images still show.
   */
  let { value = $bindable("") } = $props();

  let mode = $state("visual");
  let editor;

  $effect(() => {
    if (mode === "visual" && editor && editor.innerHTML !== value) editor.innerHTML = value;
  });

  function exec(command, arg = null) {
    editor.focus();
    document.execCommand(command, false, arg);
    value = editor.innerHTML;
  }

  function link() {
    const url = prompt("Link address (https://… or mailto:…)");
    if (url) exec("createLink", url);
  }
</script>

<div class="sig">
  <div class="bar">
    {#if mode === "visual"}
      <button onclick={() => exec("bold")} title="Bold"><b>B</b></button>
      <button onclick={() => exec("italic")} title="Italic"><i>I</i></button>
      <button onclick={() => exec("underline")} title="Underline"><u>U</u></button>
      <button onclick={link} title="Link">Link</button>
      <button onclick={() => exec("removeFormat")} title="Clear formatting">Clear</button>
    {/if}
    <span class="grow"></span>
    <button class:on={mode === "visual"} onclick={() => (mode = "visual")}>Visual</button>
    <button class:on={mode === "html"} onclick={() => (mode = "html")}>HTML</button>
  </div>
  {#if mode === "visual"}
    <div
      class="editor"
      contenteditable="true"
      role="textbox"
      aria-multiline="true"
      tabindex="0"
      bind:this={editor}
      oninput={() => (value = editor.innerHTML)}
    ></div>
  {:else}
    <textarea bind:value spellcheck="false"></textarea>
  {/if}
</div>

<style>
  .sig { border: 1px solid var(--border); border-radius: 8px; overflow: hidden; }
  .bar { display: flex; gap: 2px; padding: 4px; background: var(--bg); border-bottom: 1px solid var(--border); }
  .bar button { min-width: 28px; padding: 3px 8px; border-radius: 5px; font-size: 12px; color: var(--muted); }
  .bar button:hover { background: var(--hover); color: var(--text); }
  .bar button.on { background: var(--panel); color: var(--text); font-weight: 600; }
  .grow { flex: 1; }
  .editor {
    min-height: 120px;
    max-height: 240px;
    overflow: auto;
    padding: 12px 14px;
    background: #fff;
    color: #1d2433;
    font: 14px/1.5 system-ui, sans-serif;
    outline: none;
    user-select: text;
  }
  .editor :global(img) { max-width: 100%; height: auto; }
  textarea {
    display: block;
    width: 100%;
    min-height: 160px;
    padding: 10px 12px;
    border: 0;
    background: var(--panel);
    color: var(--text);
    font: 12px/1.5 ui-monospace, monospace;
    resize: vertical;
    outline: none;
  }
</style>
