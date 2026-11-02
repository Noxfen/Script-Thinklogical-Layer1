<script lang="ts">
  import { app } from '../lib/store.svelte';
  import type { ScreenId } from '../lib/util';
  import Icon from './Icon.svelte';

  interface Item {
    id: ScreenId;
    label: string;
    icon: string;
    expert?: boolean;
  }
  const main: Item[] = [
    { id: 'preset', label: 'Preset', icon: 'presets' },
    { id: 'postazioni', label: 'Postazioni', icon: 'station' },
    { id: 'matrice', label: 'Matrice', icon: 'matrix', expert: true },
    { id: 'porte', label: 'Porte', icon: 'ports', expert: true },
    { id: 'diagnostica', label: 'Diagnostica', icon: 'pulse', expert: true },
    { id: 'console', label: 'Console', icon: 'terminal', expert: true },
  ];
  const visible = $derived(main.filter((i) => !i.expert || app.expert));
</script>

<nav class="side" aria-label="Schermate">
  <ul>
    {#each visible as it (it.id)}
      <li>
        <button
          class="nav"
          class:tech={it.expert}
          aria-current={app.screen === it.id ? 'page' : undefined}
          onclick={() => (app.screen = it.id)}
        >
          <Icon name={it.icon} size={19} />
          <span>{it.label}</span>
        </button>
      </li>
    {/each}
    <li class="sep" aria-hidden="true"></li>
    <li>
      <button
        class="nav"
        aria-current={app.screen === 'impostazioni' ? 'page' : undefined}
        onclick={() => (app.screen = 'impostazioni')}
      >
        <Icon name="settings" size={19} />
        <span>Impostazioni</span>
      </button>
    </li>
  </ul>

  <div class="foot">
    {#if app.expert}
      <button
        class="nav quiet"
        aria-current={app.screen === 'log' ? 'page' : undefined}
        onclick={() => (app.screen = 'log')}
      >
        <Icon name="list" size={16} />
        <span>Registro eventi</span>
      </button>
    {/if}
    <div class="meta">
      {#if app.expert}
        <span class="badge accent"><Icon name="tool" size={13} stroke={2} />Modalità tecnico</span>
      {/if}
      {#if app.isMock}
        <span class="badge neutral" title="Nessun backend Tauri: la matrice è simulata nel browser">
          <Icon name="info" size={13} stroke={2} />Simulazione browser
        </span>
      {/if}
      <span class="ver num">v2.0.0</span>
    </div>
  </div>
</nav>

<style>
  .side {
    width: 212px;
    background: var(--ui-surface);
    border-right: 1px solid var(--ui-divider);
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    padding: 14px 10px 12px;
    overflow: auto;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .sep {
    height: 1px;
    background: var(--ui-divider);
    margin: 10px 8px;
  }
  .nav {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    min-height: 44px;
    padding: 0 12px;
    border: 0;
    border-radius: var(--r-md);
    background: transparent;
    color: var(--ui-text);
    font: inherit;
    font-size: 14.5px;
    font-weight: 700;
    text-align: left;
    cursor: pointer;
    position: relative;
    transition: background var(--t-fast);
  }
  .nav :global(svg) {
    color: var(--ui-text-muted);
  }
  .nav:hover {
    background: var(--ui-hover);
  }
  .nav[aria-current='page'] {
    background: var(--ui-bg);
    color: var(--ui-text);
    box-shadow: inset 3px 0 0 var(--ui-primary), 0 0 0 1px var(--ui-divider);
  }
  .nav[aria-current='page'] :global(svg) {
    color: var(--ui-link);
  }
  .nav.tech {
    font-weight: 400;
  }
  .nav.tech[aria-current='page'] {
    font-weight: 700;
  }
  .nav.quiet {
    min-height: 36px;
    font-size: 13px;
    font-weight: 400;
    color: var(--ui-text-muted);
  }
  .nav.quiet[aria-current='page'] {
    color: var(--ui-text);
    font-weight: 700;
  }
  .foot {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding-top: 16px;
  }
  .meta {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 6px;
    padding: 0 10px;
  }
  .ver {
    font-size: 12px;
    color: var(--ui-text-muted);
  }
</style>
