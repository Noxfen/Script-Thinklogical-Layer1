// Stato globale dell'app (Svelte 5 runes): config, snapshot, log, toast, dialog, schermata,
// filtri del registro, diagnostica e cronologia della console.
import { api, backend, listenAll } from './api';
import type {
  ActionResult,
  AppConfig,
  CmdResult,
  ConnState,
  LogChannel,
  LogEntry,
  LogLevel,
  MatrixProfile,
  PortLabel,
  Preset,
  Snapshot,
} from './types';
import { errText, keyboardWarnings, portName, resultKind, type ScreenId } from './util';

export type ToastKind = 'ok' | 'warn' | 'error' | 'info';
export interface Toast {
  id: number;
  kind: ToastKind;
  title: string;
  lines: string[];
  details: string[];
  /** avvisi in evidenza (effetti collaterali, sostituzioni nei preset) */
  warnings: string[];
  sticky: boolean;
}

export interface DialogReq {
  title: string;
  message: string;
  confirmLabel: string;
  cancelLabel?: string;
  danger?: boolean;
  /** se impostata, l'utente deve digitarla per confermare */
  requireWord?: string;
  /** se impostato, il dialog chiede un testo (prompt) con questo valore iniziale */
  input?: { label: string; value: string; placeholder?: string };
  resolve: (v: string | boolean | null) => void;
}

/** Una riga della console tecnico: comando inviato e risposta grezza della matrice. */
export interface ConsoleEntry {
  id: number;
  ts: number;
  command: string;
  /** null = il comando non è partito (errore di trasporto): vedi `error` */
  result: CmdResult | null;
  error: string | null;
}

export const LOG_LEVELS: { value: LogLevel; label: string }[] = [
  { value: 'info', label: 'Informazioni' },
  { value: 'warn', label: 'Avvisi' },
  { value: 'error', label: 'Errori' },
  { value: 'tx', label: 'Inviati' },
  { value: 'rx', label: 'Ricevuti' },
];

export const LOG_CHANNELS: { value: LogChannel; label: string }[] = [
  { value: 'tcp', label: 'TCP' },
  { value: 'serial', label: 'Seriale' },
  { value: 'udp', label: 'UDP' },
  { value: 'app', label: 'Applicazione' },
  { value: 'demo', label: 'Simulatore' },
];

const SCREEN_KEY = 'tlx.screen';
export const EXPERT_SCREENS: ScreenId[] = ['matrice', 'porte', 'diagnostica', 'console', 'log'];

function loadScreen(): ScreenId {
  try {
    const q = new URLSearchParams(window.location.search).get('screen');
    if (q) return q as ScreenId;
    const s = localStorage.getItem(SCREEN_KEY);
    if (s) return s as ScreenId;
  } catch {
    /* storage non disponibile */
  }
  return 'preset';
}

const emptyConn: ConnState = { status: 'disconnected', profileId: null, endpoint: '', error: null, since: 0 };

class AppStore {
  ready = $state(false);
  loadError = $state<string | null>(null);
  isMock = $state(false);
  config = $state<AppConfig | null>(null);
  snapshot = $state<Snapshot>({ conn: emptyConn, info: null, routing: null, diagnostics: null, canUndo: false });
  log = $state.raw<LogEntry[]>([]);
  toasts = $state<Toast[]>([]);
  dialog = $state<DialogReq | null>(null);
  now = $state(Date.now());
  busy = $state(false);
  /** tema effettivo (risolto da settings.theme + preferenza di sistema) */
  theme = $state<'light' | 'dark'>('light');
  private _screen = $state<ScreenId>(loadScreen());
  private toastSeq = 0;

  // ------------------------------------------------ registro eventi (schermata Log)
  /** filtro testuale sul messaggio del registro */
  logQuery = $state('');
  /** livelli selezionati; insieme vuoto = tutti */
  logLevels = $state<Set<LogLevel>>(new Set());
  /** canali selezionati; insieme vuoto = tutti */
  logChannels = $state<Set<LogChannel>>(new Set());
  /** true = il registro mostrato resta fermo (le righe nuove continuano ad arrivare in `log`) */
  logPaused = $state(false);
  private logFrozen = $state.raw<LogEntry[]>([]);

  // ------------------------------------------------ diagnostica (schermata Diagnostica)
  /** lettura diagnostica in corso */
  diagBusy = $state(false);
  /** errore dell'ultima lettura diagnostica (null = nessuno) */
  diagError = $state<string | null>(null);

