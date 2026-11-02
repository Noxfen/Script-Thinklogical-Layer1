# TLX Control — contratto IPC (backend Rust ↔ UI Svelte)

Versione contratto: 1. Tutti i nomi JSON sono **camelCase** (`#[serde(rename_all = "camelCase")]`).
Porte: interi 1..N. `0` = «nessuna porta». Tempi: millisecondi epoch (`number`).
I tipi TypeScript di riferimento sono in `ui/src/lib/types.ts` (fonte di verità lato UI);
i tipi Rust corrispondenti stanno in `src-tauri/src/ipc.rs` (o riesportati da `tlx-core`).

## Tipi

```ts
type TransportKind = 'tcp' | 'serial' | 'demo';
type FlowControl = 'xonxoff' | 'none' | 'hardware';

interface MatrixProfile {
  id: string;                 // uuid v4
  name: string;               // es. "TLX24 Showroom"
  transport: TransportKind;
  host: string;               // TCP: IP virtuale X (default 192.168.13.15)
  port: number;               // TCP: 17567
  serialPort: string;         // es. "COM3" / "/dev/tty.usbserial-1410"
  baud: number;               // 9600
  flowControl: FlowControl;   // default 'xonxoff' (spec §2.2)
  demoPorts: number;          // porte del simulatore in modalità demo (default 24)
  udpListen: boolean;         // ascolto stato UDP 17564 (solo TCP); default false (avviso firewall Windows)
  udpMcastGroup: string;      // '' = solo broadcast; es. "239.255.13.9"
}

type PortRole = 'tx' | 'rx' | 'other';
interface PortLabel { port: number; name: string; role: PortRole; group: string; locked: boolean; notes: string; }

type LinkMode = 'bidir' | 'kvm' | 'oneway';
// bidir  = logica dello script Python, ESCLUSIVA: DI a, DI b, CI a O b, CI b O a
//          (stacca a e b da qualunque altra uscita: giusto per coppie codec↔tablet)
// kvm    = a = porta del TX, b = porta dell'RX, NON esclusiva sul TX (altri RX che guardano
//          lo stesso TX restano): DI b, CI a O b, CI b O a  (Macro: M b0000 a b b a)
// oneway = solo video: CI a O b (a = ingresso, b = uscita)
interface PresetLink { a: number; b: number; mode: LinkMode; }
interface Preset { id: string; name: string; links: PresetLink[]; color: string;
  scene: boolean;   // true = «situazione completa»: ripristina TUTTA la matrice (uscite non elencate spente; bloccate saltate)
}

type BidirStrategy = 'sequence' | 'macro';
interface Settings {
  timeoutConnectMs: number;   // 3000
  timeoutCmdMs: number;       // 2000
  heartbeatS: number;         // 10  (XLASTEVENT a coda vuota; se cambia → XGET)
  pollXgetS: number;          // 60  (polling XGET quando UDP assente; 0 = off)
  verifyAfterSwitch: boolean; // IGNORATO: la verifica dopo ogni commutazione è obbligatoria
  delayMs: number;            // 300 (--delay della matrice)
  bidirStrategy: BidirStrategy; // 'sequence' = DI a, DI b, CI a O b, CI b O a (logica script); 'macro' = un solo M...
  confirmDangerous: boolean;  // true
  expertMode: boolean;        // false: nasconde console raw e comandi 9999 non confermati
  theme: 'system' | 'light' | 'dark';
}

interface AppConfig {
  version: 1;
  activeProfileId: string | null;
  profiles: MatrixProfile[];
  labels: Record<string, PortLabel[]>;   // chiave = profile id
  presets: Record<string, Preset[]>;     // chiave = profile id
  settings: Settings;
}

type ConnStatus = 'disconnected' | 'connecting' | 'handshaking' | 'connected' | 'reconnecting' | 'error';
interface ConnState { status: ConnStatus; profileId: string | null; endpoint: string; error: string | null; since: number; }

interface MatrixInfo {
  version: string; xtype: string; maxPorts: number; maxCards: number;
  installed: boolean[];        // per slot, da sinistra = scheda della porta 1
  options: number;             // bitmap XOPTIONS grezza
  upperOnly: boolean; realCardCount: boolean; p2p: boolean; mcastStatus: boolean; xctime: boolean;
  losSupported: boolean | null; // null = non ancora provato
}

type RoutingSource = 'xget' | 'udp' | 'local';
interface Routing { maxPorts: number; outputs: number[]; source: RoutingSource; updatedAt: number; }
// outputs[k] = ingresso collegato all'uscita k+1 (0 = scollegata). Lunghezza = maxPorts.

interface Diagnostics {
  alarm: number | null;
  los: (boolean | null)[];      // per porta, true = LOS
  rxUw: (number | null)[];      // µW
  txUw: (number | null)[];
  updatedAt: number;
}

type LogLevel = 'info' | 'warn' | 'error' | 'tx' | 'rx';
type LogChannel = 'tcp' | 'serial' | 'udp' | 'app' | 'demo';
interface LogEntry { id: number; ts: number; level: LogLevel; channel: LogChannel; message: string; latencyMs: number | null; }

interface CmdResult {
  ok: boolean; command: string;
  response: string | null;  // riga grezza (senza terminatore)
  data: string | null;      // dati fra OK e '#'
  errorCode: string | null; // "0007"
  comment: string | null;   // testo dopo '#'
  error: string | null;     // errore di trasporto/timeout
  latencyMs: number;
}
interface ActionResult {
  ok: boolean;
  steps: CmdResult[];
  verified: boolean | null;   // null = verifica non eseguita
  mismatches: string[];       // descrizioni leggibili ("uscita 5: atteso 1, letto 0")
  message: string;            // riepilogo per toast
  warnings: string[];         // es. «tastiera della porta 5 su più PC»
}

interface Snapshot { conn: ConnState; info: MatrixInfo | null; routing: Routing | null; diagnostics: Diagnostics | null; canUndo: boolean; }
interface RouteChange { input: number; output: number; } // input 0 = scollega l'uscita
```

