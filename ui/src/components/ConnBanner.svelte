<script lang="ts">
  // Messaggi «umani» sullo stato della connessione, con Riprova / Connetti.
  import { app } from '../lib/store.svelte';
  import Icon from './Icon.svelte';

  const conn = $derived(app.snapshot.conn);
  const profile = $derived(
    app.config?.profiles.find((p) => p.id === (conn.profileId ?? app.activeProfile?.id)) ?? app.activeProfile,
  );
  const name = $derived(profile?.name ?? 'matrice');

  const humanError = $derived.by(() => {
    if (!profile) return 'Nessuna matrice configurata.';
    if (profile.transport === 'tcp')
      return `Matrice non raggiungibile: controlla il cavo di rete e l’indirizzo ${profile.host || '(non impostato)'}.`;
    if (profile.transport === 'serial')
      return `Matrice non raggiungibile: controlla il cavo seriale e la porta ${profile.serialPort || '(non impostata)'}.`;
    return 'Il simulatore non risponde: riprova.';
  });
</script>

{#if profile && conn.status !== 'connected'}
  <div class="bar {conn.status}" role={conn.status === 'error' ? 'alert' : 'status'}>
    {#if conn.status === 'connecting' || conn.status === 'handshaking'}
      <Icon name="loader" spin size={18} />
      <span class="msg"><strong>Connessione a {name}…</strong>
        {#if app.expert && conn.endpoint}<span class="muted">{conn.endpoint}</span>{/if}</span>
    {:else if conn.status === 'reconnecting'}
      <Icon name="loader" spin size={18} />
      <span class="msg"><strong>Collegamento con {name} interrotto.</strong> Nuovo tentativo in corso…</span>
    {:else if conn.status === 'error'}
      <Icon name="errorCircle" size={18} />
      <span class="msg">
        <strong>{humanError}</strong>
        {#if conn.error}<span class="tech">Dettaglio: {conn.error}</span>{/if}
      </span>
      <button class="btn sm primary" onclick={() => app.connect(profile.id)}>
        <Icon name="refresh" size={15} /> Riprova
      </button>
      <button class="btn sm ghost" onclick={() => (app.screen = 'impostazioni')}>Impostazioni matrice</button>
    {:else}
      <Icon name="plugOff" size={18} />
      <span class="msg"><strong>Non connesso a {name}.</strong> I comandi alla matrice sono disattivati.</span>
      <button class="btn sm primary" onclick={() => app.connect(profile.id)}>
        <Icon name="plug" size={15} /> Connetti
      </button>
    {/if}
  </div>
{/if}

<style>
  .bar {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 9px 28px;
    min-height: 48px;
    border-bottom: 1px solid var(--ui-divider);
    background: var(--ui-surface);
    font-size: 14px;
  }
  .bar > :global(svg) {
    color: var(--ui-text-muted);
  }
  .bar.error {
    background: var(--ui-danger-bg);
    border-bottom-color: transparent;
  }
  .bar.error > :global(svg) {
    color: var(--ui-danger-fg);
  }
  .bar.reconnecting {
    background: var(--ui-warning-bg);
  }
  .bar.reconnecting > :global(svg) {
    color: var(--ui-warning-fg);
  }
  .msg {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    flex-wrap: wrap;
    gap: 2px 10px;
    align-items: baseline;
  }
  .tech {
    font-size: 12.5px;
    color: var(--ui-text-muted);
  }
</style>