  // ------------------------------------------------ console tecnico (schermata Console)
  /** cronologia dei comandi inviati con la rispettiva risposta, dal più vecchio */
  consoleEntries = $state<ConsoleEntry[]>([]);
  /** comandi inviati per il richiamo con le frecce, dal più recente, senza duplicati */
  consoleRecall = $state.raw<string[]>([]);
  private consoleSeq = 0;

  // ------------------------------------------------ derivati
  settings = $derived(this.config?.settings);
  expert = $derived(!!this.config?.settings.expertMode);
  activeProfile = $derived<MatrixProfile | null>(
    this.config?.profiles.find((p) => p.id === this.config?.activeProfileId) ?? null,
  );
  connected = $derived(this.snapshot.conn.status === 'connected');
  routing = $derived(this.snapshot.routing);
  portCount = $derived(
    this.snapshot.info?.maxPorts ??
      this.snapshot.routing?.maxPorts ??
      (this.activeProfile?.transport === 'demo' ? this.activeProfile.demoPorts : 24),
  );
  labelList = $derived<PortLabel[]>(
    (this.config && this.activeProfile && this.config.labels[this.activeProfile.id]) || [],
  );
  labels = $derived(new Map(this.labelList.map((l) => [l.port, l])));
  presets = $derived<Preset[]>(
    (this.config && this.activeProfile && this.config.presets[this.activeProfile.id]) || [],
  );
  kbdWarnings = $derived(keyboardWarnings(this.snapshot.routing, this.labels));
  /** ultima diagnostica ricevuta (evento `tlx://diag` o `read_diagnostics`) */
  diagnostics = $derived(this.snapshot.diagnostics);
  /** false = firmware senza i comandi LOS/potenze (Rev. M): la schermata lo dice e non li chiede */
  losSupported = $derived(this.snapshot.info?.losSupported ?? null);
  /** righe del registro visibili: sorgente ferma se in pausa, poi filtri livello/canale/testo */
  visibleLog = $derived.by(() => {
    const src = this.logPaused ? this.logFrozen : this.log;
    const lv = this.logLevels;
    const ch = this.logChannels;
    const q = this.logQuery.trim().toLowerCase();
    return src.filter(
      (e) =>
        (lv.size === 0 || lv.has(e.level)) &&
        (ch.size === 0 || ch.has(e.channel)) &&
        (q === '' || e.message.toLowerCase().includes(q)),
    );
  });
  logFiltered = $derived(this.logLevels.size > 0 || this.logChannels.size > 0 || this.logQuery.trim() !== '');

  get screen(): ScreenId {
    const s = this._screen;
    if (s === 'console' && !this.expert) return 'preset';
    if (EXPERT_SCREENS.includes(s) && !this.expert) return 'preset';
    return s;
  }
  set screen(s: ScreenId) {
    this._screen = s;
    try {
      localStorage.setItem(SCREEN_KEY, s);
    } catch {
      /* ignora */
    }
  }

  name(port: number): string {
    return portName(this.labels, port);
  }

  // ------------------------------------------------ avvio
  async init() {
    try {
      const b = await backend();
      this.isMock = b.mock;
      await listenAll({
        conn: (c) => {
          const prev = this.snapshot.conn.status;
          this.snapshot.conn = c;
          if (c.status === 'connected' && prev !== 'connected') void this.refreshSnapshot();
          if (c.status === 'disconnected') {
            this.snapshot.routing = null;
            this.snapshot.info = null;
            this.snapshot.diagnostics = null;
            this.snapshot.canUndo = false;
          }
        },
        info: (i) => (this.snapshot.info = i),
        routing: (r) => (this.snapshot.routing = r),
        diag: (d) => (this.snapshot.diagnostics = d),
        log: (e) => this.pushLog(e),
      });
      const [config, snap, log] = await Promise.all([api.getConfig(), api.getSnapshot(), api.getLog()]);
      this.config = config;
      this.snapshot = snap;
      // unisce le righe già arrivate via evento con quelle storiche, senza duplicati
      const byId = new Map<number, LogEntry>();
      for (const e of [...log, ...this.log]) byId.set(e.id, e);
      this.log = [...byId.values()].sort((a, b) => a.id - b.id).slice(-2000);
      this.ready = true;
    } catch (e) {
      this.loadError = errText(e);
    }
    setInterval(() => (this.now = Date.now()), 1000);
  }

