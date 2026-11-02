<script lang="ts">
  // Griglia crosspoint virtualizzata: righe = ingressi, colonne = uscite.
  // Disegna solo le celle visibili (+ margine), così regge anche matrici da centinaia di porte.
  import { tick } from 'svelte';
  import { app } from '../../lib/store.svelte';
  import Icon from '../Icon.svelte';

  let {
    rows,
    cols,
    current,
    staged,
    fan,
    ontoggle,
  }: {
    rows: number[];
    cols: number[];
    current: (out: number) => number;
    staged: Map<number, number>;
    fan: number[];
    ontoggle: (input: number, output: number) => void;
  } = $props();

  const CELL = 30;
  const OVER = 4;
  let body = $state<HTMLDivElement | null>(null);
  let st = $state(0);
  let sl = $state(0);
  let vw = $state(800);
  let vh = $state(500);
  let hr = $state(-1); // riga sotto il mouse (indice)
  let hc = $state(-1);
  let fr = $state(0); // cella col focus (roving tabindex)
  let fc = $state(0);

  const r0 = $derived(Math.max(0, Math.floor(st / CELL) - OVER));
  const r1 = $derived(Math.min(rows.length, Math.ceil((st + vh) / CELL) + OVER));
  const c0 = $derived(Math.max(0, Math.floor(sl / CELL) - OVER));
  const c1 = $derived(Math.min(cols.length, Math.ceil((sl + vw) / CELL) + OVER));
  const vr = $derived(range(r0, r1));
  const vc = $derived(range(c0, c1));

  function range(a: number, b: number) {
    const r: number[] = [];
    for (let i = a; i < b; i++) r.push(i);
    return r;
  }

  function eff(out: number) {
    return staged.has(out) ? staged.get(out)! : current(out);
  }

  // Non chiamarla «state»: in runes mode una variabile locale di nome `state`
  // trasforma ogni `$state(...)` in una sottoscrizione a store e la griglia esplode.
  function cellState(inp: number, out: number): 'on' | 'add' | 'del' | '' {
    const cur = current(out) === inp;
    const e = eff(out) === inp;
    if (cur && e) return 'on';
    if (!cur && e) return 'add';
    if (cur && !e) return 'del';
    return '';
  }

  function onScroll() {
    if (!body) return;
    st = body.scrollTop;
    sl = body.scrollLeft;
  }

  function locked(p: number) {
    return !!app.labels.get(p)?.locked;
  }

  function cellLabel(inp: number, out: number) {
    const s = cellState(inp, out);
    const what =
      s === 'on' ? 'collegato' : s === 'add' ? 'da collegare' : s === 'del' ? 'da scollegare' : 'non collegato';
    return `Ingresso ${inp} ${app.name(inp)} verso uscita ${out} ${app.name(out)}: ${what}${locked(out) ? ', uscita bloccata' : ''}`;
  }

  async function focusCell(r: number, c: number) {
    fr = Math.max(0, Math.min(rows.length - 1, r));
    fc = Math.max(0, Math.min(cols.length - 1, c));
    if (!body) return;
    const top = fr * CELL;
    const left = fc * CELL;
    if (top < body.scrollTop) body.scrollTop = top;
    else if (top + CELL > body.scrollTop + vh) body.scrollTop = top + CELL - vh;
    if (left < body.scrollLeft) body.scrollLeft = left;
    else if (left + CELL > body.scrollLeft + vw) body.scrollLeft = left + CELL - vw;
    onScroll();
    await tick();
    body.querySelector<HTMLElement>(`[data-r="${fr}"][data-c="${fc}"]`)?.focus();
  }

  function onKey(e: KeyboardEvent) {
    const t = e.target as HTMLElement;
    if (!t.dataset.r) return;
    const page = Math.max(1, Math.floor(vh / CELL) - 1);
    const map: Record<string, [number, number]> = {
      ArrowUp: [-1, 0],
      ArrowDown: [1, 0],
      ArrowLeft: [0, -1],
      ArrowRight: [0, 1],
      PageUp: [-page, 0],
      PageDown: [page, 0],
    };
    const d = map[e.key];
    if (d) {
      e.preventDefault();
      void focusCell(fr + d[0], fc + d[1]);
    } else if (e.key === 'Home') {
      e.preventDefault();
      void focusCell(fr, 0);
    } else if (e.key === 'End') {
      e.preventDefault();
      void focusCell(fr, cols.length - 1);
    }
  }

  // Evidenzia riga e colonna della cella puntata o messa a fuoco.
  // `focus` non fa bubbling: per la tastiera serve `focusin`, altrimenti chi
  // naviga con le frecce non vede mai a quale ingresso/uscita è arrivato.
  function onOver(e: Event) {
    const t = (e.target as HTMLElement | null)?.closest<HTMLElement>('[data-r]');
    if (t) {
      hr = Number(t.dataset.r);
      hc = Number(t.dataset.c);
    }
  }

  function onFocusOut(e: FocusEvent) {
    const to = e.relatedTarget as Node | null;
    if (!body || !to || !body.contains(to)) {
      hr = -1;
      hc = -1;
    }
  }
