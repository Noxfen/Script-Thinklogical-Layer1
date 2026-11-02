<script lang="ts">
  import { untrack } from 'svelte';
  import { app } from '../../lib/store.svelte';
  import type { LinkMode, Preset, PresetLink } from '../../lib/types';
  import { LINK_MODES, PRESET_COLORS, uuid } from '../../lib/util';
  import Icon from '../Icon.svelte';
  import PortSelect from '../PortSelect.svelte';

  let {
    preset,
    onsave,
    oncancel,
  }: { preset: Preset | null; onsave: (p: Preset) => void; oncancel: () => void } = $props();

  // Bozza locale: si scrive nella config solo con «Salva preset».
  // La copia dal preset va fatta una volta sola, di proposito: PresetScreen
  // rimonta l'editor con {#key editing}, quindi `preset` non cambia mai mentre
  // il componente è vivo. `untrack` lo dice in modo esplicito.
  const base = untrack(() => preset);
  const isNew = !base;
  let name = $state(base?.name ?? '');
  let color = $state(base?.color ?? PRESET_COLORS[0].value);
  let links = $state<PresetLink[]>(
    base ? base.links.map((l) => ({ ...l })) : [{ a: 1, b: Math.min(2, app.portCount), mode: 'bidir' }],
  );
  const scene = base?.scene ?? false;
  let tried = $state(false);

  const errors = $derived.by(() => {
    const e: string[] = [];
    if (!name.trim()) e.push('Dai un nome al preset.');
    if (!links.length) e.push('Aggiungi almeno un collegamento.');
    links.forEach((l, i) => {
      if (l.a === l.b) e.push(`Collegamento ${i + 1}: porta A e porta B devono essere diverse.`);
      if (l.a > app.portCount || l.b > app.portCount)
        e.push(`Collegamento ${i + 1}: la matrice ha solo ${app.portCount} porte.`);
    });
    return e;
  });

  function add() {
    const last = links[links.length - 1];
    links.push({ a: last?.a ?? 1, b: Math.min((last?.b ?? 1) + 1, app.portCount), mode: last?.mode ?? 'bidir' });
  }
  function remove(i: number) {
    links.splice(i, 1);
  }
  function move(i: number, d: -1 | 1) {
    const j = i + d;
    if (j < 0 || j >= links.length) return;
    [links[i], links[j]] = [links[j], links[i]];
  }
  function save() {
    tried = true;
    if (errors.length) return;
    onsave({
      id: preset?.id ?? uuid(),
      name: name.trim(),
      color,
      links: $state.snapshot(links) as PresetLink[],
      scene,
    });
  }
  function modeHelp(m: LinkMode) {
    return LINK_MODES.find((x) => x.value === m)?.help ?? '';
  }
</script>

