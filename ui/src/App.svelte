<script lang="ts">
  import { onMount } from 'svelte';
  import { app } from './lib/store.svelte';
  import { isTypingTarget } from './lib/util';
  import TopBar from './components/TopBar.svelte';
  import Sidebar from './components/Sidebar.svelte';
  import ConnBanner from './components/ConnBanner.svelte';
  import SafetyBanner from './components/SafetyBanner.svelte';
  import Toasts from './components/Toasts.svelte';
  import Dialog from './components/Dialog.svelte';
  import Icon from './components/Icon.svelte';
  import PresetScreen from './components/screens/PresetScreen.svelte';
  import StationsScreen from './components/screens/StationsScreen.svelte';
  import MatrixScreen from './components/screens/MatrixScreen.svelte';
  import PortsScreen from './components/screens/PortsScreen.svelte';
  import DiagnosticsScreen from './components/screens/DiagnosticsScreen.svelte';
  import ConsoleScreen from './components/screens/ConsoleScreen.svelte';
  import LogScreen from './components/screens/LogScreen.svelte';
  import SettingsScreen from './components/screens/SettingsScreen.svelte';

  onMount(() => {
    void app.init();
  });

  // Tema: system/light/dark → data-theme su <html>
  $effect(() => {
    const t = app.settings?.theme ?? 'system';
    const mq = window.matchMedia('(prefers-color-scheme: dark)');
    const apply = () => {
      const resolved = t === 'system' ? (mq.matches ? 'dark' : 'light') : t;
      document.documentElement.dataset.theme = resolved;
      app.theme = resolved;
    };
    apply();
    if (t === 'system') {
      mq.addEventListener('change', apply);
      return () => mq.removeEventListener('change', apply);
    }
  });

  function onKey(e: KeyboardEvent) {
    if (app.dialog) return;
    if ((e.ctrlKey || e.metaKey) && !e.shiftKey && e.key.toLowerCase() === 'z') {
      if (isTypingTarget(e.target)) return; // annulla nativo nei campi di testo
      e.preventDefault();
      void app.undo();
    } else if (e.key === 'F5') {
      e.preventDefault();
      void app.refresh();
    }
  }
</script>

<svelte:window onkeydown={onKey} />

{#if app.loadError}
  <div class="splash">
    <div class="callout error" role="alert">
      <Icon name="errorCircle" />
      <div>
        <strong>Impossibile avviare TLX Control.</strong>
        <p>{app.loadError}</p>
      </div>
    </div>
  </div>
{:else if !app.ready}
  <div class="splash" aria-busy="true">
    <Icon name="loader" spin size={22} /> <span>Avvio di TLX Control…</span>
  </div>
{:else}
  <div class="shell">
    <TopBar />
    <div class="main">
      <Sidebar />
      <div class="content-col">
        <ConnBanner />
        <SafetyBanner />
        <main class="content" id="contenuto">
          {#key app.screen + (app.activeProfile?.id ?? '')}
            {#if app.screen === 'preset'}
              <PresetScreen />
            {:else if app.screen === 'postazioni'}
              <StationsScreen />
            {:else if app.screen === 'matrice'}
              <MatrixScreen />
            {:else if app.screen === 'porte'}
              <PortsScreen />
            {:else if app.screen === 'diagnostica'}
              <DiagnosticsScreen />
            {:else if app.screen === 'console'}
              <ConsoleScreen />
            {:else if app.screen === 'log'}
              <LogScreen />
            {:else if app.screen === 'impostazioni'}
              <SettingsScreen />
            {/if}
          {/key}
        </main>
      </div>
    </div>
  </div>
{/if}

<Toasts />
<Dialog />

<style>
  .splash {
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 10px;
    color: var(--ui-text-muted);
    padding: 24px;
  }
  .shell {
    height: 100%;
    display: grid;
    grid-template-rows: auto 1fr;
    min-height: 0;
  }
  .main {
    display: grid;
    grid-template-columns: auto 1fr;
    min-height: 0;
  }
  .content-col {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }
  .content {
    flex: 1 1 auto;
    min-height: 0;
    overflow: auto;
    position: relative;
  }
</style>
