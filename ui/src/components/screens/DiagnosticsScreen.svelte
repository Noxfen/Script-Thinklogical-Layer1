<script lang="ts">
  // Schermata «Diagnostica» (solo modalità tecnico).
  // Nessuna prop: tutto lo stato arriva dallo store `app`.
  //
  // Stato disponibile:
  //   app.diagnostics      → Diagnostics | null (ultima lettura; evento `tlx://diag`)
  //   app.losSupported     → boolean | null (false = firmware senza LOS/potenze, Rev. M assente)
  //   app.diagBusy         → lettura in corso
  //   app.diagError        → errore dell'ultima lettura
  //   app.portCount, app.labels, app.name(porta), app.connected, app.now
  //   app.snapshot.info    → MatrixInfo (versione, xtype, schede installate, opzioni)
  // Azioni:
  //   app.readDiagnostics() → api.readDiagnostics() (XALARM, XLOS9999, XRXPWR9999, XTXPWR9999)
  // Formati: fmtDbm / fmtUw / uwToDbm per le potenze, hex per la bitmap, fmtAge per l'età del dato.
  import { app } from '../../lib/store.svelte';
  import { fmtAge, fmtDbm, fmtUw, hex, uwToDbm } from '../../lib/util';
  import Icon from '../Icon.svelte';

  const diag = $derived(app.diagnostics);
  const noLos = $derived(app.losSupported === false);

  const info = $derived(app.snapshot.info);
  /** true = la diagnostica ottica non è stata ancora provata su questa matrice */
  const losUnknown = $derived(app.losSupported === null);
  /** colonne ottiche visibili: nascoste sui firmware senza i comandi Rev. M */
  const showOptics = $derived(!noLos);
  const ports = $derived(Array.from({ length: app.portCount }, (_, i) => i + 1));

  /** Opzioni di XOPTIONS con una spiegazione in italiano semplice. */
  const optionRows = $derived(
    info
      ? [
          {
            key: 'upperOnly',
            label: 'Solo maiuscole',
            on: info.upperOnly,
            help: 'La matrice accetta i comandi solo scritti in MAIUSCOLO.',
          },
          {
            key: 'realCardCount',
            label: 'Conteggio schede reale',
            on: info.realCardCount,
            help: 'Il numero di schede dichiarato è quello vero; se spento può essere la metà (matrice VX o API V4).',
          },
          {
            key: 'p2p',
            label: 'Modalità punto-punto (P2P)',
            on: info.p2p,
            help: 'La matrice sa lavorare in punto-punto, cioè un ingresso collegato a una sola uscita.',
          },
          {
            key: 'mcastStatus',
            label: 'Stato in multicast',
            on: info.mcastStatus,
            help: 'La matrice può annunciare da sola i cambi di collegamento via rete (UDP), senza rileggerli.',
          },
          {
            key: 'xctime',
            label: 'Tempi di collegamento (XCTIME)',
            on: info.xctime,
            help: 'La matrice sa dire da quanto tempo una porta è collegata.',
          },
        ]
      : [],
  );

  /** Porte con LOS attivo: nessuna luce in ingresso, quindi un guasto da controllare. */
  const losPorts = $derived(diag ? ports.filter((p) => diag.los[p - 1] === true) : []);
  /** Soglia pratica: sotto −20 dBm il collegamento ottico è debole. */
  const WEAK_DBM = -20;
  function weak(uw: number | null | undefined): boolean {
    const d = uwToDbm(uw);
    return d != null && d < WEAK_DBM;
  }
</script>

