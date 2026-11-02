// Contratto IPC v1 — vedi docs/IPC.md. Tenere allineato con src-tauri/src/ipc.rs.

export type TransportKind = 'tcp' | 'serial' | 'demo';
export type FlowControl = 'xonxoff' | 'none' | 'hardware';

export interface MatrixProfile {
  id: string;
  name: string;
  transport: TransportKind;
  host: string;
  port: number;
  serialPort: string;
  baud: number;
  flowControl: FlowControl;
  demoPorts: number;
  udpListen: boolean;
  udpMcastGroup: string;
}

export type PortRole = 'tx' | 'rx' | 'other';
export interface PortLabel {
  port: number;
  name: string;
  role: PortRole;
  group: string;
  locked: boolean;
  notes: string;
}

/**
 * bidir  = logica script Python, esclusiva: DI a, DI b, CI a O b, CI b O a
 * kvm    = a = TX, b = RX, non esclusiva sul TX: DI b, CI a O b, CI b O a
 * oneway = solo video: CI a O b
 */
export type LinkMode = 'bidir' | 'kvm' | 'oneway';
export interface PresetLink {
  a: number;
  b: number;
  mode: LinkMode;
}
export interface Preset {
  id: string;
  name: string;
  links: PresetLink[];
  color: string;
  /**
   * true = «situazione completa» (creata da snapshot_to_preset): l'esecuzione ripristina TUTTA
   * la matrice come salvata — ogni uscita al suo ingresso, le uscite non elencate spente
   * (uscite bloccate saltate). `links` resta l'elenco leggibile (coppie reciproche bidir, resto oneway).
   * false = preset normale: esegue solo i link in ordine.
   */
  scene: boolean;
}

export type BidirStrategy = 'sequence' | 'macro';
export interface Settings {
  timeoutConnectMs: number;
  timeoutCmdMs: number;
  heartbeatS: number;
  pollXgetS: number;
  /** ignorato dal backend: la verifica dopo la commutazione è sempre eseguita */
  verifyAfterSwitch: boolean;
  delayMs: number;
  bidirStrategy: BidirStrategy;
  confirmDangerous: boolean;
  expertMode: boolean;
  theme: 'system' | 'light' | 'dark';
}

export interface AppConfig {
  version: 1;
  activeProfileId: string | null;
  profiles: MatrixProfile[];
  labels: Record<string, PortLabel[]>;
  presets: Record<string, Preset[]>;
  settings: Settings;
}

export type ConnStatus =
  | 'disconnected'
  | 'connecting'
  | 'handshaking'
  | 'connected'
  | 'reconnecting'
  | 'error';
export interface ConnState {
  status: ConnStatus;
  profileId: string | null;
  endpoint: string;
  error: string | null;
  since: number;
}

export interface MatrixInfo {
  version: string;
  xtype: string;
  maxPorts: number;
  maxCards: number;
  installed: boolean[];
  options: number;
  upperOnly: boolean;
  realCardCount: boolean;
  p2p: boolean;
  mcastStatus: boolean;
  xctime: boolean;
  losSupported: boolean | null;
}

export type RoutingSource = 'xget' | 'udp' | 'local';
export interface Routing {
  maxPorts: number;
  /** outputs[k] = ingresso collegato all'uscita k+1 (0 = scollegata) */
  outputs: number[];
  source: RoutingSource;
  updatedAt: number;
}

export interface Diagnostics {
  alarm: number | null;
  los: (boolean | null)[];
  rxUw: (number | null)[];
  txUw: (number | null)[];
  updatedAt: number;
}

export type LogLevel = 'info' | 'warn' | 'error' | 'tx' | 'rx';
export type LogChannel = 'tcp' | 'serial' | 'udp' | 'app' | 'demo';
export interface LogEntry {
  id: number;
  ts: number;
  level: LogLevel;
  channel: LogChannel;
  message: string;
  latencyMs: number | null;
}

export interface CmdResult {
  ok: boolean;
  command: string;
  response: string | null;
  data: string | null;
  errorCode: string | null;
  comment: string | null;
  error: string | null;
  latencyMs: number;
}

/**
 * ok=false con verified=null = «esito sconosciuto» (rilettura fallita o timeout).
 * warnings contiene anche effetti collaterali («Spente anche le uscite: 7 (Monitor sala)») e
 * sostituzioni nei preset («Nel preset, il collegamento X è stato sostituito da Y»).
 */
export interface ActionResult {
  ok: boolean;
  steps: CmdResult[];
  verified: boolean | null;
  mismatches: string[];
  message: string;
  warnings: string[];
}

export interface Snapshot {
  conn: ConnState;
  info: MatrixInfo | null;
  routing: Routing | null;
  diagnostics: Diagnostics | null;
  canUndo: boolean;
}

export interface RouteChange {
  /** 0 = scollega l'uscita */
  input: number;
  output: number;
}
