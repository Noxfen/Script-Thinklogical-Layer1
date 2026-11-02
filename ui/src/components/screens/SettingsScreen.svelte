<script lang="ts">
  // Schermata «Impostazioni» (sempre visibile, anche in modalità semplice).
  // Nessuna prop: tutto lo stato arriva dallo store `app`.
  //
  // Stato disponibile:
  //   app.config        → AppConfig completa (profiles, labels, presets, settings)
  //   app.settings      → Settings (timeout, heartbeatS, pollXgetS, delayMs, bidirStrategy,
  //                       confirmDangerous, expertMode, theme; verifyAfterSwitch è ignorato dal backend)
  //   app.activeProfile → MatrixProfile attivo, app.isMock, app.connected
  // Azioni:
  //   app.saveConfig('messaggio')  → api.saveConfig (da chiamare dopo ogni modifica)
  //   app.switchProfile(id)        → cambia matrice (disconnette, salva, riconnette)
  //   api.listSerialPorts()        → string[] per il profilo seriale
  //   api.exportConfig() / api.importConfig()        → salva/carica la configurazione
  //   api.importLegacyPresets(profileId)             → importa `tlx_presets.json` del vecchio script
  //   app.confirm({...}) / app.prompt({...})         → conferme e richieste di testo
  // Qui sta anche l'interruttore «Modalità tecnico» (settings.expertMode), che mostra o
  // nasconde Matrice, Porte, Diagnostica, Console e Registro eventi.
  //
  // Salvataggio: automatico. Ogni modifica finisce nella configurazione e viene scritta su
  // disco dopo una breve pausa (SAVE_DELAY); l'esito è sempre scritto in alto («Salvato
  // alle …»). Lasciando la schermata le modifiche in attesa vengono scritte subito, così
  // nessuna modifica si perde cambiando sezione.
  import { api } from '../../lib/api';
  import { app } from '../../lib/store.svelte';
  import type {
    AppConfig,
    FlowControl,
    MatrixProfile,
    Settings,
    TransportKind,
  } from '../../lib/types';
  import { errText, fmtTime, uuid } from '../../lib/util';
  import Icon from '../Icon.svelte';

  const s = $derived(app.settings);
  const profiles = $derived<MatrixProfile[]>(app.config?.profiles ?? []);

  async function setExpert(v: boolean) {
    if (!app.config) return;
    app.config.settings.expertMode = v;
    await app.saveConfig(v ? 'Modalità tecnico attiva' : 'Modalità tecnico disattivata');
  }

  // ---------------------------------------------------------------- valori di fabbrica
  const FACTORY: Settings = {
    timeoutConnectMs: 3000,
    timeoutCmdMs: 2000,
    heartbeatS: 10,
    pollXgetS: 60,
    verifyAfterSwitch: true,
    delayMs: 300,
    bidirStrategy: 'sequence',
    confirmDangerous: true,
    expertMode: false,
    theme: 'system',
  };

  const NEW_PROFILE: Omit<MatrixProfile, 'id' | 'name'> = {
    transport: 'tcp',
    host: '192.168.13.15',
    port: 17567,
    serialPort: '',
    baud: 9600,
    flowControl: 'xonxoff',
    demoPorts: 24,
    udpListen: false,
    udpMcastGroup: '',
  };

  const TRANSPORTS: { value: TransportKind; label: string; short: string }[] = [
    { value: 'tcp', label: 'Rete (TCP/IP)', short: 'Rete' },
    { value: 'serial', label: 'Cavo seriale (RS232)', short: 'Seriale' },
    { value: 'demo', label: 'Simulatore (senza matrice)', short: 'Simulatore' },
  ];

  const FLOWS: { value: FlowControl; label: string }[] = [
    { value: 'xonxoff', label: 'XON/XOFF (impostazione consigliata)' },
    { value: 'none', label: 'Nessuno' },
    { value: 'hardware', label: 'Hardware (RTS/CTS)' },
  ];

  const BAUDS = [1200, 2400, 4800, 9600, 19200, 38400, 57600, 115200];

  type NumKey = 'timeoutConnectMs' | 'timeoutCmdMs' | 'heartbeatS' | 'pollXgetS' | 'delayMs';
  const NUM_FIELDS: {
    key: NumKey;
    label: string;
    unit: string;
    min: number;
    max: number;
    help: string;
  }[] = [
    {
      key: 'timeoutConnectMs',
      label: 'Attesa all’apertura del collegamento',
      unit: 'ms',
      min: 500,
      max: 30000,
      help:
        'Quanto l’app aspetta la matrice quando apre il collegamento. Se la rete è lenta e compare ' +
        '«connessione non riuscita» pur essendo tutto acceso, aumentalo.',
    },
    {
      key: 'timeoutCmdMs',
      label: 'Attesa della risposta a un comando',
      unit: 'ms',
      min: 200,
      max: 20000,
      help:
        'Tempo massimo di attesa per la risposta a un singolo comando. Se è troppo basso, un comando ' +
        'andato a buon fine può comparire come non riuscito.',
    },
    {
      key: 'heartbeatS',
      label: 'Controllo periodico del collegamento',
      unit: 's',
      min: 2,
      max: 300,
      help:
        'Ogni quanto l’app chiede alla matrice se è ancora raggiungibile e se qualcosa è cambiato. ' +
        'Più basso: ti accorgi prima di un cavo staccato, con un po’ più di traffico.',
    },
    {
      key: 'pollXgetS',
      label: 'Rilettura completa dello stato',
      unit: 's',
      min: 0,
      max: 3600,
      help:
        'Ogni quanto l’app rilegge tutte le porte quando non ascolta gli aggiornamenti UDP. ' +
        '0 = mai: lo stato si aggiorna comunque dopo ogni tua azione, ma le commutazioni fatte ' +
        'da altri (pannello frontale, altro PC) compaiono più tardi.',
    },
    {
      key: 'delayMs',
      label: 'Pausa di commutazione (--delay della matrice)',
      unit: 'ms',
      min: 0,
      max: 5000,
      help:
        'Tempo in cui un’uscita resta spenta prima di riaccendersi sul nuovo ingresso. Se un monitor ' +
        'resta nero o aggancia il segnale in ritardo, aumentalo.',
    },
  ];

  // ---------------------------------------------------------------- salvataggio automatico
  const SAVE_DELAY = 500;
  let saveState = $state<'idle' | 'pending' | 'saving' | 'saved' | 'error'>('idle');
  let savedAt = $state(0);
  let timer: ReturnType<typeof setTimeout> | null = null;

  function queueSave() {
    saveState = 'pending';
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => void flush(), SAVE_DELAY);
  }

  /** Scrive subito la configurazione; `msg` compare nel toast (per le modifiche importanti). */
  async function flush(msg?: string): Promise<boolean> {
    if (timer) {
      clearTimeout(timer);
      timer = null;
    }
    if (!app.config) return false;
    saveState = 'saving';
    const ok = await app.saveConfig(msg);
    saveState = ok ? 'saved' : 'error';
    if (ok) savedAt = Date.now();
    return ok;
  }

  // Niente perdita silenziosa: uscendo dalla schermata si scrive quello che è in attesa.
  $effect(() => {
    return () => {
      if (timer) {
        clearTimeout(timer);
        timer = null;
        void app.saveConfig();
      }
    };
  });

  // ---------------------------------------------------------------- bozze dei campi
  // I campi di testo e numero si scrivono in `drafts` mentre si digita e passano nella
  // configurazione solo quando il valore è valido (così un campo vuoto a metà digitazione
  // non arriva mai alla matrice). All'uscita dal campo una bozza non valida viene scartata.
  let drafts = $state<Record<string, string>>({});

  function dropDraft(id: string) {
    delete drafts[id];
  }

  function onNum(
    id: string,
    raw: string,
    min: number,
    max: number,
    set: (v: number) => void,
  ) {
    drafts[id] = raw;
    const n = Number(raw);
    if (raw.trim() === '' || !Number.isFinite(n)) return;
    const v = Math.round(n);
    if (v < min || v > max) return;
    set(v);
    queueSave();
  }

  function onText(id: string, raw: string, set: (v: string) => void, required = false) {
    drafts[id] = raw;
    if (required && raw.trim() === '') return;
    set(raw.trim());
    queueSave();
  }

  function setNum(k: NumKey, v: number) {
    const c = app.config;
    if (!c) return;
    c.settings[k] = v;
  }

  function setP<K extends keyof MatrixProfile>(id: string, k: K, v: MatrixProfile[K]) {
    const p = app.config?.profiles.find((x) => x.id === id);
    if (!p) return;
    p[k] = v;
  }

  // ---------------------------------------------------------------- profili
  let openId = $state<string | null>(app.config?.activeProfileId ?? null);
  const activeId = $derived(app.config?.activeProfileId ?? null);

  function transportLabel(t: TransportKind): string {
    return TRANSPORTS.find((x) => x.value === t)?.label ?? t;
  }

  function summary(p: MatrixProfile): string {
    if (p.transport === 'tcp') return `Rete · ${p.host || '(indirizzo mancante)'}:${p.port}`;
    if (p.transport === 'serial')
      return `Seriale · ${p.serialPort || '(porta non scelta)'} · ${p.baud} baud`;
    return `Simulatore · ${p.demoPorts} porte`;
  }

  function uniqueName(base: string): string {
    const taken = new Set(profiles.map((p) => p.name));
    if (!taken.has(base)) return base;
    for (let i = 2; i < 100; i++) if (!taken.has(`${base} ${i}`)) return `${base} ${i}`;
    return `${base} ${Date.now()}`;
  }

  async function addProfile() {
    const c = app.config;
    if (!c) return;
    const id = uuid();
    const p: MatrixProfile = { id, name: uniqueName('Nuova matrice'), ...NEW_PROFILE };
    c.profiles.push(p);
    c.labels[id] = [];
    c.presets[id] = [];
    openId = id;
    await flush(`Matrice «${p.name}» aggiunta`);
  }

  async function duplicateProfile(p: MatrixProfile) {
    const c = app.config;
    if (!c) return;
    const id = uuid();
    const copy: MatrixProfile = { ...p, id, name: uniqueName(`${p.name} (copia)`) };
    c.profiles.push(copy);
    c.labels[id] = (c.labels[p.id] ?? []).map((l) => ({ ...l }));
    c.presets[id] = (c.presets[p.id] ?? []).map((pr) => ({
      ...pr,
      id: uuid(),
      links: pr.links.map((l) => ({ ...l })),
    }));
    openId = id;
    await flush(`Matrice «${copy.name}» duplicata (etichette e preset compresi)`);
  }

  async function renameProfile(p: MatrixProfile) {
    const name = await app.prompt({
      title: 'Rinomina la matrice',
      message: 'Il nome serve solo a riconoscerla nell’elenco: non cambia nulla sull’impianto.',
      confirmLabel: 'Rinomina',
      input: { label: 'Nome', value: p.name, placeholder: 'Es. TLX24 Sala Controllo' },
    });
    if (name == null) return;
    const n = name.trim();
    if (!n || n === p.name) return;
    setP(p.id, 'name', n);
    await flush(`Matrice rinominata in «${n}»`);
  }

  async function deleteProfile(p: MatrixProfile) {
    const c = app.config;
    if (!c || c.profiles.length <= 1) return;
    const isActive = p.id === activeId;
    const presetCount = (c.presets[p.id] ?? []).length;
    const parts = [
      `Vengono eliminati anche le etichette delle porte e ${presetCount} preset di questa matrice.`,
      'La matrice e i suoi collegamenti non vengono toccati: cambia solo questa app.',
    ];
    if (isActive) parts.unshift('È la matrice in uso: l’app si collegherà a un’altra matrice dell’elenco.');
    const ok = await app.confirm({
      title: `Eliminare «${p.name}»?`,
      message: parts.join(' '),
      confirmLabel: 'Elimina',
      danger: true,
      requireWord: isActive && s?.confirmDangerous ? 'ELIMINA' : undefined,
    });
    if (!ok) return;
    const other = c.profiles.find((x) => x.id !== p.id);
    if (!other) return;
    if (isActive) await app.switchProfile(other.id);
    const i = c.profiles.findIndex((x) => x.id === p.id);
    if (i >= 0) c.profiles.splice(i, 1);
    delete c.labels[p.id];
    delete c.presets[p.id];
    if (openId === p.id) openId = other.id;
    await flush(`Matrice «${p.name}» eliminata`);
  }

  async function makeActive(id: string) {
    if (id === activeId) return;
    await flush();
    openId = id;
    await app.switchProfile(id);
  }

  async function reconnect() {
    await flush();
    await app.disconnect();
    await app.connect();
  }

  // ---------------------------------------------------------------- porte seriali
  let ports = $state<string[]>([]);
  let portsBusy = $state(false);
  let portsError = $state<string | null>(null);
  let portsAsked = false;
  let manual = $state<Record<string, boolean>>({});

  async function loadPorts() {
    portsBusy = true;
    portsError = null;
    try {
      ports = await api.listSerialPorts();
    } catch (e) {
      portsError = errText(e);
      ports = [];
    } finally {
      portsBusy = false;
    }
  }

  $effect(() => {
    const p = profiles.find((x) => x.id === openId);
    if (p?.transport === 'serial' && !portsAsked) {
      portsAsked = true;
      void loadPorts();
    }
  });

  function serialOptions(p: MatrixProfile): string[] {
    const list = [...ports];
    if (p.serialPort && !list.includes(p.serialPort)) list.unshift(p.serialPort);
    return list;
  }

  function onSerialSelect(p: MatrixProfile, v: string) {
    if (v === '__manual__') {
      manual[p.id] = true;
      return;
    }
    manual[p.id] = false;
    setP(p.id, 'serialPort', v);
    queueSave();
  }

  // ---------------------------------------------------------------- impostazioni generali
  async function setTheme(t: Settings['theme']) {
    const c = app.config;
    if (!c) return;
    c.settings.theme = t;
    await flush(
      t === 'system' ? 'Aspetto: come il sistema' : t === 'dark' ? 'Aspetto scuro' : 'Aspetto chiaro',
    );
  }

  async function setStrategy(v: Settings['bidirStrategy']) {
    const c = app.config;
    if (!c || c.settings.bidirStrategy === v) return;
    c.settings.bidirStrategy = v;
    await flush(
      v === 'sequence'
        ? 'Collegamenti con comandi separati (come il vecchio script)'
        : 'Collegamenti con un solo comando Macro',
    );
  }

  async function setConfirm(v: boolean) {
    const c = app.config;
    if (!c) return;
    c.settings.confirmDangerous = v;
    await flush(
      v ? 'Parola di conferma richiesta per le azioni pericolose' : 'Parola di conferma disattivata',
    );
  }

  const generalChanged = $derived(
    !!s &&
      (s.timeoutConnectMs !== FACTORY.timeoutConnectMs ||
        s.timeoutCmdMs !== FACTORY.timeoutCmdMs ||
        s.heartbeatS !== FACTORY.heartbeatS ||
        s.pollXgetS !== FACTORY.pollXgetS ||
        s.delayMs !== FACTORY.delayMs ||
        s.bidirStrategy !== FACTORY.bidirStrategy ||
        s.confirmDangerous !== FACTORY.confirmDangerous),
  );

  async function resetGeneral() {
    const c = app.config;
    if (!c) return;
    const ok = await app.confirm({
      title: 'Riportare i valori di fabbrica?',
      message:
        'Tempi di attesa, pausa di commutazione, modo di eseguire i collegamenti e conferma delle ' +
        'azioni pericolose tornano come appena installata. Matrici, etichette, preset, aspetto e ' +
        'modalità tecnico non vengono toccati.',
      confirmLabel: 'Riporta ai valori di fabbrica',
    });
    if (!ok) return;
    drafts = {};
    c.settings.timeoutConnectMs = FACTORY.timeoutConnectMs;
    c.settings.timeoutCmdMs = FACTORY.timeoutCmdMs;
    c.settings.heartbeatS = FACTORY.heartbeatS;
    c.settings.pollXgetS = FACTORY.pollXgetS;
    c.settings.delayMs = FACTORY.delayMs;
    c.settings.bidirStrategy = FACTORY.bidirStrategy;
    c.settings.confirmDangerous = FACTORY.confirmDangerous;
    await flush('Valori di fabbrica ripristinati');
  }

  // ---------------------------------------------------------------- importa / esporta
  let legacyTarget = $state(app.config?.activeProfileId ?? '');
  let busyLegacy = $state(false);
  let busyExport = $state(false);
  let busyImport = $state(false);

  /** Rilegge la configurazione dal backend (dopo un'importazione fatta dal backend). */
  async function reloadConfig(): Promise<AppConfig | null> {
    try {
      const fresh = await api.getConfig();
      app.config = fresh;
      return fresh;
    } catch (e) {
      app.toast('warn', 'Configurazione importata, ma la rilettura non è riuscita', [errText(e)]);
      return null;
    }
  }

  async function importLegacy() {
    const pid = legacyTarget || activeId;
    if (!pid || busyLegacy) return;
    const target = profiles.find((p) => p.id === pid);
    busyLegacy = true;
    try {
      await flush();
      const n = await api.importLegacyPresets(pid);
      if (n > 0) {
        await reloadConfig();
        app.toast(
          'ok',
          n === 1 ? '1 preset importato' : `${n} preset importati`,
          [`Aggiunti ai preset della matrice «${target?.name ?? ''}».`],
        );
      } else {
        app.toast('info', 'Nessun preset importato', [
          'Il file non conteneva preset nuovi, oppure non è stato scelto nessun file.',
        ]);
      }
    } catch (e) {
      app.toast('error', 'Importazione dei preset non riuscita', [errText(e)]);
    } finally {
      busyLegacy = false;
    }
  }

  async function exportAll() {
    if (busyExport) return;
    busyExport = true;
    try {
      await flush();
      const path = await api.exportConfig();
      if (path) app.toast('ok', 'Configurazione salvata', [path]);
    } catch (e) {
      app.toast('error', 'Salvataggio della configurazione non riuscito', [errText(e)]);
    } finally {
      busyExport = false;
    }
  }

  async function importAll() {
    if (busyImport) return;
    const ok = await app.confirm({
      title: 'Sostituire tutta la configurazione?',
      message:
        'Il file scelto sostituisce TUTTO quello che c’è adesso su questo computer: matrici, ' +
        'etichette delle porte, preset e impostazioni. Quello che c’è ora va perso. ' +
        'Se vuoi tenerne una copia, prima esportala.',
      confirmLabel: 'Scegli il file e sostituisci',
      danger: true,
      requireWord: s?.confirmDangerous ? 'SOSTITUISCI' : undefined,
    });
    if (!ok) return;
    busyImport = true;
    try {
      const cfg = await api.importConfig();
      if (!cfg) {
        app.toast('info', 'Importazione annullata: nessun file scelto');
        return;
      }
      app.config = cfg;
      drafts = {};
      openId = cfg.activeProfileId;
      legacyTarget = cfg.activeProfileId ?? '';
      await flush('Configurazione importata');
    } catch (e) {
      app.toast('error', 'Importazione non riuscita', [errText(e)]);
    } finally {
      busyImport = false;
    }
  }

  // ---------------------------------------------------------------- informazioni
  const ua = typeof navigator === 'undefined' ? '' : `${navigator.platform ?? ''} ${navigator.userAgent}`;
  const isWin = /win/i.test(ua);
  const isMac = /mac/i.test(ua);
  const configPath = isWin
    ? '%APPDATA%\\it.tt-group.tlx-control\\config.json'
    : isMac
      ? '~/Library/Application Support/it.tt-group.tlx-control/config.json'
      : '~/.config/it.tt-group.tlx-control/config.json';

  let version = $state('2.0.0');
  $effect(() => {
    void (async () => {
      try {
        const m = await import('@tauri-apps/api/app');
        version = await m.getVersion();
      } catch {
        /* anteprima nel browser: resta la versione della UI */
      }
    })();
  });

  async function copyPath() {
    try {
      await navigator.clipboard.writeText(configPath);
      app.toast('ok', 'Percorso copiato');
    } catch {
      app.toast('info', 'Copia non disponibile', [configPath]);
    }
  }