<div class="page wide">
  <div class="page-head">
    <div>
      <h1>Diagnostica</h1>
      <p>
        Allarmi, perdita di segnale (LOS) e potenze ottiche delle porte di
        {app.activeProfile?.name ?? '—'}. Serve al tecnico per capire se un cavo o un ricetrasmettitore
        è guasto.
      </p>
    </div>
    <div class="row">
      {#if diag}
        <span class="badge neutral">
          <Icon name="info" size={13} stroke={2} />Dato aggiornato {fmtAge(app.now - diag.updatedAt)}
        </span>
      {/if}
      <button
        class="btn"
        disabled={!app.connected || app.diagBusy}
        onclick={() => app.readDiagnostics()}
      >
        <Icon name={app.diagBusy ? 'loader' : 'refresh'} size={16} spin={app.diagBusy} />
        {app.diagBusy ? 'Lettura in corso…' : 'Aggiorna'}
      </button>
    </div>
  </div>

  {#if !app.connected}
    <div class="callout warn">
      <Icon name="plugOff" />
      <div>
        <strong>Matrice non connessa.</strong> La diagnostica si legge direttamente dalla matrice:
        collegati dalla barra in alto (o da «Impostazioni») e poi premi «Aggiorna». Finché non c'è
        connessione, qui sotto resta l'ultima lettura, se c'è stata.
      </div>
    </div>
  {/if}

  {#if app.diagError}
    <div class="callout error">
      <Icon name="alert" />
      <div>
        <strong>Lettura non riuscita.</strong> {app.diagError}
      </div>
    </div>
  {/if}

  {#if noLos}
    <div class="callout info">
      <Icon name="info" />
      <div>Questa matrice non risponde ai comandi di LOS e potenze ottiche: resta solo l'allarme.</div>
    </div>
  {:else if losUnknown}
    <div class="callout info">
      <Icon name="info" />
      <div>
        Diagnostica ottica non ancora provata su questa matrice: premi «Aggiorna» per sapere se il
        firmware risponde a LOS e potenze.
      </div>
    </div>
  {/if}

  <!-- ------------------------------------------------ informazioni della matrice -->
  <h2 class="section-title">Informazioni della matrice</h2>
  <div class="card">
    {#if info}
      <dl class="specs">
        <div>
          <dt>Modello</dt>
          <dd>{info.xtype || '—'}</dd>
        </div>
        <div>
          <dt>Versione API</dt>
          <dd>{info.version || '—'}</dd>
        </div>
        <div>
          <dt>Porte</dt>
          <dd class="num">{info.maxPorts}</dd>
        </div>
        <div>
          <dt>Schede (slot)</dt>
          <dd class="num">{info.maxCards || '—'}</dd>
        </div>
      </dl>

      <div class="slots">
        <div class="slots-head">
          <strong>Schede presenti</strong>
          <span class="small muted">
            Un pallino per slot, da sinistra lo slot della porta 1. Pieno = scheda installata.
          </span>
        </div>
        {#if info.installed.length}
          <div class="dots">
            {#each info.installed as present, i (i)}
              <span
                class="dot"
                class:on={present}
                title={`Slot ${i + 1}: ${present ? 'scheda installata' : 'slot vuoto'}`}
                aria-label={`Slot ${i + 1}: ${present ? 'scheda installata' : 'slot vuoto'}`}
              ></span>
            {/each}
          </div>
          <p class="small muted">
            <strong class="num">{info.installed.filter((x) => x).length}</strong> schede installate su
            <strong class="num">{info.installed.length}</strong> slot.
          </p>
        {:else}
          <p class="muted">La matrice non ha comunicato quali slot sono occupati.</p>
        {/if}
      </div>

      <div class="opts">
        <div class="slots-head">
          <strong>Opzioni del firmware</strong>
          <span class="small muted">Bitmap <span class="mono">XOPTIONS</span> = {hex(info.options)}</span>
        </div>
        <ul class="optlist">
          {#each optionRows as o (o.key)}
            <li>
              <span class="badge {o.on ? 'ok' : 'neutral'}">
                <Icon name={o.on ? 'checkCircle' : 'x'} size={13} stroke={2} />{o.on ? 'Sì' : 'No'}
              </span>
              <span class="optname">{o.label}</span>
              <span class="small muted">{o.help}</span>
            </li>
          {/each}
        </ul>
      </div>
    {:else}
      <p class="muted">
        Nessuna informazione sulla matrice: arriva alla connessione, insieme a modello, versione e
        schede.
      </p>
    {/if}
  </div>

  <!-- ------------------------------------------------ allarmi -->
  <h2 class="section-title">Allarmi</h2>
  <div class="card">
    {#if !diag}
      <p class="muted">Nessuna lettura: premi «Aggiorna» per chiedere l'allarme alla matrice.</p>
    {:else if diag.alarm === null}
      <p>
        <span class="badge neutral"><Icon name="info" size={13} stroke={2} />n.d.</span>
        <span class="muted">La matrice non ha risposto al comando di allarme.</span>
      </p>
    {:else if diag.alarm === 0}
      <p>
        <span class="badge ok"><Icon name="checkCircle" size={13} stroke={2} />Nessun allarme</span>
        <span class="muted">La matrice non segnala guasti (ventole, alimentazioni, schede).</span>
      </p>
    {:else}
      <p class="alarm-on">
        <span class="badge error"><Icon name="alert" size={13} stroke={2} />Allarme attivo</span>
        <span class="mono big">{hex(diag.alarm)}</span>
        <span class="muted">(valore {diag.alarm})</span>
      </p>
      <p class="small muted">
        La matrice segnala un guasto. Il codice è una bitmap del firmware: annotalo e controlla
        ventole, alimentazioni e LED delle schede sul fronte della matrice.
      </p>
    {/if}
  </div>

  <!-- ------------------------------------------------ tabella per porta -->
  <h2 class="section-title">Porte</h2>
  {#if !diag}
    <div class="card empty">
      <p>Nessuna diagnostica letta finora.</p>
      <p class="small">
        Premi «Aggiorna» per leggere allarme, perdita di segnale e potenze ottiche di tutte le
        {app.portCount} porte.
      </p>
      <button class="btn primary" disabled={!app.connected || app.diagBusy} onclick={() => app.readDiagnostics()}>
        <Icon name={app.diagBusy ? 'loader' : 'refresh'} size={16} spin={app.diagBusy} />
        {app.diagBusy ? 'Lettura in corso…' : 'Aggiorna'}
      </button>
    </div>
  {:else}
    {#if showOptics}
      <p class="small muted legend">
        LOS acceso = nessuna luce in ingresso: controlla fibra e ricetrasmettitore. Le potenze sono in
        µW, fra parentesi gli stessi valori in dBm (più vicino a 0 = segnale più forte). «n.d.» = la
        matrice non ha dato il valore; non è uno zero.
        {#if losPorts.length}
          <strong>Porte senza luce: {losPorts.join(', ')}.</strong>
        {/if}
      </p>
    {/if}
    <div class="card flush tablewrap">
      <table class="table">
        <thead>
          <tr>
            <th class="num">Porta</th>
            <th>Etichetta</th>
            {#if showOptics}
              <th>Segnale (LOS)</th>
              <th class="num">Potenza ricevuta</th>
              <th class="num">Potenza trasmessa</th>
            {/if}
          </tr>
        </thead>
        <tbody>
          {#each ports as p (p)}
            {@const k = p - 1}
            {@const los = diag.los[k] ?? null}
            {@const rx = diag.rxUw[k] ?? null}
            {@const tx = diag.txUw[k] ?? null}
            <tr class:bad={los === true}>
              <td class="num"><strong>{p}</strong></td>
              <td>{app.name(p)}</td>
              {#if showOptics}
                <td>
                  {#if los === true}
                    <span class="badge error"
                      ><Icon name="errorCircle" size={13} stroke={2} />Nessuna luce</span
                    >
                  {:else if los === false}
                    <span class="badge ok"><Icon name="checkCircle" size={13} stroke={2} />OK</span>
                  {:else}
                    <span class="muted">n.d.</span>
                  {/if}
                </td>
                <td class="num pwr" class:weak={weak(rx)}>
                  {#if rx == null}
                    <span class="muted">n.d.</span>
                  {:else}
                    {fmtUw(rx)} <span class="muted">({fmtDbm(rx)})</span>
                  {/if}
                </td>
                <td class="num pwr" class:weak={weak(tx)}>
                  {#if tx == null}
                    <span class="muted">n.d.</span>
                  {:else}
                    {fmtUw(tx)} <span class="muted">({fmtDbm(tx)})</span>
                  {/if}
                </td>
              {/if}
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</div>

<style>
  .wide {
    max-width: none;
  }
  .callout {
    margin-bottom: 12px;
  }
  .section-title {
    margin-top: 22px;
  }

  /* informazioni della matrice */
  .specs {
    display: flex;
    flex-wrap: wrap;
    gap: 10px 36px;
    margin: 0;
  }
  .specs dt {
    font-size: 12.5px;
    font-weight: 700;
    color: var(--ui-text-muted);
  }
  .specs dd {
    margin: 2px 0 0;
    font-size: 15px;
    font-weight: 700;
  }
  .slots,
  .opts {
    margin-top: 18px;
    padding-top: 14px;
    border-top: 1px solid var(--ui-divider);
  }
  .slots-head {
    display: flex;
    align-items: baseline;
    gap: 10px;
    flex-wrap: wrap;
    margin-bottom: 8px;
  }
  .dots {
    display: flex;
    flex-wrap: wrap;
    gap: 5px;
    margin-bottom: 6px;
  }
  .dot {
    width: 13px;
    height: 13px;
    border-radius: 50%;
    border: 1px solid var(--ui-border);
    background: var(--ui-surface);
    flex: none;
  }
  .dot.on {
    background: var(--ui-primary);
    border-color: var(--ui-primary);
  }
  .optlist {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 7px;
  }
  .optlist li {
    display: grid;
    grid-template-columns: 62px minmax(180px, 220px) 1fr;
    align-items: baseline;
    gap: 10px;
  }
  .optname {
    font-weight: 700;
  }

  /* allarmi */
  .alarm-on {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }
  .big {
    font-size: 16px;
    font-weight: 700;
  }

  /* tabella */
  .legend {
    margin-bottom: 10px;
    max-width: 96ch;
  }
  .tablewrap {
    max-height: min(62vh, 620px);
    overflow: auto;
  }
  .pwr {
    white-space: nowrap;
  }
  .pwr.weak {
    color: var(--ui-warning-fg);
  }
  tr.bad td {
    background: var(--ui-danger-bg);
  }
  .empty p {
    margin-bottom: 6px;
  }
  .empty .btn {
    margin-top: 8px;
  }

  @media (max-width: 900px) {
    .optlist li {
      grid-template-columns: 62px 1fr;
    }
    .optlist li .small {
      grid-column: 2;
    }
  }
</style>