## Comandi (`invoke`)

| comando | argomenti | ritorno | note |
|---|---|---|---|
| `get_config` | — | `AppConfig` | crea default al primo avvio (profilo «TLX24» 192.168.13.15:17567 + profilo «Demo») |
| `save_config` | `{ config: AppConfig }` | `void` | persistenza in `<app_config_dir>/config.json` |
| `export_config` | — | `string \| null` | dialog «Salva», ritorna percorso |
| `import_config` | — | `AppConfig \| null` | dialog «Apri» |
| `import_legacy_presets` | `{ profileId }` | `number` | dialog «Apri» su `tlx_presets.json` dello script Python; converte `{name,portA,portB}` in `Preset` bidir; ritorna quanti |
| `export_log` | — | `string \| null` | salva il log in CSV |
| `list_serial_ports` | — | `string[]` | |
| `connect` | `{ profileId }` | `void` | apre sessione persistente, handshake, XGET iniziale |
| `disconnect` | — | `void` | XQUIT (TCP) e chiusura |
| `get_snapshot` | — | `Snapshot` | |
| `refresh_routing` | — | `Routing` | XGET |
| `apply_routes` | `{ changes: RouteChange[] }` | `ActionResult` | raggruppa per ingresso → `CI` multi-uscita; input 0 → `DO`; rifiuta uscite bloccate |
| `link` | `{ a, b, mode }` | `ActionResult` | vedi `LinkMode`; `bidirStrategy` sceglie comandi separati o un Macro; ci si ferma al primo `ER` (nessun comando successivo), poi rilettura |
| `disconnect_port` | `{ port, mode }` | `ActionResult` | `mode`: `'input'` = `DI`, `'output'` = `DO`, `'both'` = `DI`+`DO` |
| `disconnect_all` | — | `ActionResult` | `DO` su ogni uscita collegata e non bloccata, a blocchi di 50 (UI chiede conferma) |
| `execute_preset` | `{ profileId, presetId }` | `ActionResult` | esegue i link in ordine; verifica finale |
| `snapshot_to_preset` | `{ name }` | `Preset` | async: rilegge (XGET), crea una **scena** (`scene: true`): coppie reciproche → bidir, il resto oneway (non salva: la UI lo aggiunge e chiama `save_config`) |
| `undo_last` | — | `ActionResult` | ripristina le uscite toccate dall'ultima azione |
| `send_raw` | `{ command, force }` | `CmdResult` | maiuscolo forzato; comandi pericolosi (`XRESET`, `XTEST*`, `XPUT`, `XHELP`, forme `9999`) rifiutati se `force=false` |
| `read_diagnostics` | — | `Diagnostics` | `XALARM`, `XLOS9999`, `XRXPWR9999`, `XTXPWR9999` (salta se `losSupported=false`) |
| `clear_log` | — | `void` | |
| `get_log` | — | `LogEntry[]` | ultimi 2000 |

Regole comuni alle commutazioni (`link`, `apply_routes`, `disconnect_*`, `execute_preset`, `undo_last`):
`XGET` fresco all'inizio; piano simulato; rifiuto se cambierebbe un'uscita bloccata (anche per effetto
di un `DI`); stop al primo `ER`/timeout; attesa `delayMs`; rilettura di tutta la matrice (seconda
rilettura dopo 700 ms se non torna); `ok` = nessun errore **e** stato verificato; `verified: null` =
esito sconosciuto; `warnings` elenca uscite spente come effetto collaterale, link del preset
sostituiti da link successivi, tastiere su più PC (ruoli da etichette o da link `kvm`).
`execute_preset` rifiuta un preset di un profilo diverso da quello connesso. `undo_last` rifiuta se
le uscite toccate sono cambiate nel frattempo.

Errori: i comandi ritornano `Result<T, String>`; la stringa è già in italiano e mostrabile.

## Eventi (`listen`)

| evento | payload |
|---|---|
| `tlx://conn` | `ConnState` |
| `tlx://info` | `MatrixInfo` |
| `tlx://routing` | `Routing` |
| `tlx://diag` | `Diagnostics` |
| `tlx://log` | `LogEntry` |
