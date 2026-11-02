// Wrapper tipizzati dei comandi IPC (docs/IPC.md).
// In Tauri usa @tauri-apps/api; nel browser semplice (`npm run dev`) usa il backend simulato in ./mock.
import type {
  ActionResult,
  AppConfig,
  CmdResult,
  ConnState,
  Diagnostics,
  LinkMode,
  LogEntry,
  MatrixInfo,
  Preset,
  RouteChange,
  Routing,
  Snapshot,
} from './types';

export type InvokeFn = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
export type ListenFn = <T>(event: string, cb: (payload: T) => void) => Promise<() => void>;
export interface Backend {
  invoke: InvokeFn;
  listen: ListenFn;
  mock: boolean;
}

export const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

let backendP: Promise<Backend> | null = null;

export function backend(): Promise<Backend> {
  if (!backendP) {
    backendP = isTauri
      ? (async () => {
          const core = await import('@tauri-apps/api/core');
          const ev = await import('@tauri-apps/api/event');
          const b: Backend = {
            invoke: <T>(cmd: string, args?: Record<string, unknown>) => core.invoke<T>(cmd, args),
            listen: <T>(event: string, cb: (p: T) => void) =>
              ev.listen<T>(event, (e) => cb(e.payload)),
            mock: false,
          };
          return b;
        })()
      : import('./mock').then((m) => m.createMockBackend());
  }
  return backendP;
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const b = await backend();
  return b.invoke<T>(cmd, args);
}

export type DisconnectMode = 'input' | 'output' | 'both';

export const api = {
  getConfig: () => call<AppConfig>('get_config'),
  saveConfig: (config: AppConfig) => call<void>('save_config', { config }),
  exportConfig: () => call<string | null>('export_config'),
  importConfig: () => call<AppConfig | null>('import_config'),
  importLegacyPresets: (profileId: string) => call<number>('import_legacy_presets', { profileId }),
  exportLog: () => call<string | null>('export_log'),
  listSerialPorts: () => call<string[]>('list_serial_ports'),
  connect: (profileId: string) => call<void>('connect', { profileId }),
  disconnect: () => call<void>('disconnect'),
  getSnapshot: () => call<Snapshot>('get_snapshot'),
  refreshRouting: () => call<Routing>('refresh_routing'),
  applyRoutes: (changes: RouteChange[]) => call<ActionResult>('apply_routes', { changes }),
  link: (a: number, b: number, mode: LinkMode) => call<ActionResult>('link', { a, b, mode }),
  disconnectPort: (port: number, mode: DisconnectMode) =>
    call<ActionResult>('disconnect_port', { port, mode }),
  disconnectAll: () => call<ActionResult>('disconnect_all'),
  executePreset: (profileId: string, presetId: string) =>
    call<ActionResult>('execute_preset', { profileId, presetId }),
  snapshotToPreset: (name: string) => call<Preset>('snapshot_to_preset', { name }),
  undoLast: () => call<ActionResult>('undo_last'),
  sendRaw: (command: string, force: boolean) => call<CmdResult>('send_raw', { command, force }),
  readDiagnostics: () => call<Diagnostics>('read_diagnostics'),
  clearLog: () => call<void>('clear_log'),
  getLog: () => call<LogEntry[]>('get_log'),
};

export interface EventHandlers {
  conn: (c: ConnState) => void;
  info: (i: MatrixInfo) => void;
  routing: (r: Routing) => void;
  diag: (d: Diagnostics) => void;
  log: (e: LogEntry) => void;
}

/** Registra gli handler degli eventi `tlx://*`; ritorna la funzione di deregistrazione. */
export async function listenAll(h: EventHandlers): Promise<() => void> {
  const b = await backend();
  const offs = await Promise.all([
    b.listen<ConnState>('tlx://conn', h.conn),
    b.listen<MatrixInfo>('tlx://info', h.info),
    b.listen<Routing>('tlx://routing', h.routing),
    b.listen<Diagnostics>('tlx://diag', h.diag),
    b.listen<LogEntry>('tlx://log', h.log),
  ]);
  return () => offs.forEach((f) => f());
}