</script>

<div class="mx" style="--cell:{CELL}px">
  <div class="corner">
    <span class="c-out">Uscite <Icon name="arrowRight" size={13} /></span>
    <span class="c-in">Ingressi <Icon name="down" size={13} /></span>
  </div>

  <div class="colhead" aria-hidden="true">
    <div class="inner" style="width:{cols.length * CELL}px; transform: translateX({-sl}px)">
      {#each vc as ci (cols[ci])}
        {@const p = cols[ci]}
        <div class="ch" class:hot={ci === hc} class:lock={locked(p)} style="left:{ci * CELL}px" title={`Uscita ${p}: ${app.name(p)}`}>
          <span class="chn">{app.labels.get(p)?.name || ''}</span>
          {#if locked(p)}<Icon name="lock" size={12} />{/if}
          <span class="chp num">{p}</span>
        </div>
      {/each}
    </div>
  </div>

  <div class="rowhead" aria-hidden="true">
    <div class="inner" style="height:{rows.length * CELL}px; transform: translateY({-st}px)">
      {#each vr as ri (rows[ri])}
        {@const p = rows[ri]}
        <div class="rh" class:hot={ri === hr} style="top:{ri * CELL}px" title={`Ingresso ${p}: ${app.name(p)}`}>
          <span class="rhp num">{p}</span>
          <span class="rhn">{app.labels.get(p)?.name || `Porta ${p}`}</span>
          {#if fan[p] > 1}<span class="multi num" title={`Ingresso su ${fan[p]} uscite`}>×{fan[p]}</span>{/if}
        </div>
      {/each}
    </div>
  </div>

  <!--
    Contenitore scorrevole con role="grid": gli ascoltatori stanno qui, non sulle
    celle, perché la griglia è virtualizzata. L'interazione vera è sui <button>
    interni, quindi l'avviso non segnala un difetto.
  -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="body"
    bind:this={body}
    bind:clientWidth={vw}
    bind:clientHeight={vh}
    onscroll={onScroll}
    onmouseover={onOver}
    onmouseleave={() => ((hr = -1), (hc = -1))}
    onfocusin={onOver}
    onfocusout={onFocusOut}
    onkeydown={onKey}
    role="grid"
    aria-label="Matrice ingressi per uscite"
    aria-rowcount={rows.length}
    aria-colcount={cols.length}
    tabindex="-1"
  >
    <div class="space" style="width:{cols.length * CELL}px; height:{rows.length * CELL}px">
      {#if hr >= r0 && hr < r1}<div class="band-r" style="top:{hr * CELL}px; width:{cols.length * CELL}px"></div>{/if}
      {#if hc >= c0 && hc < c1}<div class="band-c" style="left:{hc * CELL}px; height:{rows.length * CELL}px"></div>{/if}
      {#each vr as ri (rows[ri])}
        {@const inp = rows[ri]}
        {#each vc as ci (cols[ci])}
          {@const out = cols[ci]}
          {@const s = cellState(inp, out)}
          {@const lk = locked(out)}
          <button
            class="cell {s}"
            class:lock={lk}
            class:diag={inp === out}
            style="top:{ri * CELL}px; left:{ci * CELL}px"
            data-r={ri}
            data-c={ci}
            tabindex={ri === fr && ci === fc ? 0 : -1}
            disabled={lk}
            aria-label={cellLabel(inp, out)}
            aria-pressed={s === 'on' || s === 'add'}
            onclick={() => ((fr = ri), (fc = ci), ontoggle(inp, out))}
          ><span class="mark"></span></button>
        {/each}
      {/each}
    </div>
  </div>
</div>

<style>
  .mx {
    --rhw: 210px;
    --chh: 128px;
    display: grid;
    grid-template-columns: var(--rhw) 1fr;
    grid-template-rows: var(--chh) 1fr;
    height: 100%;
    min-height: 0;
    border: 1px solid var(--ui-divider);
    border-radius: var(--r-md);
    overflow: hidden;
    background: var(--ui-raised);
  }
  .corner {
    border-right: 1px solid var(--ui-border);
    border-bottom: 1px solid var(--ui-border);
    background: var(--ui-surface);
    position: relative;
    font-size: 12px;
    font-weight: 700;
    color: var(--ui-text-muted);
  }
  .c-out {
    position: absolute;
    right: 10px;
    top: 10px;
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .c-in {
    position: absolute;
    left: 12px;
    bottom: 8px;
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .colhead,
  .rowhead {
    overflow: hidden;
    position: relative;
    background: var(--ui-surface);
  }
  .colhead {
    border-bottom: 1px solid var(--ui-border);
  }
  .rowhead {
    border-right: 1px solid var(--ui-border);
  }
  .inner {
    position: relative;
    height: 100%;
    will-change: transform;
  }
  .rowhead .inner {
    width: 100%;
  }
  .ch {
    position: absolute;
    top: 0;
    bottom: 0;
    width: var(--cell);
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: flex-end;
    gap: 4px;
    padding-bottom: 6px;
    border-right: 1px solid var(--ui-divider);
  }
  .chn {
    writing-mode: vertical-rl;
    transform: rotate(180deg);
    font-size: 12px;
    max-height: 88px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--ui-text);
  }
  .chp,
  .rhp {
    font-size: 12px;
    font-weight: 700;
    color: var(--ui-text-muted);
  }
  .ch.hot,
  .rh.hot {
    background: var(--ui-selected-bg);
  }
  .ch.hot .chp,
  .rh.hot .rhp {
    color: var(--ui-selected-text);
  }
  .ch :global(svg) {
    color: var(--ui-text-muted);
  }
  .rh {
    position: absolute;
    left: 0;
    right: 0;
    height: var(--cell);
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 8px 0 6px;
    border-bottom: 1px solid var(--ui-divider);
    font-size: 13px;
  }
  .rhp {
    width: 30px;
    text-align: right;
    flex: none;
  }
  .rhn {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .multi {
    flex: none;
    font-size: 11.5px;
    font-weight: 700;
    padding: 0 5px;
    border-radius: var(--r-sm);
    background: var(--ui-selected-bg);
    color: var(--ui-selected-text);
  }
  .body {
    overflow: auto;
    position: relative;
    min-height: 0;
    outline: none;
  }
  .space {
    position: relative;
    background-image: linear-gradient(to right, var(--ui-divider) 1px, transparent 1px),
      linear-gradient(to bottom, var(--ui-divider) 1px, transparent 1px);
    background-size: var(--cell) var(--cell);
    background-position: -1px -1px;
  }
  .band-r,
  .band-c {
    position: absolute;
    background: var(--ui-hover);
    pointer-events: none;
  }
  .band-r {
    left: 0;
    height: var(--cell);
  }
  .band-c {
    top: 0;
    width: var(--cell);
  }
  .cell {
    position: absolute;
    width: var(--cell);
    height: var(--cell);
    padding: 0;
    border: 0;
    background: transparent;
    cursor: pointer;
    display: grid;
    place-items: center;
  }
  .cell:hover:not(:disabled) .mark {
    border: 1.5px solid var(--ui-text-muted);
  }
  .cell:focus-visible {
    outline: 2px solid var(--ui-focus-ring);
    outline-offset: -2px;
    z-index: 1;
  }
  .cell.diag {
    background: var(--ui-hover);
  }
  .cell.lock {
    cursor: not-allowed;
    background: repeating-linear-gradient(45deg, transparent 0 4px, var(--ui-hover) 4px 6px);
  }
  .mark {
    width: 14px;
    height: 14px;
    border-radius: 50%;
    pointer-events: none;
  }
  .cell.on .mark {
    background: var(--ui-primary);
    border: 0;
  }
  .cell.add {
    background: var(--ui-selected-bg);
  }
  .cell.add .mark {
    background: transparent;
    border: 2px solid var(--ui-primary);
    box-shadow: inset 0 0 0 2px var(--ui-selected-bg), inset 0 0 0 6px var(--ui-primary);
  }
  .cell.del {
    background: repeating-linear-gradient(45deg, var(--ui-hatch) 0 3px, transparent 3px 7px);
  }
  .cell.del .mark {
    border: 2px solid var(--ui-danger-fg);
    background: transparent;
  }
  .cell.lock.on .mark {
    background: var(--ui-text-muted);
  }
</style>
