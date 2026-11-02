<script lang="ts">
  import { api, type DisconnectMode } from '../../lib/api';
  import { app } from '../../lib/store.svelte';
  import type { LinkMode, Preset } from '../../lib/types';
  import { errText, isTypingTarget, LINK_MODES, linkArrow, presetActive } from '../../lib/util';
  import Icon from '../Icon.svelte';
  import PortSelect from '../PortSelect.svelte';
  import PresetEditor from './PresetEditor.svelte';

  let editing = $state<Preset | null | 'new'>(null);
  let manage = $state(false);
  let running = $state<string | null>(null);

  // collegamento rapido
  let qa = $state(1);
  let qb = $state(Math.min(2, app.portCount));
  let qmode = $state<LinkMode>('bidir');
  // scollega porta
  let dport = $state(1);
  let dmode = $state<DisconnectMode>('both');

  const presets = $derived(app.presets);
  const routing = $derived(app.snapshot.routing);
  const can = $derived(app.connected && !app.busy);

  async function run(p: Preset) {
    if (running || manage) return;
    running = p.id;
    await app.executePreset(p);
    running = null;
  }

  function list(): Preset[] {
    const pid = app.activeProfile!.id;
    const c = app.config!;
    if (!c.presets[pid]) c.presets[pid] = [];
    return c.presets[pid];
  }

  async function saveEdited(p: Preset) {
    const l = list();
    const i = l.findIndex((x) => x.id === p.id);
    if (i >= 0) l[i] = p;
    else l.push(p);
    editing = null;
    await app.saveConfig(`Preset «${p.name}» salvato`);
  }

  async function del(p: Preset) {
    const ok = await app.confirm({
      title: 'Eliminare il preset?',
      message: `Il preset «${p.name}» verrà eliminato. Le porte della matrice non cambiano.`,
      confirmLabel: 'Elimina preset',
      danger: true,
    });
    if (!ok) return;
    const l = list();
    const i = l.findIndex((x) => x.id === p.id);
    if (i >= 0) l.splice(i, 1);
    await app.saveConfig(`Preset «${p.name}» eliminato`);
  }

  async function move(p: Preset, d: -1 | 1) {
    const l = list();
    const i = l.findIndex((x) => x.id === p.id);
    const j = i + d;
    if (i < 0 || j < 0 || j >= l.length) return;
    [l[i], l[j]] = [l[j], l[i]];
    await app.saveConfig();
  }

  async function saveCurrent() {
    if (!app.connected) return;
    const d = new Date();
    const def = `Situazione del ${d.toLocaleDateString('it-IT', { day: '2-digit', month: '2-digit' })} ore ${d.toLocaleTimeString('it-IT', { hour: '2-digit', minute: '2-digit' })}`;
    const name = await app.prompt({
      title: 'Salva situazione attuale',
      message:
        'Il preset ricorderà tutti i collegamenti di adesso. Quando lo esegui, la matrice torna esattamente così: le uscite non collegate adesso verranno spente.',
      confirmLabel: 'Salva preset',
      input: { label: 'Nome del preset', value: def },
    });
    if (!name) return;
    try {
      const p = await api.snapshotToPreset(name);
      if (!p.links.length) {
        app.toast('warn', 'Nessun collegamento attivo: il preset non è stato salvato.');
        return;
      }
      list().push({ ...p, scene: p.scene ?? true });
      await app.saveConfig(`Preset «${p.name}» salvato (${p.links.length} collegamenti)`);
    } catch (e) {
      app.toast('error', errText(e));
    }
  }

  function quickLink() {
    if (qa === qb) {
      app.toast('warn', 'Scegli due porte diverse.');
      return;
    }
    void app.act(() => api.link(qa, qb, qmode));
  }

  function disconnectPort() {
    void app.act(() => api.disconnectPort(dport, app.expert ? dmode : 'both'));
  }

  function onKey(e: KeyboardEvent) {
    if (app.dialog || editing || manage) return;
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    if (isTypingTarget(e.target)) return;
    if (/^[1-9]$/.test(e.key)) {
      const p = presets[Number(e.key) - 1];
      if (p) {
        e.preventDefault();
        void run(p);
      }
    } else if (e.key === 'Escape' && manage) {
      manage = false;
    }
  }

  const qhelp = $derived(LINK_MODES.find((m) => m.value === qmode)?.help ?? '');
</script>

<svelte:window onkeydown={onKey} />

