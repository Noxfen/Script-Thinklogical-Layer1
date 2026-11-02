<script lang="ts">
  // Avviso di sicurezza calcolato in UI: tastiera di una postazione (ruolo rx) che comanda più PC.
  import { app } from '../lib/store.svelte';
  import Icon from './Icon.svelte';

  const items = $derived(app.kbdWarnings);
</script>

{#if items.length && app.connected}
  <div class="safety" role="alert">
    <Icon name="keyboard" size={20} />
    <div class="lines">
      {#each items as w (w.port)}
        <p>
          <strong>Attenzione: la tastiera della porta {w.port} ({app.name(w.port)}) comanda più PC.</strong>
          <span class="muted">Arriva a {w.outputs.map((o) => app.name(o)).join(', ')}.</span>
        </p>
      {/each}
    </div>
    <Icon name="alert" size={18} stroke={2} />
  </div>
{/if}

<style>
  .safety {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 9px 28px;
    background: var(--ui-warning-bg);
    border-bottom: 1px solid var(--ui-divider);
    font-size: 14px;
  }
  .safety > :global(svg) {
    color: var(--ui-warning-fg);
  }
  .lines {
    flex: 1;
    min-width: 0;
  }
  .lines p + p {
    margin-top: 2px;
  }
  .muted {
    color: var(--ui-text-muted);
  }
</style>
