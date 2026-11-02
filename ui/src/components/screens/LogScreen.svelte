<script lang="ts">
  // Schermata «Log» / registro eventi (solo modalità tecnico).
  // Nessuna prop: tutto lo stato arriva dallo store `app`.
  //
  // Stato disponibile:
  //   app.visibleLog   → LogEntry[] già filtrate (e ferme se in pausa)
  //   app.log          → LogEntry[] complete in memoria (max 2000, evento `tlx://log`)
  //   app.logQuery     → filtro testuale (bindabile)
  //   app.logLevels    → Set<LogLevel> selezionati (vuoto = tutti)
  //   app.logChannels  → Set<LogChannel> selezionati (vuoto = tutti)
  //   app.logPaused    → registro fermo
  //   app.logFiltered  → true se almeno un filtro è attivo
  //   LOG_LEVELS / LOG_CHANNELS → etichette italiane per i pulsanti dei filtri
  // Azioni:
  //   app.toggleLogLevel(l), app.toggleLogChannel(c), app.resetLogFilters()
  //   app.setLogPaused(v) / app.toggleLogPause()
  //   app.clearLog()  → api.clearLog() e svuota la memoria
  //   app.exportLog() → api.exportLog() (CSV, dialog del sistema)
  // Formati: fmtTime(ts, true) per l'ora con i millisecondi, latencyMs per la latenza.
  import { app, LOG_CHANNELS, LOG_LEVELS } from '../../lib/store.svelte';
  import type { LogChannel, LogEntry, LogLevel } from '../../lib/types';
  import { fmtTime } from '../../lib/util';
  import Icon from '../Icon.svelte';

  /** Righe disegnate al massimo in una volta: con 2000 voci il DOM resta leggero. */
  const PAGE = 500;

  /** Etichette brevi al singolare per le righe (quelle di LOG_LEVELS sono al plurale, per i filtri). */
  const LEVEL_SHORT: Record<LogLevel, string> = {
    info: 'Info',
    warn: 'Avviso',
    error: 'Errore',
    tx: 'Inviato',
    rx: 'Ricevuto',
  };
  const CHANNEL_LABEL = new Map<LogChannel, string>(LOG_CHANNELS.map((c) => [c.value, c.label]));

  let listEl = $state<HTMLDivElement | null>(null);
  /** true = l'elenco è in coda: le righe nuove lo fanno scorrere da solo */
  let tail = $state(true);
  /** quante righe c'erano in memoria quando l'utente ha messo in pausa */
  let pausedAt = $state(0);
  /** tetto delle righe disegnate, alzabile a mano */
  let limit = $state(PAGE);
  let exporting = $state(false);

  const visible = $derived(app.visibleLog);
  /** solo la coda: le righe più vecchie si mostrano su richiesta */
  const rows = $derived<LogEntry[]>(visible.length > limit ? visible.slice(visible.length - limit) : visible);
  const olderHidden = $derived(visible.length - rows.length);
  const hiddenByFilters = $derived(Math.max(0, app.log.length - visible.length));
  /** righe arrivate dopo la pausa: l'elenco fermo non le mostra */
  const newWhilePaused = $derived(app.logPaused ? Math.max(0, app.log.length - pausedAt) : 0);
  const isRaw = (l: LogLevel) => l === 'tx' || l === 'rx';

  // Scorrimento automatico: finché l'utente resta in fondo (`tail`) e il registro non è in pausa,
  // ogni riga nuova riporta l'elenco in coda. In pausa l'elenco non si muove.
  $effect(() => {
    const last = rows.length ? rows[rows.length - 1].id : 0;
    const n = rows.length;
    if (app.logPaused || !tail || !listEl) return;
    void last;
    void n;
    listEl.scrollTop = listEl.scrollHeight;
  });

  function onScroll() {
    const el = listEl;
    if (!el) return;
    tail = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
  }

  function toBottom() {
    tail = true;
    if (listEl) listEl.scrollTop = listEl.scrollHeight;
  }

  function togglePause() {
    if (!app.logPaused) pausedAt = app.log.length;
    app.toggleLogPause();
    if (app.logPaused === false) toBottom();
  }

  function resume() {
    app.setLogPaused(false);
    toBottom();
  }

  function filterSummary(): string {
    const p: string[] = [];
    if (app.logLevels.size) p.push(`livelli: ${LOG_LEVELS.filter((l) => app.logLevels.has(l.value)).map((l) => l.label).join(', ')}`);
    if (app.logChannels.size) p.push(`canali: ${LOG_CHANNELS.filter((c) => app.logChannels.has(c.value)).map((c) => c.label).join(', ')}`);
    const q = app.logQuery.trim();
    if (q) p.push(`ricerca: «${q}»`);
    return p.join(' — ');
  }

  /** Testo per gli appunti: una riga per voce, colonne allineate, leggibile al telefono. */
  function asText(): string {
    const f = filterSummary();
    const head = [
      `Registro TLX — ${app.activeProfile?.name ?? 'nessuna matrice'} — ${new Date().toLocaleString('it-IT')}`,
      `${visible.length} righe di ${app.log.length}${f ? ` (filtri — ${f})` : ''}${app.logPaused ? ' — registro in pausa' : ''}`,
      '',
    ];
    const body = visible.map((e) => {
      const lv = LEVEL_SHORT[e.level].padEnd(8);
      const ch = (CHANNEL_LABEL.get(e.channel) ?? e.channel).padEnd(12);
      const lat = e.latencyMs != null ? `  (${e.latencyMs} ms)` : '';
      return `${fmtTime(e.ts, true)}  ${lv}${ch}${e.message}${lat}`;
    });
    return [...head, ...body].join('\n');
  }

  /** Copia negli appunti; se il webview li nega, ripiega su textarea + execCommand. */
  async function copyVisible() {
    if (!visible.length) return;
    const text = asText();
    const done = `${visible.length} righe copiate negli appunti`;
    try {
      await navigator.clipboard.writeText(text);
      app.toast('ok', done);
      return;
    } catch {
      /* appunti non disponibili: ripiego sotto */
    }
    try {
      const ta = document.createElement('textarea');
      ta.value = text;
      ta.setAttribute('readonly', '');
      ta.style.position = 'fixed';
      ta.style.opacity = '0';
      document.body.appendChild(ta);
      ta.select();
      const ok = document.execCommand('copy');
      document.body.removeChild(ta);
      if (ok) {
        app.toast('ok', done);
        return;
      }
    } catch {
      /* niente da fare */
    }
    app.toast('warn', 'Copia negli appunti non riuscita', ['Usa «Esporta CSV» per salvare il registro su file.']);
  }

  async function doExport() {
    if (exporting) return;
    exporting = true;
    try {
      // lo store mostra già il toast con il percorso scelto o con l'errore
      await app.exportLog();
    } finally {
      exporting = false;
    }
  }

  async function askClear() {
    const n = app.log.length;
    const ok = await app.confirm({
      title: 'Svuotare il registro?',
      message:
        `Le ${n} righe in memoria verranno cancellate: è la prova di cosa è stato inviato alla matrice e di cosa ha risposto. ` +
        'Se servono all’assistenza, salvale prima con «Esporta CSV» o copiale negli appunti.',
      confirmLabel: 'Svuota registro',
      danger: true,
    });
    if (!ok) return;
    await app.clearLog();
    pausedAt = 0;
    limit = PAGE;
    tail = true;
  }
