<script lang="ts">
  import type { ConnStatus } from '../lib/types';
  import Icon from './Icon.svelte';

  let { status, expert = false }: { status: ConnStatus; expert?: boolean } = $props();

  const map = $derived.by(() => {
    switch (status) {
      case 'connected':
        return { icon: 'checkCircle', label: 'Connesso', kind: 'ok', spin: false };
      case 'connecting':
        return { icon: 'loader', label: 'Connessione…', kind: 'neutral', spin: true };
      case 'handshaking':
        return { icon: 'loader', label: expert ? 'Handshake…' : 'Connessione…', kind: 'neutral', spin: true };
      case 'reconnecting':
        return { icon: 'loader', label: 'Riconnessione…', kind: 'warn', spin: true };
      case 'error':
        return { icon: 'errorCircle', label: 'Errore', kind: 'error', spin: false };
      default:
        return { icon: 'plugOff', label: 'Disconnesso', kind: 'neutral', spin: false };
    }
  });
</script>

<span class="pill {map.kind}" role="status" aria-live="polite">
  <Icon name={map.icon} size={15} spin={map.spin} stroke={2} />
  <span>{map.label}</span>
</span>

<style>
  .pill {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 11px 0 9px;
    border-radius: 999px;
    font-size: 13px;
    font-weight: 700;
    white-space: nowrap;
    border: 1px solid var(--ui-divider);
    background: var(--ui-surface);
    color: var(--ui-text);
  }
  .pill.neutral :global(svg) {
    color: var(--ui-text-muted);
  }
  .pill.ok {
    background: var(--ui-success-bg);
    border-color: transparent;
  }
  .pill.ok :global(svg) {
    color: var(--ui-success-fg);
  }
  .pill.warn {
    background: var(--ui-warning-bg);
    border-color: transparent;
  }
  .pill.warn :global(svg) {
    color: var(--ui-warning-fg);
  }
  .pill.error {
    background: var(--ui-danger-bg);
    border-color: transparent;
  }
  .pill.error :global(svg) {
    color: var(--ui-danger-fg);
  }
</style>