<section class="card editor" aria-labelledby="ed-title">
  <header>
    <div>
      <h2 id="ed-title">{isNew ? 'Nuovo preset' : `Modifica «${preset?.name}»`}</h2>
      <p>I preset si salvano per la matrice {app.activeProfile?.name}.</p>
    </div>
    <button class="btn ghost sm icon-only" aria-label="Chiudi editor" onclick={oncancel}><Icon name="x" /></button>
  </header>

  {#if scene}
    <div class="callout info scene-note">
      <Icon name="info" />
      <p>
        Questo preset è una <strong>situazione completa</strong>: quando lo esegui, la matrice torna esattamente
        come nell’elenco qui sotto e tutte le altre uscite vengono spente. Modificando i collegamenti cambi
        quello che verrà ripristinato.
      </p>
    </div>
  {/if}

  <div class="top">
    <label class="field name">
      <span class="label">Nome</span>
      <input class="input" bind:value={name} maxlength="80" placeholder="Es. Codec 1 con Tablet A" />
    </label>
    <fieldset class="field colors">
      <legend class="label">Colore</legend>
      <div class="swatches" role="radiogroup">
        {#each PRESET_COLORS as c (c.value)}
          <button
            type="button"
            class="sw"
            role="radio"
            aria-checked={color === c.value}
            aria-label={c.name}
            title={c.name}
            style="--c:{c.value}"
            onclick={() => (color = c.value)}
          >
            {#if color === c.value}<Icon name="check" size={14} stroke={2.5} />{/if}
          </button>
        {/each}
      </div>
    </fieldset>
  </div>

  <h3 class="sub">Collegamenti <span class="muted">(eseguiti in quest’ordine)</span></h3>
  <ol class="links">
    {#each links as l, i (i)}
      <li class="link">
        <span class="idx num">{i + 1}</span>
        <div class="field">
          <span class="label small">Porta A</span>
          <PortSelect bind:value={l.a} label={`Collegamento ${i + 1}, porta A`} />
        </div>
        <div class="field">
          <span class="label small">Tipo</span>
          <select class="select" bind:value={l.mode} aria-label={`Collegamento ${i + 1}, tipo`}>
            {#each LINK_MODES as m (m.value)}<option value={m.value}>{m.label}</option>{/each}
          </select>
        </div>
        <div class="field">
          <span class="label small">Porta B</span>
          <PortSelect bind:value={l.b} label={`Collegamento ${i + 1}, porta B`} />
        </div>
        <div class="tools">
          <button class="btn ghost sm icon-only" aria-label="Sposta su" disabled={i === 0} onclick={() => move(i, -1)}><Icon name="up" size={16} /></button>
          <button class="btn ghost sm icon-only" aria-label="Sposta giù" disabled={i === links.length - 1} onclick={() => move(i, 1)}><Icon name="down" size={16} /></button>
          <button class="btn ghost sm icon-only" aria-label="Rimuovi collegamento" onclick={() => remove(i)}><Icon name="trash" size={16} /></button>
        </div>
        <p class="help small muted">{modeHelp(l.mode)}</p>
      </li>
    {/each}
  </ol>
  <button class="btn sm" onclick={add}><Icon name="plus" size={16} /> Aggiungi collegamento</button>

  {#if tried && errors.length}
    <div class="callout error errs" role="alert">
      <Icon name="errorCircle" />
      <ul>{#each errors as e}<li>{e}</li>{/each}</ul>
    </div>
  {/if}

  <footer>
    <button class="btn" onclick={oncancel}>Annulla</button>
    <button class="btn primary" onclick={save}><Icon name="save" size={16} /> Salva preset</button>
  </footer>
</section>

<style>
  .editor {
    border-color: var(--ui-border);
    margin-bottom: 20px;
  }
  .scene-note {
    margin-bottom: 14px;
  }
  .top {
    display: grid;
    grid-template-columns: minmax(240px, 1fr) auto;
    gap: 20px;
    align-items: end;
  }
  fieldset {
    border: 0;
    padding: 0;
    margin: 0;
  }
  .swatches {
    display: flex;
    gap: 6px;
  }
  .sw {
    width: 34px;
    height: 34px;
    border-radius: var(--r-sm);
    border: 1px solid var(--ui-border);
    background: var(--c);
    color: #fff;
    display: grid;
    place-items: center;
    cursor: pointer;
    padding: 0;
  }
  .sw[aria-checked='true'] {
    box-shadow: 0 0 0 2px var(--ui-bg), 0 0 0 4px var(--ui-text);
  }
  .sub {
    margin: 20px 0 8px;
    font-size: 14px;
  }
  .sub .muted {
    font-weight: 400;
  }
  .links {
    list-style: none;
    margin: 0 0 10px;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .link {
    display: grid;
    grid-template-columns: 26px minmax(150px, 1fr) minmax(180px, 1fr) minmax(150px, 1fr) auto;
    gap: 6px 10px;
    align-items: end;
    padding: 10px 12px;
    background: var(--ui-surface);
    border-radius: var(--r-md);
  }
  .idx {
    font-weight: 700;
    color: var(--ui-text-muted);
    align-self: center;
    padding-top: 18px;
  }
  .tools {
    display: flex;
    gap: 2px;
  }
  .help {
    grid-column: 2 / -1;
  }
  .errs {
    margin-top: 14px;
  }
  .errs ul {
    margin: 0;
    padding-left: 16px;
  }
  footer {
    display: flex;
    justify-content: flex-end;
    gap: 10px;
    margin-top: 18px;
    padding-top: 14px;
    border-top: 1px solid var(--ui-divider);
  }
</style>