</script>

<div class="page wide logpage">
  <div class="page-head">
    <div>
      <h1>Registro eventi</h1>
      <p>
        Tutto quello che l'applicazione ha inviato e ricevuto: utile per capire cosa è andato storto.
        Restano le ultime 2000 righe.
      </p>
    </div>
    <div class="row">
      <span class="badge neutral">
        <Icon name="list" size={13} stroke={2} />{app.visibleLog.length} di {app.log.length} righe
      </span>
      <button class="btn" onclick={togglePause} aria-pressed={app.logPaused}>
        <Icon name={app.logPaused ? 'play' : 'pause'} size={16} />
        {app.logPaused ? 'Riprendi' : 'Pausa'}
      </button>
      <button class="btn" disabled={app.visibleLog.length === 0} onclick={copyVisible}>
        <Icon name="copy" size={16} /> Copia righe
      </button>
      <button class="btn" disabled={app.log.length === 0 || exporting} onclick={doExport}>
        <Icon name="download" size={16} /> Esporta CSV
      </button>
      <button class="btn danger-outline" disabled={app.log.length === 0} onclick={askClear}>
        <Icon name="trash" size={16} /> Svuota
      </button>
    </div>
  </div>

  <!-- ------------------------------------------------------------ filtri -->
  <div class="filters">
    <div class="row">
      <div class="search">
        <Icon name="search" size={16} />
        <input
          class="input"
          type="search"
          placeholder="Cerca nel messaggio"
          bind:value={app.logQuery}
          aria-label="Cerca nel registro"
        />
      </div>

      <div class="group">
        <span class="small muted" id="lbl-livelli">Livelli</span>
        <div class="seg" role="group" aria-labelledby="lbl-livelli">
          {#each LOG_LEVELS as l (l.value)}
            <button
              type="button"
              aria-pressed={app.logLevels.has(l.value)}
              onclick={() => app.toggleLogLevel(l.value)}
            >
              {l.label}
            </button>
          {/each}
        </div>
      </div>

      <div class="group">
        <span class="small muted" id="lbl-canali">Canali</span>
        <div class="seg" role="group" aria-labelledby="lbl-canali">
          {#each LOG_CHANNELS as c (c.value)}
            <button
              type="button"
              aria-pressed={app.logChannels.has(c.value)}
              onclick={() => app.toggleLogChannel(c.value)}
            >
              {c.label}
            </button>
          {/each}
        </div>
      </div>

      {#if app.logFiltered}
        <button class="btn sm" onclick={() => app.resetLogFilters()}>
          <Icon name="x" size={15} /> Azzera filtri
        </button>
      {/if}
    </div>

    <p class="small muted counts">
      {#if app.logFiltered}
        Visibili <strong class="num">{visible.length}</strong> righe di <strong class="num">{app.log.length}</strong>:
        i filtri ne nascondono <strong class="num">{hiddenByFilters}</strong>.
      {:else}
        Visibili tutte le <strong class="num">{app.log.length}</strong> righe in memoria: nessun filtro attivo.
      {/if}
      {#if app.isMock}
        In questa modalità dimostrativa il salvataggio su file può non essere disponibile: usa «Copia righe».
      {/if}
    </p>
  </div>

  {#if app.logPaused}
    <div class="callout warn paused" role="status">
      <Icon name="pause" size={16} />
      <div class="grow">
        <strong>Registro in pausa: l'elenco è fermo.</strong>
        {#if newWhilePaused > 0}
          {newWhilePaused}
          {newWhilePaused === 1 ? 'riga nuova è arrivata' : 'righe nuove sono arrivate'} dopo la pausa e non
          {newWhilePaused === 1 ? 'è' : 'sono'} mostrate.
        {:else}
          Le righe nuove continuano ad arrivare, ma non vengono mostrate.
        {/if}
      </div>
      <button class="btn sm" onclick={resume}><Icon name="play" size={15} /> Riprendi</button>
    </div>
  {/if}

  <!-- ------------------------------------------------------------ elenco -->
  <div class="card flush logwrap" class:frozen={app.logPaused}>
    {#if app.log.length === 0}
      <div class="empty">
        <p><strong>Nessun evento registrato.</strong></p>
        <p>Le operazioni compariranno qui appena l'applicazione parla con la matrice.</p>
      </div>
    {:else if visible.length === 0 && app.logFiltered}
      <div class="empty">
        <p><strong>Nessuna riga corrisponde ai filtri.</strong></p>
        <p>Il registro contiene {app.log.length} righe, ma i filtri le nascondono tutte.</p>
        <p class="cta">
          <button class="btn sm" onclick={() => app.resetLogFilters()}>
            <Icon name="x" size={15} /> Azzera filtri
          </button>
        </p>
      </div>
    {:else if visible.length === 0}
      <div class="empty">
        <p><strong>Registro in pausa.</strong></p>
        <p>Le righe arrivate dopo la pausa non vengono mostrate. Riprendi per vederle.</p>
        <p class="cta">
          <button class="btn sm" onclick={resume}><Icon name="play" size={15} /> Riprendi</button>
        </p>
      </div>
    {:else}
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <div
        class="scroller"
        bind:this={listEl}
        onscroll={onScroll}
        role="log"
        aria-label="Voci del registro"
        tabindex="0"
      >
        <div class="lrow lhead">
          <span>Ora</span>
          <span>Livello</span>
          <span>Canale</span>
          <span>Messaggio</span>
          <span class="rt">Latenza</span>
        </div>

        {#if olderHidden > 0}
          <div class="older">
            <span class="small muted">
              Mostrate le ultime <strong class="num">{rows.length}</strong> righe:
              <strong class="num">{olderHidden}</strong> più vecchie non sono disegnate.
            </span>
            <button class="btn sm" onclick={() => (limit += PAGE)}>
              <Icon name="up" size={15} /> Mostra altre {Math.min(PAGE, olderHidden)}
            </button>
          </div>
        {/if}

        {#each rows as e (e.id)}
          <div class="lrow entry lv-{e.level}">
            <span class="num time">{fmtTime(e.ts, true)}</span>
            <span class="lv">
              {#if e.level === 'error'}<Icon name="errorCircle" size={13} stroke={2} />{/if}
              {#if e.level === 'tx'}→{/if}{#if e.level === 'rx'}←{/if}
              {LEVEL_SHORT[e.level]}
            </span>
            <span class="ch small">{CHANNEL_LABEL.get(e.channel) ?? e.channel}</span>
            <span class="msg" class:raw={isRaw(e.level)}>{e.message}</span>
            <span class="num lat">{e.latencyMs != null ? `${e.latencyMs} ms` : ''}</span>
          </div>
        {/each}
      </div>

      {#if !tail && !app.logPaused}
        <button class="btn sm tobottom" onclick={toBottom}>
          <Icon name="down" size={15} /> Vai in fondo
        </button>
      {/if}
    {/if}
  </div>
</div>

<style>
  .wide {
    max-width: none;
  }
  /* la schermata occupa l'altezza disponibile: a scorrere è solo l'elenco */
  .logpage {
    height: 100%;
    min-height: 460px;
    display: flex;
    flex-direction: column;
    gap: 0;
  }
  .page-head {
    flex: none;
  }

  .filters {
    flex: none;
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-bottom: 10px;
  }
  .search {
    position: relative;
    width: 240px;
    flex: none;
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
  .group {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .counts {
    min-height: 18px;
  }

  .paused {
    flex: none;
    margin-bottom: 10px;
  }

  .logwrap {
    position: relative;
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .logwrap.frozen {
    border-color: var(--ui-warning-fg);
  }
  .scroller {
    flex: 1 1 auto;
    min-height: 0;
    overflow: auto;
    overscroll-behavior: contain;
  }

  .lrow {
    display: grid;
    grid-template-columns: 96px 104px 92px minmax(0, 1fr) 76px;
    gap: 10px;
    align-items: baseline;
    padding: 3px 12px;
  }
  .lhead {
    position: sticky;
    top: 0;
    z-index: 1;
    padding: 7px 12px;
    background: var(--ui-raised);
    border-bottom: 1px solid var(--ui-border);
    font-size: 12.5px;
    font-weight: 700;
    color: var(--ui-text-muted);
  }
  .rt {
    text-align: right;
  }

  .entry {
    border-bottom: 1px solid var(--ui-divider);
    border-left: 3px solid transparent;
    padding-left: 9px;
    font-size: 13.5px;
  }
  .entry:hover {
    background: var(--ui-hover);
  }
  .time {
    color: var(--ui-text-muted);
    font-family: var(--mono);
    font-size: 12.5px;
    white-space: nowrap;
  }
  .lv {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 12px;
    font-weight: 700;
    white-space: nowrap;
  }
  .ch {
    color: var(--ui-text-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .msg {
    min-width: 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  /* tx/rx = righe grezze del protocollo */
  .msg.raw {
    font-family: var(--mono);
    font-size: 12.5px;
  }
  .lat {
    color: var(--ui-text-muted);
    font-size: 12.5px;
    text-align: right;
    white-space: nowrap;
  }

  /* livelli distinti a vista: errore in evidenza, avviso meno, tx/rx discreti */
  .lv-error {
    background: var(--ui-danger-bg);
    border-left-color: var(--ui-danger-fg);
  }
  .lv-error .lv {
    color: var(--ui-danger-fg);
  }
  .lv-error .msg {
    font-weight: 700;
  }
  .lv-warn {
    background: var(--ui-warning-bg);
    border-left-color: var(--ui-warning-fg);
  }
  .lv-warn .lv {
    color: var(--ui-warning-fg);
  }
  .lv-info .lv {
    color: var(--ui-text-muted);
  }
  .lv-tx,
  .lv-rx {
    color: var(--ui-text-muted);
  }
  .lv-tx .lv,
  .lv-rx .lv {
    color: var(--ui-text-muted);
    font-weight: 400;
    font-family: var(--mono);
  }
  .lv-tx {
    border-left-color: var(--ui-border);
  }
  .lv-rx {
    border-left-color: var(--ui-divider);
  }

  .older {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 6px 12px;
    background: var(--ui-surface);
    border-bottom: 1px solid var(--ui-divider);
  }

  .tobottom {
    position: absolute;
    right: 18px;
    bottom: 14px;
    box-shadow: var(--ui-shadow);
    background: var(--ui-raised);
  }

  .empty .cta {
    margin-top: 12px;
  }
</style>