  pushLog(e: LogEntry) {
    const l = this.log;
    this.log = l.length >= 2000 ? [...l.slice(l.length - 1999), e] : [...l, e];
  }

  async refreshSnapshot() {
    try {
      this.snapshot = await api.getSnapshot();
    } catch {
      /* lo stato arriverà dagli eventi */
    }
  }

  // ------------------------------------------------ toast e dialog
  /** `sticky` = resta aperto finché l'utente non lo chiude. */
  toast(kind: ToastKind, title: string, lines: string[] = [], details: string[] = [], sticky = false, warnings: string[] = []) {
    const id = ++this.toastSeq;
    this.toasts.push({ id, kind, title, lines, details, warnings, sticky });
    if (this.toasts.length > 5) {
      // scarta prima i toast non persistenti più vecchi
      const i = this.toasts.findIndex((t) => !t.sticky);
      this.toasts.splice(i >= 0 ? i : 0, 1);
    }
    if (!sticky) setTimeout(() => this.dismiss(id), kind === 'error' ? 9000 : kind === 'warn' ? 7000 : 4000);
  }
  dismiss(id: number) {
    const i = this.toasts.findIndex((t) => t.id === id);
    if (i >= 0) this.toasts.splice(i, 1);
  }
  toastAction(r: ActionResult) {
    const kind = resultKind(r);
    const lines: string[] = [];
    let title = r.message;
    if (!r.ok && r.verified === null) {
      title = 'Esito non verificato: controlla la matrice';
      lines.push(r.message);
    }
    if (r.verified === true) lines.push('Verificato sulla matrice');
    if (r.verified === false && r.ok) lines.push('Verifica non riuscita: la matrice non corrisponde');
    lines.push(...r.mismatches.map((m) => `Differenza: ${m}`));
    const details = this.expert
      ? r.steps.map((s) => `${s.command} → ${s.response ?? s.error ?? '—'}${s.latencyMs ? ` (${s.latencyMs} ms)` : ''}`)
      : [];
    const sticky = r.warnings.length > 0 || r.mismatches.length > 0 || (!r.ok && r.verified === null);
    this.toast(r.warnings.length && kind === 'ok' ? 'warn' : kind, title, lines, details, sticky, r.warnings);
  }

  confirm(opts: Omit<DialogReq, 'resolve'>): Promise<boolean> {
    return new Promise((resolve) => {
      this.dialog = { ...opts, resolve: (v) => resolve(v === true || typeof v === 'string') };
    });
  }
  prompt(opts: Omit<DialogReq, 'resolve'>): Promise<string | null> {
    return new Promise((resolve) => {
      this.dialog = { ...opts, resolve: (v) => resolve(typeof v === 'string' ? v : null) };
    });
  }
  closeDialog(v: string | boolean | null) {
    const d = this.dialog;
    this.dialog = null;
    d?.resolve(v);
  }

  // ------------------------------------------------ azioni
  /** Esegue un'azione di commutazione: toast con l'esito e aggiornamento di canUndo. */
  async act(fn: () => Promise<ActionResult>): Promise<ActionResult | null> {
    if (this.busy) return null;
    this.busy = true;
    try {
      const r = await fn();
      this.toastAction(r);
      return r;
    } catch (e) {
      this.toast('error', errText(e));
      return null;
    } finally {
      this.busy = false;
      await this.refreshSnapshot();
    }
  }

  async saveConfig(okMsg?: string): Promise<boolean> {
    if (!this.config) return false;
    try {
      await api.saveConfig($state.snapshot(this.config) as AppConfig);
      if (okMsg) this.toast('ok', okMsg);
      return true;
    } catch (e) {
      this.toast('error', `Salvataggio non riuscito: ${errText(e)}`);
      return false;
    }
  }

  async connect(profileId?: string) {
    const id = profileId ?? this.activeProfile?.id;
    if (!id) return;
    try {
      await api.connect(id);
    } catch (e) {
      this.toast('error', errText(e));
    }
  }

  async disconnect() {
    try {
      await api.disconnect();
    } catch (e) {
      this.toast('error', errText(e));
    }
  }

  /** Cambia matrice: disconnette la vecchia, salva il profilo attivo e connette la nuova. */
  async switchProfile(id: string) {
    if (!this.config || this.config.activeProfileId === id) return;
    const st = this.snapshot.conn.status;
    if (st !== 'disconnected' && st !== 'error') await this.disconnect();
    this.config.activeProfileId = id;
    await this.saveConfig();
    await this.connect(id);
  }

