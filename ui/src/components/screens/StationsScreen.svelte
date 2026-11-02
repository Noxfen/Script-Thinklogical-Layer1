<script lang="ts">
  // Pannello destinazione → sorgente (stile Videohub Smart Control / AJA KUMO).
  import { api } from '../../lib/api';
  import { app } from '../../lib/store.svelte';
  import { fanout, inputOf, isTypingTarget, matchPort } from '../../lib/util';
  import Icon from '../Icon.svelte';

  let q = $state('');
  let dest = $state<number | null>(null);
  let pending = $state<number | null>(null);
  let videoOnly = $state(false);
  let instant = $state(false);

  const all = $derived(Array.from({ length: app.portCount }, (_, i) => i + 1));
  const hasRoles = $derived(app.labelList.some((l) => l.role === 'tx' || l.role === 'rx'));
  const dests = $derived(hasRoles ? all.filter((p) => app.labels.get(p)?.role === 'rx') : all);
  const sources = $derived(hasRoles ? all.filter((p) => app.labels.get(p)?.role === 'tx') : all);
  const fDests = $derived(dests.filter((p) => matchPort(q, p, app.labels.get(p))));
  const fSources = $derived(sources.filter((p) => matchPort(q, p, app.labels.get(p))));
  const routing = $derived(app.snapshot.routing);
  const fan = $derived(fanout(routing));
  const current = $derived(dest ? inputOf(routing, dest) : 0);
  const canTake = $derived(
    app.connected && !app.busy && dest != null && pending != null && pending !== dest && pending !== current,
  );

  function groups(list: number[]) {
    const m = new Map<string, number[]>();
    for (const p of list) {
      const g = app.labels.get(p)?.group || '';
      if (!m.has(g)) m.set(g, []);
      m.get(g)!.push(p);
    }
    return [...m.entries()];
  }
  const destGroups = $derived(groups(fDests));
  const srcGroups = $derived(groups(fSources));

  function locked(p: number) {
    return !!app.labels.get(p)?.locked;
  }

  function pickDest(p: number) {
    if (locked(p)) return;
    dest = dest === p ? null : p;
    pending = null;
  }
  function pickSource(p: number) {
    if (locked(p) || dest == null) return;
    pending = p;
    if (instant) void take();
  }

  async function take() {
    if (dest == null || pending == null || pending === dest) return;
    if (pending === current) {
      pending = null;
      return;
    }
    const a = pending;
    const b = dest;
    const r = await app.act(() => api.link(a, b, videoOnly ? 'oneway' : 'kvm'));
    if (r) pending = null;
  }

  function cancel() {
    if (pending != null) pending = null;
    else dest = null;
  }

  function clearDest() {
    if (dest == null) return;
    const b = dest;
    void app.act(() => api.disconnectPort(b, 'both'));
  }

  function onKey(e: KeyboardEvent) {
    if (app.dialog) return;
    if (e.key === 'Escape') {
      if (isTypingTarget(e.target) && q) return;
      cancel();
    } else if (e.key === 'Enter' && !isTypingTarget(e.target)) {
      if (canTake) {
        e.preventDefault();
        void take();
      }
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="page fill">
  <div class="page-head">
    <div>
      <h1>Postazioni</h1>
      <p>1. Scegli la postazione. 2. Scegli il PC da mostrare. 3. Premi «Collega» (o Invio).</p>
    </div>
    <div class="row">
      <div class="search">
        <Icon name="search" size={16} />
        <input class="input" type="search" placeholder="Cerca nome, porta o gruppo" bind:value={q} aria-label="Cerca" />
      </div>
      <label class="switch" title="Collega solo l’immagine, senza tastiera e mouse">
        <input type="checkbox" bind:checked={videoOnly} /> Solo video
      </label>
      <label class="switch" title="Collega appena scegli il PC, senza premere Collega">
        <input type="checkbox" bind:checked={instant} /> Collega subito
      </label>
    </div>
  </div>

  {#if !hasRoles}
    <div class="callout info hint">
      <Icon name="info" />
      <p>Nessuna porta è segnata come «PC» o «Postazione»: sono mostrate tutte in entrambe le colonne.
        {#if app.expert}Assegna i ruoli nella schermata Porte.{:else}Per ordinarle, attiva la modalità tecnico in Impostazioni e assegna i ruoli nella schermata Porte.{/if}</p>
    </div>
  {/if}

  <div class="cols">
    <section class="col" aria-labelledby="h-dest">
      <h2 id="h-dest">Postazioni <span class="count num">{fDests.length}</span></h2>
      <div class="scroll">
        {#each destGroups as [g, ports] (g)}
          {#if g}<h3 class="group">{g}</h3>{/if}
          <div class="tiles">
            {#each ports as p (p)}
              {@const src = inputOf(routing, p)}
              {@const lk = locked(p)}
              <button
                class="tile"
                class:selected={dest === p}
                disabled={lk}
                aria-pressed={dest === p}
                onclick={() => pickDest(p)}
              >
                <span class="t1"><span class="pn num">{p}</span><span class="nm">{app.name(p)}</span>{#if lk}<Icon name="lock" size={15} label="Bloccata" />{/if}</span>
                <span class="t2">
                  {#if src}<Icon name="arrowRight" size={13} /> {app.name(src)}{:else}<span class="none">Nessun PC</span>{/if}
                </span>
              </button>
            {/each}
          </div>
        {:else}
          <p class="empty">Nessuna postazione trovata.</p>
        {/each}
      </div>
    </section>

    <section class="col" aria-labelledby="h-src">
      <h2 id="h-src">PC <span class="count num">{fSources.length}</span>
        {#if dest == null}<span class="muted small">scegli prima una postazione</span>{/if}</h2>
      <div class="scroll">
        {#each srcGroups as [g, ports] (g)}
          {#if g}<h3 class="group">{g}</h3>{/if}
          <div class="tiles">
            {#each ports as p (p)}
              {@const lk = locked(p)}
              {@const isCur = dest != null && current === p}
              {@const isPend = pending === p}
              <button
                class="tile src"
                class:current={isCur}
                class:pending={isPend}
                disabled={lk || dest == null || p === dest}
                aria-pressed={isPend}
                onclick={() => pickSource(p)}
              >
                <span class="t1"><span class="pn num">{p}</span><span class="nm">{app.name(p)}</span>{#if lk}<Icon name="lock" size={15} label="Bloccata" />{/if}</span>
                <span class="t2">
                  {#if isPend}
                    <span class="tag"><Icon name="dot" size={12} stroke={3} /> Da collegare</span>
                  {:else if isCur}
                    <span class="tag"><Icon name="check" size={13} stroke={2.5} /> In uso su questa postazione</span>
                  {:else if fan[p] > 0}
                    <span class="muted">Visto da {fan[p]} {fan[p] === 1 ? 'uscita' : 'uscite'}</span>
                  {:else}
                    <span class="none">Libero</span>
                  {/if}
                </span>
              </button>
            {/each}
          </div>
        {:else}
          <p class="empty">Nessun PC trovato.</p>
        {/each}
      </div>
    </section>
  </div>

  <div class="takebar" aria-live="polite">
    <div class="sel">
      {#if dest == null}
        <span class="muted">Nessuna postazione scelta.</span>
      {:else}
        <span class="big"><span class="num">{dest}</span> {app.name(dest)}</span>
        <Icon name="arrowsLR" size={18} />
        {#if pending != null}
          <span class="big"><span class="num">{pending}</span> {app.name(pending)}</span>
          <span class="chip">{videoOnly ? 'Solo video' : 'Postazione KVM'}</span>
        {:else if current}
          <span class="muted">ora mostra {app.name(current)}: scegli un altro PC</span>
        {:else}
          <span class="muted">scegli un PC</span>
        {/if}
      {/if}
    </div>
    <div class="row">
      {#if dest != null && current}
        <button class="btn" disabled={!app.connected || app.busy || locked(dest)} onclick={clearDest}>
          <Icon name="unlink" size={16} /> Scollega postazione
        </button>
      {/if}
      <button class="btn" disabled={dest == null} onclick={cancel} title="Esc">Annulla</button>
      <button class="btn primary lg take" disabled={!canTake} onclick={take}>
        <Icon name="link" size={18} /> {app.expert ? 'TAKE' : 'Collega'} <kbd>Invio</kbd>
      </button>
    </div>
  </div>
</div>

<style>
  .fill {
    height: 100%;
    display: flex;
    flex-direction: column;
    padding-bottom: 0;
    max-width: none;
  }
  .search {
    position: relative;
    width: 280px;
  }
  .search :global(svg) {
    position: absolute;
    left: 10px;
    top: 50%;
    transform: translateY(-50%);
    color: var(--ui-text-muted);
  }
  .search .input {
    padding-left: 32px;
  }
  .hint {
    margin-bottom: 12px;
  }
  .cols {
    flex: 1 1 auto;
    min-height: 0;
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 16px;
  }
  .col {
    display: flex;
    flex-direction: column;
    min-height: 0;
    border: 1px solid var(--ui-divider);
    border-radius: var(--r-md);
    background: var(--ui-surface);
  }
  .col h2 {
    display: flex;
    align-items: baseline;
    gap: 8px;
    padding: 12px 14px 10px;
    border-bottom: 1px solid var(--ui-divider);
  }
  .count {
    font-size: 12.5px;
    font-weight: 400;
    color: var(--ui-text-muted);
  }
  .scroll {
    overflow: auto;
    padding: 4px 12px 14px;
    flex: 1;
  }
  .group {
    font-size: 12.5px;
    color: var(--ui-text-muted);
    margin: 12px 2px 6px;
  }
  .tiles {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(190px, 1fr));
    gap: 8px;
    margin-top: 8px;
  }
  .group + .tiles {
    margin-top: 0;
  }
  .tile {
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-height: 58px;
    padding: 8px 12px;
    border: 1px solid var(--ui-border);
    border-radius: var(--r-md);
    background: var(--ui-raised);
    color: var(--ui-text);
    font: inherit;
    text-align: left;
    cursor: pointer;
    transition: background var(--t-fast), border-color var(--t-fast);
  }
  .tile:hover:not(:disabled) {
    border-color: var(--ui-text-muted);
  }
  .tile:disabled {
    cursor: not-allowed;
    opacity: 0.5;
  }
  .tile.selected,
  .tile.current {
    border-color: var(--ui-selected-border);
    background: var(--ui-selected-bg);
    box-shadow: inset 0 0 0 1px var(--ui-selected-border);
  }
  .tile.current:disabled {
    opacity: 1;
  }
  .tile.pending {
    border: 2px dashed var(--ui-primary);
    padding: 7px 11px;
    background: var(--ui-raised);
  }
  .t1 {
    display: flex;
    align-items: center;
    gap: 8px;
    font-weight: 700;
    min-width: 0;
  }
  .pn {
    min-width: 24px;
    color: var(--ui-text-muted);
  }
  .nm {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .t2 {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 12.5px;
    color: var(--ui-text-muted);
    padding-left: 32px;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
  }
  .tile.selected .t2,
  .tile.current .t2 {
    color: var(--ui-selected-text);
  }
  .tag {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-weight: 700;
    color: var(--ui-selected-text);
  }
  .tile.pending .tag {
    color: var(--ui-link);
  }
  .none {
    font-style: normal;
    opacity: 0.85;
  }
  .empty {
    padding: 20px 4px;
    color: var(--ui-text-muted);
  }
  .takebar {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    margin: 14px -28px 0;
    padding: 12px 28px;
    border-top: 1px solid var(--ui-divider);
    background: var(--ui-bg);
  }
  .sel {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
    flex-wrap: wrap;
  }
  .sel > :global(svg) {
    color: var(--ui-text-muted);
  }
  .big {
    font-size: 16px;
    font-weight: 700;
  }
  .big .num {
    color: var(--ui-text-muted);
    margin-right: 4px;
  }
  .take {
    min-width: 170px;
  }
  .take kbd {
    background: transparent;
    color: inherit;
    border-color: currentColor;
    opacity: 0.8;
  }
</style>
