<script lang="ts">
  import { app } from '../lib/store.svelte';
  import { fmtAge } from '../lib/util';
  import Icon from './Icon.svelte';
  import StatusPill from './StatusPill.svelte';
  import logoP from '../assets/tt-group_logotipo_p.svg';
  import logoN from '../assets/tt-group_logotipo_n.svg';

  const conn = $derived(app.snapshot.conn);
  const status = $derived(conn.status);
  const busyConn = $derived(status === 'connecting' || status === 'handshaking' || status === 'reconnecting');
  const isOn = $derived(status === 'connected' || busyConn);
  const info = $derived(app.snapshot.info);
  const routing = $derived(app.snapshot.routing);
  const age = $derived(routing ? app.now - routing.updatedAt : 0);
  const staleLimitMs = $derived(Math.max(20, 3 * (app.settings?.pollXgetS ?? 0)) * 1000);
  const stale = $derived(!!routing && age > staleLimitMs);
  const sourceLabel = $derived(
    routing ? (routing.source === 'xget' ? 'XGET' : routing.source === 'udp' ? 'UDP' : 'locale') : '',
  );
  const endpoint = $derived(
    conn.endpoint ||
      (app.activeProfile?.transport === 'tcp'
        ? `${app.activeProfile.host}:${app.activeProfile.port}`
        : app.activeProfile?.transport === 'serial'
          ? app.activeProfile.serialPort
          : 'simulatore'),
  );

  function onProfile(e: Event) {
    const id = (e.currentTarget as HTMLSelectElement).value;
    void app.switchProfile(id);
  }

  function toggleConn() {
    if (isOn) void app.disconnect();
    else void app.connect();
  }
</script>

<header class="topbar">
  <div class="brand">
    <img src={app.theme === 'dark' ? logoN : logoP} alt="TT Group" class="logo" />
    <span class="appname">TLX Control</span>
  </div>

  <div class="matrix-pick">
    <label for="profilo" class="sr-only">Matrice in uso</label>
    <div class="pick-wrap" title="Matrice in uso: cambiandola, la app si scollega dalla precedente e si collega a questa">
      <Icon name="matrix" size={18} />
      <select id="profilo" class="pick" value={app.config?.activeProfileId ?? ''} onchange={onProfile}>
        {#each app.config?.profiles ?? [] as p (p.id)}
          <option value={p.id}>{p.name}</option>
        {/each}
      </select>
      <Icon name="chevronDown" size={16} />
    </div>
    <div class="conn">
      <StatusPill {status} expert={app.expert} />
      <span class="endpoint num" title="Indirizzo della matrice">{endpoint}</span>
    </div>
    <button class="btn sm" class:primary={!isOn} onclick={toggleConn} disabled={!app.activeProfile}>
      <Icon name={isOn ? 'plugOff' : 'plug'} size={16} />
      {isOn ? 'Disconnetti' : 'Connetti'}
    </button>
  </div>

  <div class="right">
    {#if info}
      <div class="meta">
        <span class="model num">{info.xtype} · {info.maxPorts} porte</span>
        {#if routing}
          <span class="fresh" class:stale title={stale ? 'Stato non aggiornato di recente: premi Aggiorna (F5)' : 'Ultimo aggiornamento dello stato'}>
            {#if stale}<Icon name="alert" size={13} stroke={2} />{/if}
            aggiornato {fmtAge(age)}{#if app.expert}&nbsp;({sourceLabel}){/if}
          </span>
        {/if}
      </div>
    {/if}
    <button
      class="btn sm"
      onclick={() => app.undo()}
      disabled={!app.snapshot.canUndo || !app.connected || app.busy}
      title="Annulla l’ultima commutazione (Ctrl+Z)"
    >
      <Icon name="undo" size={16} /> Annulla
    </button>
    <button
      class="btn sm icon-only"
      onclick={() => app.refresh()}
      disabled={!app.connected}
      title="Aggiorna lo stato dalla matrice (F5)"
      aria-label="Aggiorna lo stato dalla matrice"
    >
      <Icon name="refresh" size={16} />
    </button>
  </div>
</header>

<style>
  .topbar {
    display: flex;
    align-items: center;
    gap: 18px;
    height: 60px;
    padding: 0 16px 0 18px;
    border-bottom: 1px solid var(--ui-divider);
    background: var(--ui-bg);
    min-width: 0;
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 12px;
    flex: none;
  }
  .logo {
    height: 34px;
    width: auto;
    display: block;
    margin: -4px -6px;
  }
  .appname {
    font-weight: 700;
    font-size: 15px;
    padding-left: 12px;
    border-left: 1px solid var(--ui-divider);
    white-space: nowrap;
  }
  .matrix-pick {
    display: flex;
    align-items: center;
    gap: 12px;
    min-width: 0;
    flex: 1 1 auto;
  }
  .pick-wrap {
    position: relative;
    display: flex;
    align-items: center;
    gap: 8px;
    height: 40px;
    padding: 0 10px 0 12px;
    border: 1px solid var(--ui-border);
    border-radius: var(--r-md);
    background: var(--ui-bg);
    min-width: 200px;
    max-width: 300px;
    flex: 0 1 auto;
  }
  .pick-wrap:hover {
    border-color: var(--ui-text-muted);
  }
  .pick-wrap:focus-within {
    outline: 2px solid var(--ui-focus-ring);
    outline-offset: 2px;
  }
  .pick-wrap > :global(svg) {
    color: var(--ui-text-muted);
    pointer-events: none;
  }
  .pick {
    appearance: none;
    border: 0;
    background: transparent;
    color: var(--ui-text);
    font: inherit;
    font-size: 15px;
    font-weight: 700;
    flex: 1 1 auto;
    min-width: 0;
    height: 100%;
    cursor: pointer;
    padding-right: 4px;
  }
  .pick:focus-visible {
    outline: none;
  }
  .pick option {
    background: var(--ui-bg);
    color: var(--ui-text);
    font-weight: 400;
  }
  .conn {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .endpoint {
    color: var(--ui-text-muted);
    font-size: 12.5px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .right {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: none;
  }
  .meta {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    line-height: 1.25;
    margin-right: 6px;
  }
  .model {
    font-weight: 700;
    font-size: 13px;
    white-space: nowrap;
  }
  .fresh {
    font-size: 12px;
    color: var(--ui-text-muted);
    display: inline-flex;
    align-items: center;
    gap: 4px;
    white-space: nowrap;
  }
  .fresh.stale {
    color: var(--ui-text);
    background: var(--ui-warning-bg);
    padding: 0 6px;
    border-radius: var(--r-sm);
  }
  .fresh.stale :global(svg) {
    color: var(--ui-warning-fg);
  }
  @media (max-width: 1180px) {
    .endpoint {
      display: none;
    }
  }
</style>
