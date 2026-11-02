// Backend simulato per il browser (`npm run dev` senza Tauri).
// Simula in memoria una matrice TLX: routing ingressi→uscite, link bidir/kvm/oneway, DI/DO,
// scollega tutto, preset, undo, diagnostica finta, log ed eventi `tlx://*`.
//
// Parametri URL utili:
//   ?ports=160     dimensione della matrice simulata (default 24)
//   ?profile=demo  avvia sul profilo «Demo (simulatore)» (etichette e ruoli completi)
//   ?offline=1     il profilo TCP non è raggiungibile (stato «Errore» + Riprova)
//   ?connect=0     niente connessione automatica all'avvio
//   ?los=0         firmware senza comandi Rev. M (LOS/potenze non supportati)
//   ?fail=0.2      probabilità di risposta ER per ogni comando di commutazione
//   ?theme=dark    forza il tema iniziale (utile per gli screenshot)
//   ?expert=1      avvia in modalità tecnico
//   ?screen=matrice apre direttamente una schermata
import type { Backend } from './api';
import type {
  ActionResult,
  AppConfig,
  CmdResult,
  ConnState,
  Diagnostics,
  LinkMode,
  LogChannel,
  LogEntry,
  LogLevel,
  MatrixInfo,
  MatrixProfile,
  PortLabel,
  Preset,
  PresetLink,
  RouteChange,
  Routing,
  Snapshot,
} from './types';
import { linkSummary, pad4, uuid } from './util';

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const clone = <T>(v: T): T => JSON.parse(JSON.stringify(v)) as T;

function param(name: string): string | null {
  try {
    return new URLSearchParams(window.location.search).get(name);
  } catch {
    return null;
  }
}

function hash(n: number): number {
  const x = Math.sin(n * 12.9898) * 43758.5453;
  return x - Math.floor(x);
}

// ---------------------------------------------------------------- configurazione di default

const LEGACY_PRESETS: { name: string; a: number; b: number }[] = [
  { name: 'Codec 1 ↔ Tablet A', a: 1, b: 5 },
  { name: 'Codec 1 ↔ Tablet B', a: 1, b: 8 },
  { name: 'Codec 2 ↔ Tablet A', a: 4, b: 5 },
  { name: 'Codec 2 ↔ Tablet B', a: 4, b: 8 },
];

function legacyPresets(): Preset[] {
  return LEGACY_PRESETS.map((p, i) => ({
    id: `legacy-${i + 1}`,
    name: p.name,
    links: [{ a: p.a, b: p.b, mode: 'bidir' as LinkMode }],
    color: '#0F6E78',
    scene: false,
  }));
}

function lbl(port: number, name: string, role: PortLabel['role'], group: string, locked = false, notes = ''): PortLabel {
  return { port, name, role, group, locked, notes };
}

/** Etichette di un impianto tipo (24 porte) per il profilo demo. */
function demoLabels(n: number): PortLabel[] {
  const base: PortLabel[] = [
    lbl(1, 'Codec 1', 'other', 'Sala conferenze'),
    lbl(2, 'PC Regia 1', 'tx', 'Regia'),
    lbl(3, 'PC Regia 2', 'tx', 'Regia'),
    lbl(4, 'Codec 2', 'other', 'Sala conferenze'),
    lbl(5, 'Tablet A', 'other', 'Sala conferenze'),
    lbl(6, 'Server SCADA', 'tx', 'Rack tecnico'),
    lbl(7, 'PC Streaming', 'tx', 'Regia'),
    lbl(8, 'Tablet B', 'other', 'Sala conferenze'),
    lbl(9, 'PC Grafica', 'tx', 'Regia'),
    lbl(10, 'PC Replay', 'tx', 'Regia'),
    lbl(11, 'PC Meteo', 'tx', 'Rack tecnico'),
    lbl(12, 'PC Telecamere TVCC', 'tx', 'Rack tecnico'),
    lbl(13, 'PC Archivio', 'tx', 'Rack tecnico'),
    lbl(14, 'PC Backup', 'tx', 'Rack tecnico', false, 'Solo manutenzione'),
    lbl(15, 'Postazione 1 – Capoturno', 'rx', 'Sala controllo'),
    lbl(16, 'Postazione 2', 'rx', 'Sala controllo'),
    lbl(17, 'Postazione 3', 'rx', 'Sala controllo'),
    lbl(18, 'Postazione 4', 'rx', 'Sala controllo'),
    lbl(19, 'Postazione 5', 'rx', 'Sala controllo'),
    lbl(20, 'Postazione 6', 'rx', 'Sala controllo'),
    lbl(21, 'Postazione Supervisore', 'rx', 'Sala controllo'),
    lbl(22, 'Postazione Ospiti', 'rx', 'Sala riunioni'),
    lbl(23, 'Videowall sinistro', 'rx', 'Sala controllo'),
    lbl(24, 'Proiettore sala riunioni', 'rx', 'Sala riunioni', true, 'Non toccare: cablato fisso'),
  ];
  if (n <= 24) return base.filter((l) => l.port <= n);
  // Matrici grandi: metà PC (tx) a rack da 16, metà postazioni (rx) a sale da 12.
  const res: PortLabel[] = [];
  const half = Math.floor(n / 2);
  for (let p = 1; p <= n; p++) {
    if (p <= half) res.push(lbl(p, `PC ${String(p).padStart(3, '0')}`, 'tx', `Rack ${Math.ceil(p / 16)}`));
    else {
      const k = p - half;
      res.push(lbl(p, `Postazione ${k}`, 'rx', `Sala ${String.fromCharCode(64 + Math.ceil(k / 12))}`, k % 37 === 0));
    }
  }
  return res;
}

