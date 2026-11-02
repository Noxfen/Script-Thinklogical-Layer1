<script lang="ts">
  import { app } from '../lib/store.svelte';
  import Icon from './Icon.svelte';

  const icons: Record<string, string> = { ok: 'checkCircle', warn: 'alert', error: 'errorCircle', info: 'info' };
  const kindLabel: Record<string, string> = { ok: 'Fatto', warn: 'Attenzione', error: 'Errore', info: 'Info' };
</script>

<div class="toasts" aria-live="polite" aria-relevant="additions">
  {#each app.toasts as t (t.id)}
    <div class="toast {t.kind}" role={t.kind === 'error' ? 'alert' : 'status'}>
      <Icon name={icons[t.kind]} size={20} stroke={2} label={kindLabel[t.kind]} />
      <div class="body">
        <p class="title">{t.title}</p>
        {#each t.lines as l}
          <p class="line">{l}</p>
        {/each}
        {#if t.warnings.length}
          <ul class="warns">
            {#each t.warnings as w}
              <li><Icon name="alert" size={14} stroke={2} /><span>{w}</span></li>
            {/each}
          </ul>
        {/if}
        {#if t.details.length}
          <details>
            <summary>Dettagli comandi ({t.details.length})</summary>
            <ul>
              {#each t.details as d}<li class="mono">{d}</li>{/each}
            </ul>
          </details>
        {/if}
      </div>
      <button class="btn ghost sm icon-only" aria-label="Chiudi notifica" onclick={() => app.dismiss(t.id)}>
        <Icon name="x" size={16} />
      </button>
    </div>
  {/each}
</div>

<style>
  .toasts {
    position: fixed;
    right: 18px;
    bottom: 18px;
    z-index: 50;
    display: flex;
    flex-direction: column;
    gap: 10px;
    width: min(420px, calc(100vw - 36px));
    pointer-events: none;
  }
  .toast {
    pointer-events: auto;
    display: flex;
    gap: 12px;
    align-items: flex-start;
    padding: 12px 8px 12px 14px;
    border-radius: var(--r-md);
    background: var(--ui-raised);
    border: 1px solid var(--ui-divider);
    border-left: 4px solid var(--ui-text-muted);
    box-shadow: var(--ui-shadow);
    animation: in var(--t) ease-out;
  }
  .toast.ok {
    border-left-color: var(--ui-success-fg);
  }
  .toast.ok > :global(svg) {
    color: var(--ui-success-fg);
  }
  .toast.warn {
    border-left-color: var(--ui-warning-fg);
  }
  .toast.warn > :global(svg) {
    color: var(--ui-warning-fg);
  }
  .toast.error {
    border-left-color: var(--ui-danger-fg);
  }
  .toast.error > :global(svg) {
    color: var(--ui-danger-fg);
  }
  .toast.info > :global(svg) {
    color: var(--ui-link);
  }
  .body {
    flex: 1;
    min-width: 0;
    padding-top: 1px;
  }
  .title {
    font-weight: 700;
    overflow-wrap: anywhere;
  }
  .line {
    font-size: 13px;
    color: var(--ui-text-muted);
    margin-top: 2px;
  }
  .warns {
    margin: 8px 0 0;
    padding: 6px 8px;
    list-style: none;
    background: var(--ui-warning-bg);
    border-radius: var(--r-sm);
    max-height: none;
  }
  .warns li {
    display: flex;
    gap: 6px;
    align-items: flex-start;
    font-size: 13px;
    font-weight: 700;
    color: var(--ui-text);
    padding: 2px 0;
  }
  .warns li :global(svg) {
    color: var(--ui-warning-fg);
    margin-top: 2px;
  }
  details {
    margin-top: 6px;
    font-size: 12.5px;
  }
  summary {
    cursor: pointer;
    color: var(--ui-link);
  }
  details ul {
    margin: 6px 0 0;
    padding: 0;
    list-style: none;
    max-height: 140px;
    overflow: auto;
  }
  details li {
    padding: 1px 0;
    font-size: 12px;
  }
  @keyframes in {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
  }
</style>