</script>

{#snippet numField(
  id: string,
  label: string,
  unit: string,
  value: number,
  def: number,
  min: number,
  max: number,
  help: string,
  set: (v: number) => void,
)}
  {@const str = drafts[id] ?? String(value)}
  {@const n = Number(str)}
  {@const bad = str.trim() === '' || !Number.isFinite(n) || n < min || n > max}
  <div class="field">
    <label class="label" for={id}>{label}{#if unit}{' '}<span class="muted">({unit})</span>{/if}</label>
    <div class="num-row">
      <input
        {id}
        class="input num"
        type="number"
        inputmode="numeric"
        {min}
        {max}
        value={str}
        aria-invalid={bad}
        aria-describedby="{id}-help"
        oninput={(e) => onNum(id, e.currentTarget.value, min, max, set)}
        onblur={() => dropDraft(id)}
      />
      <button
        class="btn ghost sm"
        disabled={value === def && !drafts[id]}
        title={`Valore di fabbrica: ${def}${unit ? ' ' + unit : ''}`}
        aria-label={`Riporta «${label}» al valore di fabbrica: ${def}${unit ? ' ' + unit : ''}`}
        onclick={() => {
          dropDraft(id);
          set(def);
          queueSave();
        }}
      >
        <Icon name="undo" size={15} /> {def}
      </button>
    </div>
    <p class="help" id="{id}-help">
      {help}
      {#if bad}<strong class="bad">Valore ammesso: da {min} a {max}{unit ? ' ' + unit : ''}.</strong>{/if}
    </p>
  </div>
{/snippet}

<div class="page">
  <div class="page-head">
    <div>
      <h1>Impostazioni</h1>
      <p>Matrici, tempi di attesa, aspetto e modalità tecnico. Le modifiche vengono salvate su questo computer.</p>
    </div>
    <div class="row">
      <span class="badge neutral">
        <Icon name="matrix" size={13} stroke={2} />{profiles.length} matrici
      </span>
      <button class="btn" onclick={() => setExpert(!app.expert)}>
        <Icon name="tool" size={16} />
        {app.expert ? 'Esci dalla modalità tecnico' : 'Attiva la modalità tecnico'}
      </button>
    </div>
  </div>

  {#if !s || !app.config}
    <div class="empty">Configurazione non disponibile.</div>
  {:else}
    <!-- ------------------------------------------------------------ come funziona il salvataggio -->
    <div class="callout info save-note" aria-live="polite">
      <Icon name="save" />
      <div class="grow">
        <p>
          <strong>Non c’è nulla da salvare a mano.</strong> Ogni modifica viene scritta subito su questo
          computer: qui accanto c’è sempre l’esito dell’ultimo salvataggio. Se cambi sezione mentre stai
          scrivendo, la modifica viene salvata comunque.
        </p>
      </div>
      <span
        class="badge"
        class:ok={saveState === 'saved'}
        class:error={saveState === 'error'}
        class:neutral={saveState === 'idle' || saveState === 'pending' || saveState === 'saving'}
      >
        {#if saveState === 'saved'}
          <Icon name="checkCircle" size={13} stroke={2} />Salvato alle {fmtTime(savedAt)}
        {:else if saveState === 'saving'}
          <Icon name="loader" size={13} stroke={2} spin />Salvataggio…
        {:else if saveState === 'pending'}
          <Icon name="dot" size={13} stroke={2} />Salvataggio fra un istante…
        {:else if saveState === 'error'}
          <Icon name="alert" size={13} stroke={2} />Salvataggio non riuscito
        {:else}
          <Icon name="check" size={13} stroke={2} />Nessuna modifica da salvare
        {/if}
      </span>
    </div>

    {#if app.isMock}
      <div class="callout warn">
        <Icon name="alert" />
        <p>
          Stai usando l’anteprima nel browser con la <strong>matrice simulata</strong>: niente di quello
          che fai qui arriva a una matrice vera.
        </p>
      </div>
    {/if}

    <!-- ------------------------------------------------------------ 1. profili matrice -->
    <h2 class="section-title" id="t-matrici">Matrici</h2>
    <section class="card" aria-labelledby="t-matrici">
      <header>
        <div>
          <h2>Elenco delle matrici</h2>
          <p>
            Una riga per ogni matrice che questa app sa comandare. Quella in uso è la matrice su cui
            agiscono Preset, Postazioni e tutte le altre schermate.
          </p>
        </div>
        <button class="btn" onclick={addProfile}><Icon name="plus" size={16} /> Aggiungi matrice</button>
      </header>

      <ul class="profiles" role="list">
        {#each profiles as p (p.id)}
          {@const isActive = p.id === activeId}
          {@const isOpen = openId === p.id}
          <li class="prof" class:active={isActive} class:open={isOpen}>
            <div class="prof-head">
              <label class="use">
                <input
                  type="radio"
                  name="matrice-in-uso"
                  checked={isActive}
                  onchange={() => makeActive(p.id)}
                />
                <span class="sr-only">Usa la matrice {p.name}</span>
              </label>
              <div class="prof-id grow">
                <div class="row tight">
                  <strong class="pname">{p.name}</strong>
                  {#if isActive}
                    <span class="badge accent"><Icon name="check" size={13} stroke={2.5} />In uso</span>
                  {/if}
                  {#if p.transport === 'demo'}
                    <span class="chip">Simulatore</span>
                  {/if}
                </div>
                <p class="small muted">{summary(p)}</p>
              </div>
              <div class="prof-tools">
                <button
                  class="btn sm"
                  aria-expanded={isOpen}
                  aria-controls={`prof-${p.id}`}
                  onclick={() => (openId = isOpen ? null : p.id)}
                >
                  <Icon name={isOpen ? 'chevronDown' : 'chevronRight'} size={16} />
                  {isOpen ? 'Chiudi' : 'Impostazioni'}
                </button>
                <button class="btn ghost sm icon-only" aria-label={`Rinomina ${p.name}`} title="Rinomina" onclick={() => renameProfile(p)}>
                  <Icon name="edit" size={16} />
                </button>
                <button class="btn ghost sm icon-only" aria-label={`Duplica ${p.name}`} title="Duplica" onclick={() => duplicateProfile(p)}>
                  <Icon name="copy" size={16} />
                </button>
                <button
                  class="btn ghost sm icon-only"
                  aria-label={`Elimina ${p.name}`}
                  title={profiles.length <= 1
                    ? 'Serve almeno una matrice: non si può eliminare l’ultima'
                    : 'Elimina'}
                  disabled={profiles.length <= 1}
                  onclick={() => deleteProfile(p)}
                >
                  <Icon name="trash" size={16} />
                </button>
              </div>
            </div>

            {#if isOpen}
              <div class="prof-body" id={`prof-${p.id}`}>
                {#if isActive && app.connected}
                  <div class="callout info">
                    <Icon name="info" />
                    <div class="grow">
                      <p>
                        È la matrice in uso e il collegamento è aperto: le modifiche qui sotto valgono
                        dal prossimo collegamento.
                      </p>
                    </div>
                    <button class="btn sm" onclick={reconnect}><Icon name="refresh" size={15} /> Riconnetti ora</button>
                  </div>
                {/if}

                <fieldset class="field">
                  <legend class="label">Come è collegata</legend>
                  <div class="seg" role="radiogroup" aria-label="Tipo di collegamento">
                    {#each TRANSPORTS as t (t.value)}
                      <button
                        type="button"
                        role="radio"
                        aria-checked={p.transport === t.value}
                        onclick={() => {
                          setP(p.id, 'transport', t.value);
                          queueSave();
                        }}
                      >
                        {t.short}
                      </button>
                    {/each}
                  </div>
                  <p class="help">{transportLabel(p.transport)}</p>
                </fieldset>

                {#if p.transport === 'tcp'}
                  <div class="fields">
                    <div class="field">
                      <label class="label" for={`host-${p.id}`}>Indirizzo IP virtuale</label>
                      <input
                        id={`host-${p.id}`}
                        class="input"
                        autocomplete="off"
                        spellcheck="false"
                        placeholder="192.168.13.15"
                        aria-describedby={`host-${p.id}-help`}
                        value={drafts[`${p.id}.host`] ?? p.host}
                        oninput={(e) =>
                          onText(`${p.id}.host`, e.currentTarget.value, (v) => setP(p.id, 'host', v), true)}
                        onblur={() => dropDraft(`${p.id}.host`)}
                      />
                      <p class="help" id={`host-${p.id}-help`}>
                        Qui va l’indirizzo <strong>virtuale</strong> della matrice (di fabbrica 192.168.13.15):
                        il controller primario risponde su questo indirizzo +100 e il backup su +1, ma
                        l’interfaccia di comando risponde solo sul controller attivo, quindi usa sempre
                        l’indirizzo virtuale.
                        {#if (drafts[`${p.id}.host`] ?? p.host).trim() === ''}
                          <strong class="bad">L’indirizzo non può restare vuoto.</strong>
                        {/if}
                      </p>
                    </div>
                    {@render numField(
                      `port-${p.id}`,
                      'Porta di rete',
                      '',
                      p.port,
                      17567,
                      1,
                      65535,
                      'Di fabbrica 17567. Cambiala solo se chi ha configurato la matrice l’ha cambiata.',
                      (v) => setP(p.id, 'port', v),
                    )}
                  </div>
                {:else if p.transport === 'serial'}
                  <div class="fields">
                    <div class="field">
                      <label class="label" for={`ser-${p.id}`}>Porta seriale del computer</label>
                      <div class="num-row">
                        <select
                          id={`ser-${p.id}`}
                          class="select"
                          aria-describedby={`ser-${p.id}-help`}
                          value={manual[p.id] ? '__manual__' : p.serialPort}
                          onchange={(e) => onSerialSelect(p, e.currentTarget.value)}
                        >
                          {#if !p.serialPort && !manual[p.id]}
                            <option value="">— scegli una porta —</option>
                          {/if}
                          {#each serialOptions(p) as sp (sp)}
                            <option value={sp}>{sp}</option>
                          {/each}
                          <option value="__manual__">Scrivi la porta a mano…</option>
                        </select>
                        <button
                          class="btn ghost sm icon-only"
                          aria-label="Rileggi l’elenco delle porte seriali"
                          title="Rileggi l’elenco"
                          onclick={loadPorts}
                        >
                          <Icon name="refresh" size={16} spin={portsBusy} />
                        </button>
                      </div>
                      {#if manual[p.id]}
                        <input
                          class="input"
                          autocomplete="off"
                          spellcheck="false"
                          aria-label="Nome della porta seriale"
                          placeholder={isWin ? 'COM3' : '/dev/cu.usbserial-1410'}
                          value={drafts[`${p.id}.serial`] ?? p.serialPort}
                          oninput={(e) =>
                            onText(`${p.id}.serial`, e.currentTarget.value, (v) =>
                              setP(p.id, 'serialPort', v),
                            )}
                          onblur={() => dropDraft(`${p.id}.serial`)}
                        />
                      {/if}
                      <p class="help" id={`ser-${p.id}-help`}>
                        Su Windows si chiamano COM3, COM4…; su macOS /dev/cu.usbserial-…
                        Se la porta giusta non compare nell’elenco, scrivila a mano.
                        {#if portsError}
                          <strong class="bad">Elenco non disponibile: {portsError}</strong>
                        {:else if !portsBusy && ports.length === 0}
                          <strong class="bad">Nessuna porta seriale rilevata su questo computer.</strong>
                        {/if}
                      </p>
                    </div>

                    <div class="field">
                      <label class="label" for={`baud-${p.id}`}>Velocità (baud)</label>
                      <select
                        id={`baud-${p.id}`}
                        class="select"
                        aria-describedby={`baud-${p.id}-help`}
                        value={String(p.baud)}
                        onchange={(e) => {
                          setP(p.id, 'baud', Number(e.currentTarget.value));
                          queueSave();
                        }}
                      >
                        {#each BAUDS as b (b)}
                          <option value={String(b)}>{b}{b === 9600 ? ' (di fabbrica)' : ''}</option>
                        {/each}
                      </select>
                      <p class="help" id={`baud-${p.id}-help`}>
                        La matrice esce di fabbrica a 9600. Se non è stata cambiata, lascia 9600.
                      </p>
                    </div>

                    <div class="field">
                      <label class="label" for={`flow-${p.id}`}>Controllo di flusso</label>
                      <select
                        id={`flow-${p.id}`}
                        class="select"
                        aria-describedby={`flow-${p.id}-help`}
                        value={p.flowControl}
                        onchange={(e) => {
                          setP(p.id, 'flowControl', e.currentTarget.value as FlowControl);
                          queueSave();
                        }}
                      >
                        {#each FLOWS as f (f.value)}
                          <option value={f.value}>{f.label}</option>
                        {/each}
                      </select>
                      <p class="help" id={`flow-${p.id}-help`}>
                        Se la matrice non risponde, prova a cambiare questa voce: è la causa più
                        frequente con il cavo seriale.
                      </p>
                    </div>
                  </div>
                  <div class="callout warn">
                    <Icon name="alert" />
                    <p>
                      Il cavo va collegato alla porta <strong>«RS232»</strong> del controller attivo,
                      <strong>non</strong> alla porta «CONSOLE»: dalla porta CONSOLE la matrice non accetta
                      questi comandi.
                    </p>
                  </div>
                {:else}
                  <div class="callout info">
                    <Icon name="info" />
                    <p>
                      <strong>Simulatore integrato: nessuna matrice collegata.</strong> Serve per provare
                      l’app, preparare etichette e preset e fare formazione senza toccare l’impianto.
                      Nessun comando esce da questo computer.
                    </p>
                  </div>
                  <div class="fields">
                    {@render numField(
                      `dports-${p.id}`,
                      'Porte del simulatore',
                      'porte',
                      p.demoPorts,
                      24,
                      2,
                      288,
                      'Quante porte finge di avere la matrice simulata. Mettine tante quante la matrice vera (di fabbrica 24).',
                      (v) => setP(p.id, 'demoPorts', v),
                    )}
                  </div>
                {/if}

                <h3 class="sub">Aggiornamento dello stato via UDP</h3>
                {#if p.transport === 'tcp'}
                  <label class="switch">
                    <input
                      type="checkbox"
                      checked={p.udpListen}
                      onchange={(e) => {
                        setP(p.id, 'udpListen', e.currentTarget.checked);
                        queueSave();
                      }}
                    />
                    <span>Ascolta gli aggiornamenti di stato sulla porta UDP 17564</span>
                  </label>
                  <div class="callout warn">
                    <Icon name="alert" />
                    <p>
                      Su Windows, attivandolo compare l’avviso del firewall e serve una regola in
                      <strong>ingresso</strong> per l’app. <strong>Lasciarlo spento va benissimo:</strong>
                      lo stato della matrice resta aggiornato comunque, perché l’app rilegge le porte dopo
                      ogni azione e a intervalli regolari.
                    </p>
                  </div>
                  <div class="fields">
                    <div class="field">
                      <label class="label" for={`mcast-${p.id}`}>Gruppo multicast (facoltativo)</label>
                      <input
                        id={`mcast-${p.id}`}
                        class="input"
                        autocomplete="off"
                        spellcheck="false"
                        placeholder="239.255.13.9"
                        disabled={!p.udpListen}
                        aria-describedby={`mcast-${p.id}-help`}
                        value={drafts[`${p.id}.mcast`] ?? p.udpMcastGroup}
                        oninput={(e) =>
                          onText(`${p.id}.mcast`, e.currentTarget.value, (v) =>
                            setP(p.id, 'udpMcastGroup', v),
                          )}
                        onblur={() => dropDraft(`${p.id}.mcast`)}
                      />
                      <p class="help" id={`mcast-${p.id}-help`}>
                        Vuoto = ascolta solo i messaggi in broadcast (è il caso normale). Compilalo solo se
                        la matrice è stata configurata per inviare in multicast, con lo stesso indirizzo
                        impostato sulla matrice (per esempio 239.255.13.9). Si può modificare solo con
                        l’ascolto UDP attivo.
                      </p>
                    </div>
                  </div>
                {:else}
                  <p class="help">
                    L’ascolto UDP esiste solo con il collegamento di rete. Con
                    {p.transport === 'serial' ? 'il cavo seriale' : 'il simulatore'} lo stato si legge
                    direttamente dalla matrice.
                  </p>
                {/if}
              </div>
            {/if}
          </li>
        {/each}
      </ul>
    </section>

    <!-- ------------------------------------------------------------ 2. impostazioni generali -->
    <h2 class="section-title" id="t-generali">Impostazioni generali</h2>
    <section class="card" aria-labelledby="t-generali">
      <header>
        <div>
          <h2>Tempi e comportamento</h2>
          <p>Valgono per tutte le matrici. Se non sai cosa cambiare, lascia i valori di fabbrica.</p>
        </div>
        <button class="btn" disabled={!generalChanged} onclick={resetGeneral}>
          <Icon name="undo" size={16} /> Valori di fabbrica
        </button>
      </header>

      <div class="fields">
        {#each NUM_FIELDS as f (f.key)}
          {@render numField(
            `set-${f.key}`,
            f.label,
            f.unit,
            s[f.key],
            FACTORY[f.key],
            f.min,
            f.max,
            f.help,
            (v) => setNum(f.key, v),
          )}
        {/each}
      </div>

      <div class="callout info verify">
        <Icon name="checkCircle" />
        <p>
          <strong>Dopo ogni commutazione lo stato della matrice viene sempre riletto e verificato</strong>
          (non si può disattivare): se quello che la matrice riporta non corrisponde a quello che hai
          chiesto, compare un avviso con la differenza.
        </p>
      </div>

      <h3 class="sub">Come vengono eseguiti i collegamenti bidirezionali</h3>
      <fieldset class="field">
        <legend class="sr-only">Come vengono eseguiti i collegamenti bidirezionali</legend>
        <div class="seg" role="radiogroup" aria-label="Modo di eseguire i collegamenti bidirezionali">
          <button
            type="button"
            role="radio"
            aria-checked={s.bidirStrategy === 'sequence'}
            onclick={() => setStrategy('sequence')}>Comandi separati</button
          >
          <button
            type="button"
            role="radio"
            aria-checked={s.bidirStrategy === 'macro'}
            onclick={() => setStrategy('macro')}>Un solo comando Macro</button
          >
        </div>
        <p class="help">
          <strong>Comandi separati</strong> (impostazione di fabbrica): quattro comandi uno dopo l’altro,
          esattamente come faceva il vecchio script Python; se uno va male gli altri non partono.
          <strong>Un solo comando Macro</strong>: la matrice esegue tutto in un colpo, più rapido, ma
          serve una matrice che lo supporti. Nel dubbio lascia «Comandi separati».
        </p>
      </fieldset>

      <h3 class="sub">Conferme</h3>
      <label class="switch">
        <input
          type="checkbox"
          checked={s.confirmDangerous}
          onchange={(e) => setConfirm(e.currentTarget.checked)}
        />
        <span>Per le azioni pericolose chiedi di scrivere una parola di conferma</span>
      </label>
      <p class="help">
        Con l’interruttore attivo, azioni come «Scollega tutto» o la sostituzione della configurazione
        chiedono di scrivere una parola (per esempio SCOLLEGA) prima di partire: un clic distratto non
        basta. La richiesta di conferma normale rimane in ogni caso.
      </p>

      <h3 class="sub">Aspetto</h3>
      <fieldset class="field">
        <legend class="sr-only">Aspetto</legend>
        <div class="seg" role="radiogroup" aria-label="Aspetto">
          <button type="button" role="radio" aria-checked={s.theme === 'system'} onclick={() => setTheme('system')}>
            <Icon name="monitorAuto" size={15} /> Come il sistema
          </button>
          <button type="button" role="radio" aria-checked={s.theme === 'light'} onclick={() => setTheme('light')}>
            <Icon name="sun" size={15} /> Chiaro
          </button>
          <button type="button" role="radio" aria-checked={s.theme === 'dark'} onclick={() => setTheme('dark')}>
            <Icon name="moon" size={15} /> Scuro
          </button>
        </div>
        <p class="help">
          «Come il sistema» segue l’impostazione di Windows o del Mac. In sala controllo, con le luci
          basse, lo sfondo scuro stanca meno la vista.
        </p>
      </fieldset>
    </section>

    <!-- ------------------------------------------------------------ 3. importa ed esporta -->
    <h2 class="section-title" id="t-dati">Importa ed esporta</h2>
    <section class="card" aria-labelledby="t-dati">
      <header>
        <div>
          <h2>Preset del vecchio programma</h2>
          <p>
            Porta qui i preset del file <code>tlx_presets.json</code> usato dal vecchio programma Python.
            I preset vengono <strong>aggiunti</strong> a quelli della matrice scelta: niente viene
            sostituito o cancellato.
          </p>
        </div>
      </header>
      <div class="fields">
        <div class="field">
          <label class="label" for="legacy-target">Aggiungi i preset alla matrice</label>
          <select
            id="legacy-target"
            class="select"
            value={legacyTarget || (activeId ?? '')}
            onchange={(e) => (legacyTarget = e.currentTarget.value)}
          >
            {#each profiles as p (p.id)}
              <option value={p.id}>{p.name}{p.id === activeId ? ' (in uso)' : ''}</option>
            {/each}
          </select>
          <p class="help">
            I preset del vecchio programma sono coppie di porte: diventano collegamenti bidirezionali,
            cioè esattamente quello che faceva prima il pulsante del preset.
          </p>
        </div>
        <div class="field self-end">
          <button class="btn primary" disabled={busyLegacy || profiles.length === 0} onclick={importLegacy}>
            <Icon name="upload" size={16} spin={busyLegacy} /> Importa i preset di tlx_presets.json
          </button>
          <p class="help">Si apre la finestra per scegliere il file.</p>
        </div>
      </div>

      <h3 class="sub">Tutta la configurazione</h3>
      <p class="help">
        Un solo file con matrici, etichette delle porte, preset e impostazioni: comodo per portare la
        stessa configurazione su un altro computer o per tenere una copia prima di cambiare qualcosa.
      </p>
      <div class="row dual">
        <button class="btn" disabled={busyExport} onclick={exportAll}>
          <Icon name="download" size={16} spin={busyExport} /> Esporta la configurazione
        </button>
        <button class="btn danger-outline" disabled={busyImport} onclick={importAll}>
          <Icon name="upload" size={16} spin={busyImport} /> Importa e sostituisci la configurazione
        </button>
      </div>
      <div class="callout warn">
        <Icon name="alert" />
        <p>
          L’importazione <strong>sostituisce tutto</strong>: matrici, etichette delle porte, preset e
          impostazioni di questo computer. Prima di importare, esporta una copia di quello che hai adesso.
        </p>
      </div>
    </section>

    <!-- ------------------------------------------------------------ 4. per l'assistenza -->
    <h2 class="section-title" id="t-info">Informazioni per l’assistenza</h2>
    <section class="card about" aria-labelledby="t-info">
      <dl>
        <div>
          <dt>Versione dell’app</dt>
          <dd class="mono">{version}</dd>
        </div>
        <div>
          <dt>File della configurazione</dt>
          <dd class="path">
            <code>{configPath}</code>
            <button class="btn ghost sm" onclick={copyPath}><Icon name="copy" size={15} /> Copia</button>
          </dd>
        </div>
        <div>
          <dt>Matrice in uso</dt>
          <dd>{app.activeProfile?.name ?? '—'} — {app.activeProfile ? summary(app.activeProfile) : ''}</dd>
        </div>
        <div>
          <dt>Stato del collegamento</dt>
          <dd>
            {app.connected ? 'Connesso' : 'Non connesso'}
            {#if app.snapshot.conn.endpoint}<span class="muted"> · {app.snapshot.conn.endpoint}</span>{/if}
          </dd>
        </div>
      </dl>
      <p class="help">
        Quando chiami l’assistenza, leggi la versione e, se te lo chiedono, invia il file della
        configurazione (il pulsante «Esporta la configurazione» qui sopra crea una copia da allegare).
      </p>
    </section>
  {/if}
</div>

<style>
  .save-note {
    align-items: center;
    margin-bottom: 4px;
  }
  .save-note p {
    max-width: 78ch;
  }
  .callout {
    margin-bottom: 12px;
  }
  .card > .callout:last-child {
    margin-bottom: 0;
  }
  .section-title {
    margin-top: 22px;
  }
  /* finestra stretta: titolo e pulsante della testata vanno a capo invece di schiacciarsi */
  .card > header {
    flex-wrap: wrap;
  }
  .card > header > div {
    min-width: 220px;
    flex: 1 1 320px;
  }
  .sub {
    margin: 20px 0 8px;
    padding-top: 14px;
    border-top: 1px solid var(--ui-divider);
    font-size: 14px;
  }
  .fields {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
    gap: 16px 20px;
    align-items: start;
  }
  .field .label {
    font-weight: 700;
    font-size: 13px;
  }
  .help {
    font-size: 12.5px;
    color: var(--ui-text-muted);
    max-width: 68ch;
  }
  .bad {
    color: var(--ui-danger-fg);
  }
  .num-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .num-row .input {
    max-width: 150px;
  }
  .num-row .select {
    flex: 1 1 auto;
    min-width: 0;
  }
  .self-end {
    justify-content: flex-end;
    gap: 6px;
  }
  .row.dual {
    gap: 10px;
    margin-bottom: 12px;
  }
  .row.tight {
    gap: 8px;
  }
  fieldset {
    border: 0;
    padding: 0;
    margin: 0;
    min-width: 0;
  }
  .seg {
    align-self: flex-start;
    max-width: 100%;
    flex-wrap: wrap;
  }

  /* ---------------------------------------------------------------- elenco matrici */
  .profiles {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .prof {
    border: 1px solid var(--ui-divider);
    border-radius: var(--r-md);
    background: var(--ui-bg);
    overflow: hidden;
  }
  .prof.active {
    border-color: var(--ui-selected-border);
    box-shadow: inset 3px 0 0 var(--ui-selected-border);
  }
  .prof-head {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 14px;
    flex-wrap: wrap;
  }
  .prof.open .prof-head {
    background: var(--ui-surface);
  }
  .use {
    display: inline-flex;
    align-items: center;
    cursor: pointer;
  }
  .use input {
    width: 18px;
    height: 18px;
    margin: 0;
    accent-color: var(--ui-primary);
    cursor: pointer;
  }
  .prof-id {
    min-width: 160px;
  }
  .pname {
    font-size: 14.5px;
  }
  .prof-tools {
    display: flex;
    align-items: center;
    gap: 4px;
    flex-wrap: wrap;
  }
  .prof-body {
    padding: 14px;
    border-top: 1px solid var(--ui-divider);
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .prof-body .fields {
    margin: 0;
  }
  .prof-body .sub {
    margin: 0;
  }
  .prof-body .callout {
    margin: 0;
    align-items: center;
  }
  .prof-body .switch + .callout {
    margin-top: 0;
  }

  /* ---------------------------------------------------------------- informazioni */
  .about dl {
    margin: 0 0 12px;
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
    gap: 12px 20px;
  }
  .about dt {
    font-weight: 700;
    font-size: 12.5px;
    color: var(--ui-text-muted);
  }
  .about dd {
    margin: 2px 0 0;
    word-break: break-word;
  }
  .about .path {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
</style>