function demoPresets(): Preset[] {
  return [
    ...legacyPresets().map((p) => ({ ...p, id: `demo-${p.id}` })),
    {
      id: 'demo-turno-giorno',
      name: 'Turno giorno: regia su postazioni 1 e 2',
      links: [
        { a: 2, b: 15, mode: 'kvm' },
        { a: 3, b: 16, mode: 'kvm' },
      ],
      color: '#0A4A52',
      scene: false,
    },
    {
      id: 'demo-videowall',
      name: 'Videowall: grafica e telecamere',
      links: [
        { a: 9, b: 23, mode: 'oneway' },
        { a: 12, b: 17, mode: 'kvm' },
      ],
      color: '#53565A',
      scene: false,
    },
    {
      id: 'demo-scena-notte',
      name: 'Situazione completa: turno notte',
      links: [
        { a: 6, b: 15, mode: 'kvm' },
        { a: 11, b: 16, mode: 'kvm' },
        { a: 12, b: 23, mode: 'oneway' },
      ],
      color: '#5BA8B2',
      scene: true,
    },
    {
      id: 'demo-emergenza',
      name: 'Emergenza: Server SCADA al supervisore',
      links: [{ a: 6, b: 21, mode: 'kvm' }],
      color: '#000000',
      scene: false,
    },
  ];
}

function defaultConfig(ports: number): AppConfig {
  const tlx: MatrixProfile = {
    id: 'profilo-tlx24',
    name: 'TLX24',
    transport: 'tcp',
    host: '192.168.13.15',
    port: 17567,
    serialPort: '',
    baud: 9600,
    flowControl: 'xonxoff',
    demoPorts: 24,
    udpListen: true,
    udpMcastGroup: '',
  };
  const demo: MatrixProfile = {
    ...tlx,
    id: 'profilo-demo',
    name: 'Demo (simulatore)',
    transport: 'demo',
    host: '',
    demoPorts: ports,
    udpListen: false,
  };
  const startDemo = param('profile') === 'demo' || param('ports') != null;
  return {
    version: 1,
    activeProfileId: startDemo ? demo.id : tlx.id,
    profiles: [tlx, demo],
    labels: {
      [tlx.id]: [
        lbl(1, 'Codec 1', 'other', 'Sala conferenze'),
        lbl(4, 'Codec 2', 'other', 'Sala conferenze'),
        lbl(5, 'Tablet A', 'other', 'Sala conferenze'),
        lbl(8, 'Tablet B', 'other', 'Sala conferenze'),
      ],
      [demo.id]: demoLabels(ports),
    },
    presets: { [tlx.id]: legacyPresets(), [demo.id]: demoPresets() },
    settings: {
      timeoutConnectMs: 3000,
      timeoutCmdMs: 2000,
      heartbeatS: 10,
      pollXgetS: 60,
      verifyAfterSwitch: true,
      delayMs: 300,
      bidirStrategy: 'sequence',
      confirmDangerous: true,
      expertMode: param('expert') === '1',
      theme: (['light', 'dark', 'system'].includes(param('theme') ?? '')
        ? param('theme')
        : 'system') as AppConfig['settings']['theme'],
    },
  };
}

// ---------------------------------------------------------------- backend simulato

