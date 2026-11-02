<script lang="ts">
  import { SvelteMap } from 'svelte/reactivity';
  import { api } from '../../lib/api';
  import { app } from '../../lib/store.svelte';
  import type { RouteChange } from '../../lib/types';
  import { fanout, inputOf, isTypingTarget, matchPort } from '../../lib/util';
  import Icon from '../Icon.svelte';
  import MatrixGrid from './MatrixGrid.svelte';

  const VIEW_KEY = 'tlx.matrixView';
  function loadView(): 'grid' | 'list' {
    try {
      return localStorage.getItem(VIEW_KEY) === 'list' ? 'list' : 'grid';
    } catch {
      return 'grid';
    }
  }
  let view = $state<'grid' | 'list'>(loadView());
  $effect(() => {
    try {
      localStorage.setItem(VIEW_KEY, view);
    } catch {
      /* ignora */
    }
  });

  let q = $state('');
  let group = $state('');
  let onlyConnected = $state(false);
  let editingOut = $state<number | null>(null);
  /** modifiche in attesa: uscita → nuovo ingresso (0 = scollega) */
  const staged = new SvelteMap<number, number>();

  const routing = $derived(app.snapshot.routing);
  const fan = $derived(fanout(routing));
  const all = $derived(Array.from({ length: app.portCount }, (_, i) => i + 1));
  const groups = $derived([...new Set(app.labelList.map((l) => l.group).filter(Boolean))].sort());

  function base(p: number) {
    const l = app.labels.get(p);
    return matchPort(q, p, l) && (!group || l?.group === group);
  }
  const stagedInputs = $derived(new Set(staged.values()));
  const rows = $derived(all.filter((p) => base(p) && (!onlyConnected || fan[p] > 0 || stagedInputs.has(p))));
  const cols = $derived(all.filter((p) => base(p) && (!onlyConnected || inputOf(routing, p) > 0 || staged.has(p))));

  const cur = (out: number) => inputOf(routing, out);
  const changes = $derived<RouteChange[]>(
    [...staged.entries()].filter(([o, i]) => cur(o) !== i).map(([output, input]) => ({ input, output })),
  );

  function toggle(inp: number, out: number) {
    if (app.labels.get(out)?.locked) return;
    const e = staged.has(out) ? staged.get(out)! : cur(out);
    const next = e === inp ? 0 : inp;
    if (next === cur(out)) staged.delete(out);
    else staged.set(out, next);
  }

  function setOut(out: number, inp: number) {
    if (inp === cur(out)) staged.delete(out);
    else staged.set(out, inp);
    editingOut = null;
  }

  async function commit() {
    if (!changes.length) return;
    const r = await app.act(() => api.applyRoutes(changes));
    if (r) staged.clear();
  }

  function onKey(e: KeyboardEvent) {
    if (app.dialog) return;
    if (e.key === 'Escape') {
      if (editingOut != null) editingOut = null;
      else if (!isTypingTarget(e.target)) staged.clear();
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="page fill">
  <div class="page-head">
    <div>
      <h1>Matrice</h1>
      <p>Clicca un incrocio per preparare un collegamento; le modifiche partono solo con «Esegui».</p>
    </div>
    <div class="seg" role="radiogroup" aria-label="Vista">
      <button role="radio" aria-checked={view === 'grid'} onclick={() => (view = 'grid')}><Icon name="matrix" size={15} /> Griglia</button>
      <button role="radio" aria-checked={view === 'list'} onclick={() => (view = 'list')}><Icon name="list" size={15} /> Elenco uscite</button>
    </div>
  </div>

  <div class="filters row">
    <div class="search">
      <Icon name="search" size={16} />
      <input class="input" type="search" placeholder="Cerca nome, porta o gruppo" bind:value={q} aria-label="Cerca" />
    </div>
    <select class="select grp" bind:value={group} aria-label="Gruppo">
      <option value="">Tutti i gruppi</option>
      {#each groups as g (g)}<option value={g}>{g}</option>{/each}
    </select>
    <label class="check"><input type="checkbox" bind:checked={onlyConnected} /> Solo porte collegate</label>
    <span class="spacer"></span>
    <span class="legend small muted">
      <span class="lg on"></span> collegato
      <span class="lg add"></span> da collegare
      <span class="lg del"></span> da scollegare
      <Icon name="lock" size={13} /> bloccata
    </span>
  </div>

  <div class="area">
    {#if !routing}
      <div class="empty card">Connettiti alla matrice per vedere i collegamenti.</div>
    {:else if view === 'grid'}
      {#if rows.length && cols.length}
        <MatrixGrid {rows} {cols} current={cur} {staged} {fan} ontoggle={toggle} />
      {:else}
        <div class="empty card">Nessuna porta corrisponde ai filtri.</div>
      {/if}
    {:else}
      <div class="listwrap card flush">
        <table class="table">
          <thead>
            <tr>
              <th class="num">Uscita</th>
              <th>Nome</th>
              <th>Gruppo</th>
              <th>Ingresso attuale</th>
              <th>Nuovo ingresso</th>
            </tr>
          </thead>
          <tbody>
            {#each cols as out (out)}
              {@const c = cur(out)}
              {@const lk = !!app.labels.get(out)?.locked}
              {@const has = staged.has(out) && staged.get(out) !== c}
              <tr class:pend={has}>
                <td class="num"><strong>{out}</strong></td>
                <td>{app.name(out)} {#if lk}<Icon name="lock" size={13} label="Bloccata" />{/if}</td>
                <td class="muted">{app.labels.get(out)?.group ?? ''}</td>
                <td>{#if c}<span class="num">{c}</span> {app.name(c)}{:else}<span class="muted">Scollegata</span>{/if}</td>
                <td class="newcell">
                  {#if editingOut === out}
                    <!-- svelte-ignore a11y_autofocus -->
                    <select
                      class="select num"
                      autofocus
                      value={staged.get(out) ?? c}
                      onchange={(e) => setOut(out, Number((e.currentTarget as HTMLSelectElement).value))}
                      onblur={() => (editingOut = null)}
                      aria-label={`Nuovo ingresso per l’uscita ${out}`}
                    >
                      <option value={0}>Scollegata</option>
                      {#each all as p (p)}<option value={p}>{p} – {app.name(p)}</option>{/each}
                    </select>
                  {:else}
                    <button class="btn sm pick" disabled={lk} onclick={() => (editingOut = out)}>
                      {#if has}
                        {@const v = staged.get(out)!}
                        <span class="badge accent"><Icon name="dot" size={11} stroke={3} />{v ? `${v} – ${app.name(v)}` : 'Scollega'}</span>
                      {:else}
                        <span class="muted">Cambia…</span>
                      {/if}
                    </button>
                    {#if has}
                      <button class="btn ghost sm icon-only" aria-label="Annulla modifica" onclick={() => staged.delete(out)}><Icon name="x" size={14} /></button>
                    {/if}
                  {/if}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </div>

  <div class="commitbar" class:show={changes.length > 0} aria-live="polite">
    <span>
      {#if changes.length}
        <strong class="num">{changes.length}</strong> {changes.length === 1 ? 'modifica in attesa' : 'modifiche in attesa'}
      {:else}
        <span class="muted">Nessuna modifica in attesa.</span>
      {/if}
    </span>
    <div class="row">
      <button class="btn" disabled={!changes.length} onclick={() => staged.clear()} title="Esc">Annulla modifiche</button>
      <button class="btn primary lg" disabled={!changes.length || !app.connected || app.busy} onclick={commit}>
        <Icon name="check" size={18} /> Esegui ({changes.length})
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
  .page-head {
    margin-bottom: 12px;
  }
  .filters {
    margin-bottom: 12px;
  }
  .search {
    position: relative;
    width: 260px;
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
  .grp {
    width: 200px;
  }
  .spacer {
    flex: 1;
  }
  .legend {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .lg {
    display: inline-block;
    width: 13px;
    height: 13px;
    border-radius: 50%;
    margin-left: 8px;
  }
  .lg.on {
    background: var(--ui-primary);
  }
  .lg.add {
    border: 2px solid var(--ui-primary);
    background: var(--ui-selected-bg);
  }
  .lg.del {
    border: 2px solid var(--ui-danger-fg);
    background: repeating-linear-gradient(45deg, var(--ui-hatch) 0 2px, transparent 2px 4px);
  }
  .area {
    flex: 1 1 auto;
    min-height: 0;
  }
  .empty {
    padding: 40px;
    text-align: center;
    color: var(--ui-text-muted);
  }
  .listwrap {
    height: 100%;
    overflow: auto;
  }
  tr.pend td {
    background: var(--ui-selected-bg);
  }
  .newcell {
    display: flex;
    align-items: center;
    gap: 4px;
    min-width: 260px;
  }
  .newcell .select {
    max-width: 300px;
  }
  .pick {
    min-width: 160px;
    justify-content: flex-start;
    font-weight: 400;
  }
  .commitbar {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    margin: 12px -28px 0;
    padding: 10px 28px;
    border-top: 1px solid var(--ui-divider);
    background: var(--ui-bg);
  }
  .commitbar.show {
    background: var(--ui-selected-bg);
    border-top-color: var(--ui-selected-border);
  }
</style>