<div class="page">
  <div class="page-head">
    <div>
      <h1>Preset</h1>
      <p>Tocca un preset per collegare le porte. Quello attivo in questo momento è evidenziato.</p>
    </div>
    <div class="row">
      <button class="btn" onclick={saveCurrent} disabled={!app.connected} title={app.connected ? '' : 'Connettiti alla matrice per salvare la situazione attuale'}>
        <Icon name="save" size={16} /> Salva situazione attuale come preset
      </button>
      <button class="btn primary" onclick={() => ((editing = 'new'), (manage = false))}>
        <Icon name="plus" size={16} /> Nuovo preset
      </button>
      {#if presets.length}
        <button class="btn" aria-pressed={manage} class:on={manage} onclick={() => (manage = !manage)}>
          <Icon name={manage ? 'check' : 'edit'} size={16} /> {manage ? 'Fine modifiche' : 'Modifica preset'}
        </button>
      {/if}
    </div>
  </div>

  {#if editing}
    {#key editing}
      <PresetEditor preset={editing === 'new' ? null : editing} onsave={saveEdited} oncancel={() => (editing = null)} />
    {/key}
  {/if}

  {#if presets.length === 0}
    <div class="card empty-card">
      <Icon name="presets" size={28} />
      <h2>Nessun preset per {app.activeProfile?.name}</h2>
      <p class="muted">Crea il primo con «Nuovo preset» oppure collega le porte e premi «Salva situazione attuale come preset».</p>
    </div>
  {:else}
    <ul class="grid" aria-label="Preset">
      {#each presets as p, i (p.id)}
        {@const active = presetActive(routing, p)}
        <li class="item" class:manage>
          <button
            class="preset"
            class:active
            class:running={running === p.id}
            style="--pc:{p.color || 'var(--ui-primary)'}"
            disabled={!manage && (!app.connected || (app.busy && running !== p.id))}
            aria-pressed={active}
            aria-describedby={`pd-${p.id}`}
            onclick={() => (manage ? (editing = p) : run(p))}
          >
            <span class="stripe" aria-hidden="true"></span>
            <span class="top">
              {#if i < 9}<kbd title="Scorciatoia da tastiera">{i + 1}</kbd>{/if}
              {#if p.scene}<span class="badge neutral"><Icon name="copy" size={12} stroke={2} />Situazione completa</span>{/if}
              <span class="spacer"></span>
              {#if running === p.id}
                <span class="badge neutral"><Icon name="loader" size={13} spin />In corso…</span>
              {:else if active}
                <span class="badge ok"><Icon name="checkCircle" size={13} stroke={2} />Attivo</span>
              {/if}
            </span>
            <span class="name">{p.name}</span>
            <span class="links" id={`pd-${p.id}`}>
              {#each p.links.slice(0, 3) as l}
                <span class="lk">
                  <span class="pair num">{l.a} {linkArrow(l.mode)} {l.b}</span>
                  <span class="names">{app.name(l.a)} {linkArrow(l.mode)} {app.name(l.b)}</span>
                  {#if l.mode !== 'bidir'}<span class="chip">{l.mode === 'kvm' ? 'KVM' : 'Solo video'}</span>{/if}
                </span>
              {/each}
              {#if p.links.length > 3}<span class="more">e altri {p.links.length - 3} collegamenti</span>{/if}
            </span>
          </button>
          {#if manage}
            <div class="tools">
              <button class="btn sm" onclick={() => (editing = p)}><Icon name="edit" size={15} /> Modifica</button>
              <button class="btn sm icon-only" aria-label="Sposta prima" disabled={i === 0} onclick={() => move(p, -1)}><Icon name="up" size={15} /></button>
              <button class="btn sm icon-only" aria-label="Sposta dopo" disabled={i === presets.length - 1} onclick={() => move(p, 1)}><Icon name="down" size={15} /></button>
              <span class="spacer"></span>
              <button class="btn sm danger-outline" onclick={() => del(p)}><Icon name="trash" size={15} /> Elimina</button>
            </div>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}

  <div class="cards">
    <section class="card" aria-labelledby="ql-title">
      <header>
        <div>
          <h2 id="ql-title">Collegamento rapido</h2>
          <p>Collega due porte senza creare un preset.</p>
        </div>
      </header>
      <div class="qgrid">
        <label class="field"><span class="label">Porta A</span><PortSelect bind:value={qa} /></label>
        <label class="field"><span class="label">Tipo</span>
          <select class="select" bind:value={qmode}>
            {#each LINK_MODES as m (m.value)}<option value={m.value}>{m.label}</option>{/each}
          </select>
        </label>
        <label class="field"><span class="label">Porta B</span><PortSelect bind:value={qb} /></label>
      </div>
      <p class="help small muted">{qhelp}</p>
      <div class="actions">
        <button class="btn primary" disabled={!can} onclick={quickLink}><Icon name="link" size={16} /> Collega</button>
      </div>
    </section>

    <section class="card" aria-labelledby="dp-title">
      <header>
        <div>
          <h2 id="dp-title">Scollega una porta</h2>
          <p>Stacca immagine e tastiera di una sola porta.</p>
        </div>
      </header>
      <div class="dgrid">
        <label class="field"><span class="label">Porta</span><PortSelect bind:value={dport} /></label>
        {#if app.expert}
          <div class="field">
            <span class="label" id="dm-l">Cosa scollegare</span>
            <div class="seg" role="radiogroup" aria-labelledby="dm-l">
              <button role="radio" aria-checked={dmode === 'input'} onclick={() => (dmode = 'input')} title="DI: stacca l’ingresso da tutte le uscite">Ingresso</button>
              <button role="radio" aria-checked={dmode === 'output'} onclick={() => (dmode = 'output')} title="DO: stacca l’uscita">Uscita</button>
              <button role="radio" aria-checked={dmode === 'both'} onclick={() => (dmode = 'both')} title="DI + DO">Entrambi</button>
            </div>
          </div>
        {/if}
      </div>
      <div class="actions">
        <button class="btn" disabled={!can} onclick={disconnectPort}><Icon name="unlink" size={16} /> Scollega porta</button>
      </div>
    </section>
  </div>

  <section class="danger-zone" aria-labelledby="dz-title">
    <div>
      <h2 id="dz-title">Scollega tutto</h2>
      <p class="muted">Stacca tutte le porte della matrice (quelle bloccate restano come sono). Chiede conferma.</p>
    </div>
    <button class="btn danger" disabled={!can} onclick={() => app.disconnectAll()}>
      <Icon name="unlink" size={16} /> Scollega tutto
    </button>
  </section>
</div>

<style>
  .grid {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
    gap: 12px;
  }
  .item {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .item.manage .preset {
    border-bottom-left-radius: 0;
    border-bottom-right-radius: 0;
  }
  .preset {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: 6px;
    min-height: 128px;
    height: 100%;
    padding: 12px 16px 14px 20px;
    border: 1px solid var(--ui-border);
    border-radius: var(--r-md);
    background: var(--ui-raised);
    color: var(--ui-text);
    font: inherit;
    text-align: left;
    cursor: pointer;
    overflow: hidden;
    transition: background var(--t-fast), border-color var(--t-fast), box-shadow var(--t-fast);
  }
  .preset:hover:not(:disabled) {
    border-color: var(--ui-text-muted);
    background: linear-gradient(var(--ui-hover), var(--ui-hover)), var(--ui-raised);
  }
  .preset:active:not(:disabled) {
    background: linear-gradient(var(--ui-active), var(--ui-active)), var(--ui-raised);
  }
  .preset:disabled {
    cursor: not-allowed;
    opacity: 0.6;
  }
  .preset.active {
    border-color: var(--ui-selected-border);
    background: var(--ui-selected-bg);
    box-shadow: inset 0 0 0 1px var(--ui-selected-border);
  }
  .stripe {
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
    width: 6px;
    background: var(--pc);
  }
  /* data-theme sta su <html>, fuori dal componente: senza :global() Svelte
     scopa via la regola e in tema scuro la banda resta senza stacco. */
  :global([data-theme='dark']) .stripe {
    box-shadow: inset -1px 0 0 rgba(255, 255, 255, 0.12);
  }
  .top {
    display: flex;
    align-items: center;
    gap: 6px;
    min-height: 22px;
  }
  .spacer {
    flex: 1;
  }
  .name {
    font-size: 16.5px;
    font-weight: 700;
    line-height: 1.3;
    overflow-wrap: anywhere;
  }
  .links {
    display: flex;
    flex-direction: column;
    gap: 3px;
    margin-top: auto;
  }
  .lk {
    display: flex;
    align-items: baseline;
    gap: 8px;
    min-width: 0;
  }
  .pair {
    font-weight: 700;
    font-size: 15px;
    white-space: nowrap;
    min-width: 62px;
  }
  .names {
    font-size: 12.5px;
    color: var(--ui-text-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
    flex: 1;
  }
  .more {
    font-size: 12.5px;
    color: var(--ui-text-muted);
  }
  .tools {
    display: flex;
    gap: 6px;
    align-items: center;
    padding: 8px;
    border: 1px solid var(--ui-border);
    border-top: 0;
    border-radius: 0 0 var(--r-md) var(--r-md);
    background: var(--ui-surface);
  }
  .btn.on {
    background: var(--ui-selected-bg);
    color: var(--ui-selected-text);
    border-color: var(--ui-selected-border);
  }
  .empty-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 36px;
    text-align: center;
    color: var(--ui-text);
  }
  .empty-card :global(svg) {
    color: var(--ui-text-muted);
  }
  .cards {
    display: grid;
    grid-template-columns: minmax(0, 1.5fr) minmax(0, 1fr);
    gap: 16px;
    margin-top: 28px;
  }
  .qgrid {
    display: grid;
    grid-template-columns: 1fr 1fr 1fr;
    gap: 12px;
  }
  .dgrid {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
  }
  .dgrid > .field:first-child {
    flex: 1 1 180px;
  }
  .help {
    margin-top: 8px;
    min-height: 2.6em;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    margin-top: 12px;
  }
  .danger-zone {
    margin-top: 16px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 14px 20px;
    border: 1px solid var(--ui-divider);
    border-left: 4px solid var(--tt-error);
    border-radius: var(--r-md);
  }
  .danger-zone h2 {
    font-size: 15px;
  }
  .danger-zone p {
    font-size: 13px;
    margin-top: 2px;
  }
  @media (max-width: 1100px) {
    .cards {
      grid-template-columns: 1fr;
    }
  }
</style>