export function createMockBackend(): Backend {
  const paramPorts = Number(param('ports'));
  const PORTS = Number.isFinite(paramPorts) && paramPorts >= 2 ? Math.min(1024, Math.floor(paramPorts)) : 24;
  const offline = param('offline') === '1';
  const losSupported = param('los') !== '0';
  const failRate = Math.max(0, Math.min(1, Number(param('fail')) || 0));

  let config = defaultConfig(PORTS);
  let conn: ConnState = { status: 'disconnected', profileId: null, endpoint: '', error: null, since: Date.now() };
  let info: MatrixInfo | null = null;
  let routing: Routing | null = null;
  let diagnostics: Diagnostics | null = null;
  let outputs: number[] = [];
  const undoStack: number[][] = [];
  let log: LogEntry[] = [];
  let logId = 0;
  let timers: ReturnType<typeof setInterval>[] = [];
  let lastEvent = 1;

  const listeners = new Map<string, Set<(p: unknown) => void>>();
  function emit(event: string, payload: unknown) {
    const set = listeners.get(event);
    if (!set) return;
    const data = clone(payload);
    queueMicrotask(() => set.forEach((cb) => cb(data)));
  }

  function channel(): LogChannel {
    const p = activeConnProfile();
    if (!p) return 'app';
    return p.transport === 'tcp' ? 'tcp' : p.transport === 'serial' ? 'serial' : 'demo';
  }

  function addLog(level: LogLevel, message: string, ch: LogChannel = 'app', latencyMs: number | null = null) {
    const e: LogEntry = { id: ++logId, ts: Date.now(), level, channel: ch, message, latencyMs };
    log.push(e);
    if (log.length > 2000) log = log.slice(-2000);
    emit('tlx://log', e);
  }

  function setConn(patch: Partial<ConnState>) {
    conn = { ...conn, ...patch, since: Date.now() };
    emit('tlx://conn', conn);
  }

  function activeConnProfile(): MatrixProfile | undefined {
    return config.profiles.find((p) => p.id === conn.profileId);
  }

  function labelsOf(): PortLabel[] {
    return (conn.profileId && config.labels[conn.profileId]) || [];
  }

  function isLocked(port: number): boolean {
    return labelsOf().some((l) => l.port === port && l.locked);
  }

  function matrixSize(p: MatrixProfile): number {
    if (param('ports') != null) return PORTS;
    return p.transport === 'demo' ? Math.max(2, p.demoPorts || 24) : 24;
  }

  function seedRouting(n: number) {
    outputs = new Array(n).fill(0);
    const set = (inp: number, out: number) => {
      if (inp <= n && out <= n) outputs[out - 1] = inp;
    };
    if (n <= 24) {
      // codec 1 ↔ tablet A (bidir), codec 2 ↔ tablet B
      set(1, 5); set(5, 1); set(4, 8); set(8, 4);
      // KVM regia → postazioni
      set(2, 15); set(15, 2); set(3, 16); set(16, 3); set(9, 17); set(17, 9);
      set(6, 18); set(18, 6); set(10, 19); set(19, 10);
      // grafica anche sul videowall (multicast video)
      set(9, 23);
      // tastiera della postazione 3 anche sul PC Meteo: attiva il banner di sicurezza
      set(17, 11);
    } else {
      const half = Math.floor(n / 2);
      for (let k = 1; k <= n - half; k++) {
        if (hash(k) < 0.7) {
          const tx = 1 + Math.floor(hash(k + 1000) * half);
          const rx = half + k;
          set(tx, rx);
          if (hash(k + 7) < 0.8) set(rx, tx);
        }
      }
    }
  }

  function makeInfo(n: number): MatrixInfo {
    const cards = Math.ceil(n / 8);
    const installed = new Array(cards).fill(true);
    if (n > 24) installed[cards - 1] = false;
    return {
      version: 'TLX 4.12.7 Rev. M',
      xtype: n <= 12 ? 'TLX12' : n <= 24 ? 'TLX24' : n <= 48 ? 'TLX48' : n <= 80 ? 'TLX80' : n <= 160 ? 'TLX160' : 'TLX320',
      maxPorts: n,
      maxCards: cards,
      installed,
      options: 0b10110,
      upperOnly: false,
      realCardCount: true,
      p2p: true,
      mcastStatus: false,
      xctime: true,
      losSupported,
    };
  }

  function publishRouting(source: Routing['source']) {
    routing = { maxPorts: outputs.length, outputs: outputs.slice(), source, updatedAt: Date.now() };
    emit('tlx://routing', routing);
  }

  function requireConnected() {
    if (conn.status !== 'connected') throw 'Non connesso alla matrice: collegati prima di inviare comandi.';
  }

  function checkPort(p: number) {
    if (!Number.isInteger(p) || p < 1 || p > outputs.length) throw `Porta ${p} non valida (1–${outputs.length}).`;
  }

  // ------------------------------------------------ esecuzione comandi ASCII simulati

  function okRes(command: string, data = '', comment: string | null = null, latencyMs = 0): CmdResult {
    return {
      ok: true,
      command,
      response: `OK${data}#${comment ?? ''}`,
      data: data || null,
      errorCode: null,
      comment,
      error: null,
      latencyMs,
    };
  }
  function erRes(command: string, code: string, comment: string, latencyMs = 0): CmdResult {
    return { ok: false, command, response: `ER${code}#${comment}`, data: null, errorCode: code, comment, error: null, latencyMs };
  }

  function exec(command: string): CmdResult {
    const c = command.trim().toUpperCase();
    const n = outputs.length;
    const isSwitch = /^(CI|DI|DO|M)/.test(c);
    if (isSwitch && failRate > 0 && Math.random() < failRate) return erRes(c, '0007', 'porta occupata');
    let m: RegExpMatchArray | null;
    if ((m = c.match(/^CI(\d{4})((?:O\d{4})+)$/))) {
      const inp = Number(m[1]);
      const outs = m[2].match(/\d{4}/g)!.map(Number);
      if (inp < 1 || inp > n || outs.some((o) => o < 1 || o > n)) return erRes(c, '0003', 'porta fuori intervallo');
      outs.forEach((o) => (outputs[o - 1] = inp));
      lastEvent++;
      return okRes(c);
    }
    if ((m = c.match(/^DI(\d{4})$/))) {
      const p = Number(m[1]);
      if (p === 9999) outputs.fill(0);
      else if (p < 1 || p > n) return erRes(c, '0003', 'porta fuori intervallo');
      else outputs = outputs.map((i) => (i === p ? 0 : i));
      lastEvent++;
      return okRes(c);
    }
    if ((m = c.match(/^DO(\d{4})$/))) {
      const p = Number(m[1]);
      if (p === 9999) outputs.fill(0);
      else if (p < 1 || p > n) return erRes(c, '0003', 'porta fuori intervallo');
      else outputs[p - 1] = 0;
      lastEvent++;
      return okRes(c);
    }
    if ((m = c.match(/^M((?:\d{4})+)$/))) {
      // Macro simulata: gruppi da 4 cifre; "0000" separa i DI iniziali dalle coppie CI (in, out).
      const g = m[1].match(/\d{4}/g)!.map(Number);
      const sep = g.indexOf(0);
      const dis = sep >= 0 ? g.slice(0, sep) : [];
      const pairs = sep >= 0 ? g.slice(sep + 1) : g;
      dis.forEach((p) => (outputs = outputs.map((i) => (i === p ? 0 : i))));
      for (let k = 0; k + 1 < pairs.length; k += 2) outputs[pairs[k + 1] - 1] = pairs[k];
      lastEvent++;
      return okRes(c);
    }
    if ((m = c.match(/^SI(\d{4})$/))) {
      const p = Number(m[1]);
      const outs: number[] = [];
      outputs.forEach((i, k) => i === p && outs.push(k + 1));
      return okRes(c, outs.map(pad4).join(''));
    }
    switch (c) {
      case 'XGET':
        return okRes(c, outputs.map(pad4).join(''));
      case 'XVERSION':
        return okRes(c, info?.version ?? 'TLX 4.12.7 Rev. M');
      case 'XTYPE':
        return okRes(c, info?.xtype ?? 'TLX24');
      case 'XMAXPORTS':
        return okRes(c, pad4(n));
      case 'XOPTIONS':
        return okRes(c, pad4(info?.options ?? 0));
      case 'XSTATUSIO':
        return okRes(c, (info?.installed ?? []).map((x) => (x ? '1' : '0')).join(''));
      case 'XALARM':
        return okRes(c, '0000');
      case 'XLASTEVENT':
        return okRes(c, String(lastEvent).padStart(8, '0'));
      case 'XLOS9999':
      case 'XRXPWR9999':
      case 'XTXPWR9999':
        if (!losSupported) return erRes(c, '0002', 'comando sconosciuto');
        return okRes(c, fakeDiagData(c));
      case 'XQUIT':
        return okRes(c, '', 'bye');
      case 'XHELP':
        return okRes(c, '', 'CI DI DO SI M XGET XVERSION XTYPE XMAXPORTS XOPTIONS XSTATUSIO XALARM XLASTEVENT XLOS XRXPWR XTXPWR');
      case 'XRESET':
        return okRes(c, '', 'reset simulato');
    }
    return erRes(c, '0002', 'comando sconosciuto');
  }

  function fakeDiag(): Diagnostics {
    const n = outputs.length;
    const los: (boolean | null)[] = [];
    const rx: (number | null)[] = [];
    const tx: (number | null)[] = [];
    const inst = info?.installed ?? [];
    for (let p = 1; p <= n; p++) {
      const cardOk = inst[Math.floor((p - 1) / 8)] !== false;
      if (!losSupported || !cardOk) {
        los.push(null); rx.push(null); tx.push(null);
        continue;
      }
      const dark = p === 14 || p === 22 || (n > 24 && hash(p + 99) < 0.06);
      los.push(dark);
      rx.push(dark ? 0 : Math.round((180 + hash(p) * 420) * 10) / 10);
      tx.push(Math.round((420 + hash(p + 50) * 160) * 10) / 10);
    }
    return { alarm: 0, los, rxUw: rx, txUw: tx, updatedAt: Date.now() };
  }

  function fakeDiagData(cmd: string): string {
    const d = fakeDiag();
    if (cmd === 'XLOS9999') return d.los.map((x) => (x ? '1' : '0')).join('');
    const arr = cmd === 'XRXPWR9999' ? d.rxUw : d.txUw;
    return arr.map((v) => pad4(Math.round(v ?? 0))).join('');
  }

  async function cmd(command: string): Promise<CmdResult> {
    const ch = channel();
    addLog('tx', command, ch);
    const latency = 4 + Math.round(Math.random() * 14);
    await sleep(latency);
    const r = exec(command);
    r.latencyMs = latency;
    addLog(r.ok ? 'rx' : 'error', r.response ?? '', ch, latency);
    return r;
  }

  // ------------------------------------------------ azioni di commutazione

  function linkCommands(l: PresetLink): string[] {
    const a = pad4(l.a);
    const b = pad4(l.b);
    if (l.mode === 'oneway') return [`CI${a}O${b}`];
    const macro = config.settings.bidirStrategy === 'macro';
    if (l.mode === 'bidir')
      return macro ? [`M${a}${b}0000${a}${b}${b}${a}`] : [`DI${a}`, `DI${b}`, `CI${a}O${b}`, `CI${b}O${a}`];
    return macro ? [`M${b}0000${a}${b}${b}${a}`] : [`DI${b}`, `CI${a}O${b}`, `CI${b}O${a}`];
  }

  function expectedAfter(l: PresetLink, exp: Map<number, number>) {
    exp.set(l.b, l.a);
    if (l.mode !== 'oneway') exp.set(l.a, l.b);
  }

  function warningsNow(): string[] {
    const labels = labelsOf();
    const count = new Map<number, number>();
    outputs.forEach((i) => i && count.set(i, (count.get(i) ?? 0) + 1));
    const res: string[] = [];
    for (const [inp, c] of count) {
      const l = labels.find((x) => x.port === inp);
      if (c > 1 && l?.role === 'rx') res.push(`Tastiera della porta ${inp} (${l.name || 'senza nome'}) su più PC`);
    }
    return res;
  }

  /** Esegue una sequenza di comandi; si ferma al primo errore; verifica finale opzionale. */
  async function runAction(
    commands: string[],
    expected: Map<number, number>,
    okMessage: string,
    pushUndo = true,
    extraWarnings: string[] = [],
  ): Promise<ActionResult> {
    requireConnected();
    const before = outputs.slice();
    const steps: CmdResult[] = [];
    let ok = true;
    for (const c of commands) {
      const r = await cmd(c);
      steps.push(r);
      if (!r.ok) {
        ok = false;
        break;
      }
    }
    if (pushUndo && steps.length && before.some((v, k) => v !== outputs[k])) {
      undoStack.push(before);
      if (undoStack.length > 20) undoStack.shift();
    }
    let verified: boolean | null = null;
    const mismatches: string[] = [];
    const prof = activeConnProfile();
    // Verifica sempre eseguita (settings.verifyAfterSwitch è ignorato dal backend).
    await sleep(Math.min(config.settings.delayMs, 400));
    if (failRate > 0 && Math.random() < failRate / 2) {
      // rilettura persa: esito sconosciuto
      addLog('tx', 'XGET', channel());
      addLog('error', `Timeout: nessuna risposta entro ${config.settings.timeoutCmdMs} ms`, channel());
      steps.push({ ok: false, command: 'XGET', response: null, data: null, errorCode: null, comment: null, error: 'timeout', latencyMs: config.settings.timeoutCmdMs });
      publishRouting(prof?.udpListen ? 'udp' : 'local');
      const message = 'Esito non verificato: la rilettura della matrice non ha risposto';
      addLog('warn', message, 'app');
      return { ok: false, steps, verified: null, mismatches, message, warnings: [...extraWarnings, ...warningsNow()] };
    }
    const r = await cmd('XGET');
    steps.push(r);
    for (const [out, inp] of expected) {
      const got = outputs[out - 1];
      if (got !== inp) mismatches.push(`uscita ${out}: atteso ${inp}, letto ${got}`);
    }
    verified = ok && mismatches.length === 0;
    publishRouting(prof?.udpListen ? 'udp' : 'xget');
    const failed = steps.find((s) => !s.ok);
    const message = ok
      ? okMessage
      : `Comando ${failed?.command} rifiutato dalla matrice (${failed?.errorCode ?? 'errore'}${failed?.comment ? ': ' + failed.comment : ''}). Comandi successivi non inviati.`;
    addLog(ok ? 'info' : 'error', message, 'app');
    return { ok, steps, verified, mismatches, message, warnings: [...(ok ? extraWarnings : []), ...warningsNow()] };
  }

  /**
   * Simula una sequenza di link su una copia della routing per calcolare gli effetti collaterali:
   * uscite spente dai DI esclusivi e link del preset sostituiti da link successivi.
   */
  function planLinks(links: PresetLink[]): string[] {
    const o = outputs.slice();
    const start = outputs.slice();
    const setBy = new Map<number, number>();
    const cleared = new Set<number>();
    const replaced: string[] = [];
    const note = (out: number, idx: number) => {
      const prev = setBy.get(out);
      if (prev !== undefined && prev !== idx) {
        const msg = `Nel preset, il collegamento ${linkSummary(links[prev])} è stato sostituito da ${linkSummary(links[idx])}`;
        if (!replaced.includes(msg)) replaced.push(msg);
        setBy.delete(out);
      }
    };
    links.forEach((l, idx) => {
      const dis = l.mode === 'bidir' ? [l.a, l.b] : l.mode === 'kvm' ? [l.b] : [];
      o.forEach((inp, k) => {
        const out = k + 1;
        if (inp && dis.includes(inp)) {
          o[k] = 0;
          note(out, idx);
          cleared.add(out);
        }
      });
      note(l.b, idx);
      o[l.b - 1] = l.a;
      setBy.set(l.b, idx);
      if (l.mode !== 'oneway') {
        note(l.a, idx);
        o[l.a - 1] = l.b;
        setBy.set(l.a, idx);
      }
    });
    const off = [...cleared].filter((out) => o[out - 1] === 0 && start[out - 1] !== 0).sort((x, y) => x - y);
    const warnings = [...replaced];
    if (off.length) {
      const names = off.map((out) => {
        const l = labelsOf().find((x) => x.port === out);
        return l?.name ? `${out} (${l.name})` : String(out);
      });
      warnings.unshift(`Spente anche le uscite: ${names.join(', ')}`);
    }
    return warnings;
  }

  function checkNotLocked(...ports: number[]) {
    for (const p of ports) if (isLocked(p)) throw `La porta ${p} è bloccata: sbloccala nella schermata Porte per modificarla.`;
  }

  async function doLink(a: number, b: number, mode: LinkMode): Promise<ActionResult> {
    requireConnected();
    checkPort(a);
    checkPort(b);
    if (a === b) throw 'Porta A e porta B devono essere diverse.';
    checkNotLocked(a, b);
    const exp = new Map<number, number>();
    const l = { a, b, mode };
    expectedAfter(l, exp);
    const what = mode === 'oneway' ? `Video ${a} → ${b}` : mode === 'kvm' ? `Postazione KVM ${a} ↔ ${b}` : `Collegamento ${a} ↔ ${b}`;
    return runAction(linkCommands(l), exp, `${what} eseguito`, true, planLinks([l]));
  }

  // ------------------------------------------------ connessione

  function stopTimers() {
    timers.forEach(clearInterval);
    timers = [];
  }

  async function doConnect(profileId: string) {
    const p = config.profiles.find((x) => x.id === profileId);
    if (!p) throw 'Profilo inesistente.';
    if (conn.status === 'connected' || conn.status === 'connecting' || conn.status === 'handshaking') await doDisconnect();
    const endpoint =
      p.transport === 'tcp' ? `${p.host}:${p.port}` : p.transport === 'serial' ? `${p.serialPort || '—'} @ ${p.baud}` : `simulatore ${p.demoPorts} porte`;
    setConn({ status: 'connecting', profileId, endpoint, error: null });
    addLog('info', `Connessione a ${p.name} (${endpoint})…`, 'app');
    await sleep(500);
    if (conn.profileId !== profileId) return;
    const unreachable = (p.transport === 'tcp' && (offline || !p.host.trim())) || (p.transport === 'serial' && !p.serialPort);
    if (unreachable) {
      const err = p.transport === 'tcp' ? `Timeout di connessione a ${endpoint} dopo ${config.settings.timeoutConnectMs} ms` : 'Porta seriale non specificata';
      addLog('error', err, p.transport === 'tcp' ? 'tcp' : 'serial');
      setConn({ status: 'error', error: err });
      return;
    }
    setConn({ status: 'handshaking' });
    const n = matrixSize(p);
    if (outputs.length !== n) seedRouting(n);
    info = makeInfo(n);
    for (const c of ['XVERSION', 'XTYPE', 'XMAXPORTS', 'XOPTIONS', 'XSTATUSIO']) await cmd(c);
    await sleep(200);
    emit('tlx://info', info);
    await cmd('XGET');
    publishRouting('xget');
    setConn({ status: 'connected' });
    addLog('info', `Connesso a ${p.name}: ${info.xtype}, ${n} porte`, 'app');
    if (p.udpListen) addLog('info', 'Ascolto stato UDP sulla porta 17564', 'udp');
    const hb = Math.max(5, config.settings.heartbeatS) * 1000;
    timers.push(
      setInterval(() => {
        if (conn.status === 'connected') void cmd('XLASTEVENT');
      }, hb),
    );
    if (!p.udpListen && config.settings.pollXgetS > 0) {
      timers.push(
        setInterval(async () => {
          if (conn.status !== 'connected') return;
          await cmd('XGET');
          publishRouting('xget');
        }, config.settings.pollXgetS * 1000),
      );
    }
  }

  async function doDisconnect() {
    stopTimers();
    if (conn.status === 'connected') await cmd('XQUIT');
    const name = activeConnProfile()?.name;
    setConn({ status: 'disconnected', error: null });
    routing = null;
    info = null;
    diagnostics = null;
    undoStack.length = 0;
    if (name) addLog('info', `Disconnesso da ${name}`, 'app');
  }

  // ------------------------------------------------ file (solo browser)

  function download(name: string, mime: string, text: string) {
    const url = URL.createObjectURL(new Blob([text], { type: mime }));
    const a = document.createElement('a');
    a.href = url;
    a.download = name;
    a.click();
    setTimeout(() => URL.revokeObjectURL(url), 2000);
  }

  function pickFile(accept: string): Promise<string | null> {
    return new Promise((resolve) => {
      const inp = document.createElement('input');
      inp.type = 'file';
      inp.accept = accept;
      inp.onchange = async () => {
        const f = inp.files?.[0];
        resolve(f ? await f.text() : null);
      };
      inp.addEventListener('cancel', () => resolve(null));
      inp.click();
    });
  }

  // ------------------------------------------------ dispatcher

  const handlers: Record<string, (args: Record<string, unknown>) => Promise<unknown> | unknown> = {
    get_config: () => clone(config),
    save_config: (a) => {
      config = clone(a.config as AppConfig);
      addLog('info', 'Configurazione salvata', 'app');
    },
    export_config: () => {
      download('tlx-control-config.json', 'application/json', JSON.stringify(config, null, 2));
      return 'tlx-control-config.json';
    },
    import_config: async () => {
      const t = await pickFile('.json,application/json');
      if (!t) return null;
      try {
        const c = JSON.parse(t) as AppConfig;
        if (c.version !== 1 || !Array.isArray(c.profiles)) throw new Error();
        return c;
      } catch {
        throw 'File non valido: non è una configurazione di TLX Control.';
      }
    },
    import_legacy_presets: async (a) => {
      const pid = a.profileId as string;
      const t = await pickFile('.json,application/json');
      if (!t) return 0;
      let arr: { name: string; portA: number; portB: number }[];
      try {
        arr = JSON.parse(t);
        if (!Array.isArray(arr)) throw new Error();
      } catch {
        throw 'File non valido: atteso tlx_presets.json dello script Python.';
      }
      const list = (config.presets[pid] ??= []);
      for (const p of arr)
        list.push({ id: uuid(), name: String(p.name), links: [{ a: Number(p.portA), b: Number(p.portB), mode: 'bidir' }], color: '#0F6E78', scene: false });
      addLog('info', `Importati ${arr.length} preset dallo script Python`, 'app');
      return arr.length;
    },
    export_log: () => {
      const rows = ['id;ora;livello;canale;messaggio;latenza_ms'];
      for (const e of log)
        rows.push([e.id, new Date(e.ts).toISOString(), e.level, e.channel, `"${e.message.replace(/"/g, '""')}"`, e.latencyMs ?? ''].join(';'));
      download('tlx-log.csv', 'text/csv', rows.join('\n'));
      return 'tlx-log.csv';
    },
    list_serial_ports: () => ['COM1', 'COM3', '/dev/tty.usbserial-1410'],
    connect: (a) => {
      // Nel backend reale la connessione è asincrona e lo stato arriva con `tlx://conn`.
      void doConnect(a.profileId as string);
    },
    disconnect: () => doDisconnect(),
    get_snapshot: (): Snapshot => clone({ conn, info, routing, diagnostics, canUndo: undoStack.length > 0 }),
    refresh_routing: async () => {
      requireConnected();
      await cmd('XGET');
      publishRouting('xget');
      return clone(routing);
    },
    apply_routes: async (a) => {
      requireConnected();
      const changes = (a.changes as RouteChange[]) ?? [];
      changes.forEach((c) => checkPort(c.output));
      const locked = changes.filter((c) => isLocked(c.output)).map((c) => c.output);
      if (locked.length) throw `Uscite bloccate, nessuna modifica eseguita: ${locked.join(', ')}.`;
      const byInput = new Map<number, number[]>();
      for (const c of changes) {
        if (!byInput.has(c.input)) byInput.set(c.input, []);
        byInput.get(c.input)!.push(c.output);
      }
      const cmds: string[] = [];
      const exp = new Map<number, number>();
      for (const [inp, outs] of byInput) {
        if (inp === 0) outs.forEach((o) => cmds.push(`DO${pad4(o)}`));
        else cmds.push(`CI${pad4(inp)}` + outs.map((o) => `O${pad4(o)}`).join(''));
        outs.forEach((o) => exp.set(o, inp));
      }
      return runAction(cmds, exp, `${changes.length} ${changes.length === 1 ? 'modifica applicata' : 'modifiche applicate'}`);
    },
    link: (a) => doLink(Number(a.a), Number(a.b), a.mode as LinkMode),
    disconnect_port: async (a) => {
      requireConnected();
      const port = Number(a.port);
      checkPort(port);
      checkNotLocked(port);
      const mode = a.mode as 'input' | 'output' | 'both';
      if (!['input', 'output', 'both'].includes(mode)) throw `Modalità di scollegamento non valida: ${String(a.mode)}.`;
      const cmds: string[] = [];
      const exp = new Map<number, number>();
      if (mode !== 'output') {
        cmds.push(`DI${pad4(port)}`);
        outputs.forEach((i, k) => i === port && exp.set(k + 1, 0));
      }
      if (mode !== 'input') {
        cmds.push(`DO${pad4(port)}`);
        exp.set(port, 0);
      }
      return runAction(cmds, exp, `Porta ${port} scollegata`);
    },
    disconnect_all: () => {
      // DO per ogni uscita collegata e non bloccata (niente più DI9999)
      requireConnected();
      const exp = new Map<number, number>();
      const cmds: string[] = [];
      const skipped: number[] = [];
      outputs.forEach((inp, k) => {
        const out = k + 1;
        if (!inp) return;
        if (isLocked(out)) {
          skipped.push(out);
          return;
        }
        exp.set(out, 0);
        cmds.push(`DO${pad4(out)}`);
      });
      const w = skipped.length ? [`Uscite bloccate lasciate collegate: ${skipped.join(', ')}`] : [];
      return runAction(cmds, exp, `Tutto scollegato (${cmds.length} uscite)`, true, w);
    },
    execute_preset: async (a) => {
      requireConnected();
      const p = (config.presets[a.profileId as string] ?? []).find((x) => x.id === a.presetId);
      if (!p) throw 'Preset non trovato.';
      if (p.scene) {
        // Situazione completa: ogni uscita al suo ingresso salvato, le altre spente; bloccate saltate.
        const target = new Array(outputs.length).fill(0);
        for (const l of p.links) {
          if (l.b <= target.length) target[l.b - 1] = l.a;
          if (l.mode !== 'oneway' && l.a <= target.length) target[l.a - 1] = l.b;
        }
        const exp = new Map<number, number>();
        const byInput = new Map<number, number[]>();
        const skipped: number[] = [];
        target.forEach((inp, k) => {
          const out = k + 1;
          if (isLocked(out)) {
            if (outputs[k] !== inp) skipped.push(out);
            return;
          }
          exp.set(out, inp);
          if (outputs[k] === inp) return;
          if (!byInput.has(inp)) byInput.set(inp, []);
          byInput.get(inp)!.push(out);
        });
        const cmds: string[] = [];
        for (const [inp, outs] of byInput) {
          if (inp === 0) outs.forEach((o) => cmds.push(`DO${pad4(o)}`));
          else cmds.push(`CI${pad4(inp)}` + outs.map((o) => `O${pad4(o)}`).join(''));
        }
        const w = skipped.length ? [`Uscite bloccate non ripristinate: ${skipped.join(', ')}`] : [];
        return runAction(cmds, exp, `Situazione «${p.name}» ripristinata`, true, w);
      }
      for (const l of p.links) {
        checkPort(l.a);
        checkPort(l.b);
        checkNotLocked(l.a, l.b);
      }
      const exp = new Map<number, number>();
      const cmds: string[] = [];
      for (const l of p.links) {
        cmds.push(...linkCommands(l));
        expectedAfter(l, exp);
      }
      return runAction(cmds, exp, `Preset «${p.name}» eseguito`, true, planLinks(p.links));
    },
    snapshot_to_preset: async (a): Promise<Preset> => {
      requireConnected();
      await cmd('XGET'); // il backend rilegge la matrice prima di salvare
      publishRouting('xget');
      const links: PresetLink[] = [];
      const used = new Set<number>();
      outputs.forEach((inp, k) => {
        const out = k + 1;
        if (inp > 0 && inp < out && outputs[inp - 1] === out) {
          links.push({ a: inp, b: out, mode: 'bidir' });
          used.add(out);
          used.add(inp);
        }
      });
      outputs.forEach((inp, k) => {
        const out = k + 1;
        if (inp > 0 && !used.has(out)) links.push({ a: inp, b: out, mode: 'oneway' });
      });
      return { id: uuid(), name: String(a.name), links, color: '#0F6E78', scene: true };
    },
    undo_last: async () => {
      requireConnected();
      const prev = undoStack.pop();
      if (!prev) throw 'Niente da annullare.';
      const exp = new Map<number, number>();
      const cmds: string[] = [];
      prev.forEach((inp, k) => {
        if (outputs[k] !== inp) {
          exp.set(k + 1, inp);
          cmds.push(inp === 0 ? `DO${pad4(k + 1)}` : `CI${pad4(inp)}O${pad4(k + 1)}`);
        }
      });
      return runAction(cmds, exp, `Ultima azione annullata (${exp.size} uscite ripristinate)`, false);
    },
    send_raw: async (a) => {
      requireConnected();
      const c = String(a.command).trim().toUpperCase();
      const dangerous = /^(XRESET|XTEST|XPUT|XHELP)/.test(c) || (!/^X/.test(c) && /9999/.test(c));
      if (dangerous && !a.force) throw `Comando pericoloso «${c}»: richiede conferma (force).`;
      const r = await cmd(c);
      if (/^(CI|DI|DO|M)/.test(c)) publishRouting('local');
      return r;
    },
    read_diagnostics: async () => {
      requireConnected();
      await cmd('XALARM');
      if (losSupported) {
        await cmd('XLOS9999');
        await cmd('XRXPWR9999');
        await cmd('XTXPWR9999');
      } else {
        addLog('warn', 'LOS e potenze ottiche non supportati dal firmware (comandi Rev. M assenti)', 'app');
      }
      diagnostics = fakeDiag();
      emit('tlx://diag', diagnostics);
      return clone(diagnostics);
    },
    clear_log: () => {
      log = [];
    },
    get_log: () => clone(log.slice(-2000)),
  };

  addLog('info', 'TLX Control avviato (backend simulato nel browser)', 'app');
  if (param('connect') !== '0' && config.activeProfileId) void doConnect(config.activeProfileId);

  return {
    mock: true,
    invoke: async <T>(command: string, args?: Record<string, unknown>): Promise<T> => {
      const h = handlers[command];
      if (!h) throw `Comando sconosciuto: ${command}`;
      await sleep(5);
      return clone((await h(args ?? {})) ?? null) as T;
    },
    listen: async <T>(event: string, cb: (p: T) => void) => {
      if (!listeners.has(event)) listeners.set(event, new Set());
      const set = listeners.get(event)!;
      const f = cb as (p: unknown) => void;
      set.add(f);
      return () => set.delete(f);
    },
  };
}
