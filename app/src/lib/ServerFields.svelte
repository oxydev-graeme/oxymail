<script>
  /** Username plus IMAP and SMTP host, port and security, edited in place on `config`. */
  let { config = $bindable() } = $props();

  // Standard ports for each security mode, so switching mode fixes an untouched port.
  const PORTS = { imap: { tls: 993, starttls: 143 }, smtp: { tls: 465, starttls: 587 } };

  function setSecurity(key, security) {
    const standard = Object.values(PORTS[key]);
    if (standard.includes(Number(config[key].port))) config[key].port = PORTS[key][security];
    config[key].security = security;
  }
</script>

<div class="servers">
  <label class="username">
    <span class="label">Username</span>
    <input bind:value={config.username} />
  </label>
  {#each [["imap", "Incoming mail (IMAP)"], ["smtp", "Outgoing mail (SMTP)"]] as [key, title]}
    <fieldset>
      <legend>{title}</legend>
      <label class="host"><span>Server</span><input bind:value={config[key].host} /></label>
      <label class="port"><span>Port</span><input type="number" min="1" max="65535" bind:value={config[key].port} /></label>
      <label class="security">
        <span>Security</span>
        <select value={config[key].security} onchange={(e) => setSecurity(key, e.currentTarget.value)}>
          <option value="tls">SSL/TLS</option>
          <option value="starttls">STARTTLS</option>
        </select>
      </label>
    </fieldset>
  {/each}
</div>

<style>
  .servers { display: grid; gap: 14px; }
  .label { display: block; font-size: 13px; font-weight: 600; margin-bottom: 6px; }
  fieldset {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 90px 130px;
    gap: 10px;
    margin: 0;
    padding: 10px 12px 12px;
    border: 1px solid var(--border);
    border-radius: 8px;
  }
  legend { padding: 0 6px; font-size: 13px; font-weight: 600; }
  fieldset label span { display: block; margin-bottom: 4px; font-size: 12px; color: var(--muted); }
  input, select {
    width: 100%;
    padding: 7px 10px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg);
    color: var(--text);
    outline: none;
    font: inherit;
  }
  input:focus, select:focus { border-color: var(--accent); }
</style>