  async undo() {
    if (!this.snapshot.canUndo || !this.connected) return;
    await this.act(() => api.undoLast());
  }

  async refresh() {
    if (!this.connected) return;
    try {
      const r = await api.refreshRouting();
      this.snapshot.routing = r;
    } catch (e) {
      this.toast('error', errText(e));
    }
  }

  async executePreset(p: Preset) {
    if (!this.activeProfile) return;
    if (!this.connected) {
      this.toast('warn', 'Matrice non connessa: il preset non è stato eseguito.');
      return;
    }
    const pid = this.activeProfile.id;
    await this.act(() => api.executePreset(pid, p.id));
  }

  async disconnectAll() {
    const s = this.settings;
    const ok = await this.confirm({
      title: 'Scollegare tutte le porte?',
      message:
        'Tutti i monitor e le tastiere collegati verranno staccati (le porte bloccate restano come sono). ' +
        'Le postazioni resteranno senza immagine finché non esegui un preset.',
      confirmLabel: 'Scollega tutto',
      danger: true,
      requireWord: s?.confirmDangerous ? 'SCOLLEGA' : undefined,
    });
    if (ok) await this.act(() => api.disconnectAll());
  }

  // ------------------------------------------------ registro eventi
  toggleLogLevel(l: LogLevel) {
    const s = new Set(this.logLevels);
    if (s.has(l)) s.delete(l);
    else s.add(l);
    this.logLevels = s;
  }
  toggleLogChannel(c: LogChannel) {
    const s = new Set(this.logChannels);
    if (s.has(c)) s.delete(c);
    else s.add(c);
    this.logChannels = s;
  }
  resetLogFilters() {
    this.logLevels = new Set();
    this.logChannels = new Set();
    this.logQuery = '';
  }
  /** In pausa il registro mostrato resta fermo: congela le righe presenti in quel momento. */
  setLogPaused(v: boolean) {
    if (v) this.logFrozen = this.log;
    this.logPaused = v;
  }
  toggleLogPause() {
    this.setLogPaused(!this.logPaused);
  }
  async clearLog() {
    try {
      await api.clearLog();
      this.log = [];
      this.logFrozen = [];
    } catch (e) {
      this.toast('error', errText(e));
    }
  }
  /** Salva il registro in CSV (dialog del sistema); toast con il percorso scelto. */
  async exportLog() {
    try {
      const path = await api.exportLog();
      if (path) this.toast('ok', 'Registro salvato', [path]);
    } catch (e) {
      this.toast('error', `Salvataggio del registro non riuscito: ${errText(e)}`);
    }
  }

  // ------------------------------------------------ diagnostica
  /** Rilegge allarme, LOS e potenze ottiche; aggiorna `snapshot.diagnostics`. */
  async readDiagnostics(): Promise<boolean> {
    if (!this.connected || this.diagBusy) return false;
    this.diagBusy = true;
    this.diagError = null;
    try {
      this.snapshot.diagnostics = await api.readDiagnostics();
      return true;
    } catch (e) {
      this.diagError = errText(e);
      this.toast('error', `Lettura diagnostica non riuscita: ${this.diagError}`);
      return false;
    } finally {
      this.diagBusy = false;
    }
  }

  // ------------------------------------------------ console tecnico
  /**
   * Invia un comando grezzo e lo registra nella cronologia.
   * La conferma dei comandi pericolosi (`isDangerousCommand`) spetta alla schermata:
   * qui `force` viene passato al backend così com'è.
   */
  async sendRaw(command: string, force = false): Promise<CmdResult | null> {
    const cmd = command.trim();
    if (!cmd) return null;
    const id = ++this.consoleSeq;
    this.consoleRecall = [cmd, ...this.consoleRecall.filter((c) => c !== cmd)].slice(0, 100);
    try {
      const result = await api.sendRaw(cmd, force);
      this.pushConsole({ id, ts: Date.now(), command: cmd, result, error: null });
      return result;
    } catch (e) {
      this.pushConsole({ id, ts: Date.now(), command: cmd, result: null, error: errText(e) });
      return null;
    }
  }
  private pushConsole(e: ConsoleEntry) {
    const l = this.consoleEntries;
    this.consoleEntries = l.length >= 200 ? [...l.slice(l.length - 199), e] : [...l, e];
  }
  clearConsole() {
    this.consoleEntries = [];
  }
}

export const app = new AppStore();
