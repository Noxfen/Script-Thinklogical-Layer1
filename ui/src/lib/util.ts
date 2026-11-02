// Funzioni di supporto pure (nessuno stato): etichette porte, preset, diagnostica, formati.
import type {
  ActionResult,
  LinkMode,
  PortLabel,
  Preset,
  PresetLink,
  Routing,
} from './types';

export type ScreenId =
  | 'preset'
  | 'postazioni'
  | 'matrice'
  | 'porte'
  | 'diagnostica'
  | 'console'
  | 'log'
  | 'impostazioni';

/** Palette preset compatibile con il brand TT (teal + grigi). */
export const PRESET_COLORS: { value: string; name: string }[] = [
  { value: '#0F6E78', name: 'Teal' },
  { value: '#5BA8B2', name: 'Teal chiaro' },
  { value: '#0A4A52', name: 'Teal scuro' },
  { value: '#53565A', name: 'Grigio scuro' },
  { value: '#A3A9AC', name: 'Grigio' },
  { value: '#000000', name: 'Nero' },
];

export const LINK_MODES: { value: LinkMode; label: string; help: string }[] = [
  {
    value: 'bidir',
    label: 'Bidirezionale (come prima)',
    help: 'A e B si vedono a vicenda. Prima stacca A e B da qualunque altro collegamento.',
  },
  {
    value: 'kvm',
    label: 'Postazione KVM',
    help: 'A = PC, B = postazione: immagine dal PC alla postazione e tastiera/mouse dalla postazione al PC. Gli altri monitor che guardano il PC restano collegati.',
  },
  { value: 'oneway', label: 'Solo video', help: 'Solo l’immagine da A verso B. Nessuna tastiera.' },
];

export function modeLabel(m: LinkMode): string {
  return m === 'bidir' ? 'Bidirezionale' : m === 'kvm' ? 'Postazione KVM' : 'Solo video';
}

export const ROLE_LABELS: Record<string, string> = {
  tx: 'PC (sorgente)',
  rx: 'Postazione (monitor e tastiera)',
  other: 'Altro',
};

export function defaultLabel(port: number): PortLabel {
  return { port, name: '', role: 'other', group: '', locked: false, notes: '' };
}

export function portName(labels: Map<number, PortLabel>, port: number): string {
  const l = labels.get(port);
  return l && l.name.trim() ? l.name : `Porta ${port}`;
}

export function linkArrow(m: LinkMode): string {
  return m === 'oneway' ? '→' : '↔';
}

export function linkSummary(l: PresetLink): string {
  return `P${l.a} ${linkArrow(l.mode)} P${l.b}`;
}

export function presetSummary(p: Preset, max = 2): string {
  if (p.links.length === 0) return 'Nessun collegamento';
  const parts = p.links.slice(0, max).map(linkSummary);
  const rest = p.links.length - max;
  return parts.join(', ') + (rest > 0 ? ` +${rest}` : '');
}

/** Ingresso che alimenta l'uscita `port` (0 = nessuno). */
export function inputOf(r: Routing | null, port: number): number {
  if (!r) return 0;
  return r.outputs[port - 1] ?? 0;
}

/** Uscite alimentate dall'ingresso `port`. */
export function outputsOf(r: Routing | null, port: number): number[] {
  const res: number[] = [];
  if (!r) return res;
  r.outputs.forEach((inp, k) => {
    if (inp === port) res.push(k + 1);
  });
  return res;
}

export function linkPresent(r: Routing | null, l: PresetLink): boolean {
  if (!r) return false;
  const ab = inputOf(r, l.b) === l.a;
  if (l.mode === 'oneway') return ab;
  return ab && inputOf(r, l.a) === l.b;
}

export function presetActive(r: Routing | null, p: Preset): boolean {
  return !!r && p.links.length > 0 && p.links.every((l) => linkPresent(r, l));
}

/** Conteggio uscite per ingresso (indice = porta). */
export function fanout(r: Routing | null): number[] {
  const n = r ? r.maxPorts : 0;
  const c = new Array(n + 1).fill(0);
  if (r) for (const i of r.outputs) if (i > 0 && i <= n) c[i]++;
  return c;
}

/**
 * Avvisi di sicurezza: un ingresso di una porta con ruolo `rx` (tastiera di una postazione)
 * che alimenta più di un'uscita = la stessa tastiera comanda più PC.
 */
export function keyboardWarnings(
  r: Routing | null,
  labels: Map<number, PortLabel>,
): { port: number; outputs: number[] }[] {
  if (!r) return [];
  const res: { port: number; outputs: number[] }[] = [];
  const f = fanout(r);
  for (let p = 1; p < f.length; p++) {
    if (f[p] > 1 && labels.get(p)?.role === 'rx') res.push({ port: p, outputs: outputsOf(r, p) });
  }
  return res;
}

/** dBm = 10·log10(µW / 1000). 0 o null → null. */
export function uwToDbm(uw: number | null | undefined): number | null {
  if (uw == null || !(uw > 0)) return null;
  return 10 * Math.log10(uw / 1000);
}

export function fmtDbm(uw: number | null | undefined): string {
  const d = uwToDbm(uw);
  return d == null ? '—' : `${d.toFixed(1).replace('.', ',')} dBm`;
}

export function fmtUw(uw: number | null | undefined): string {
  if (uw == null || !(uw > 0)) return '—';
  return `${uw.toFixed(1).replace('.', ',')} µW`;
}

export function fmtAge(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  if (s < 60) return `${s} s fa`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m} min fa`;
  const h = Math.floor(m / 60);
  return `${h} h fa`;
}

export function fmtTime(ts: number, withMs = false): string {
  const d = new Date(ts);
  const p = (n: number, l = 2) => String(n).padStart(l, '0');
  const base = `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
  return withMs ? `${base}.${p(d.getMilliseconds(), 3)}` : base;
}

export function uuid(): string {
  const c = globalThis.crypto as Crypto | undefined;
  if (c && typeof c.randomUUID === 'function') return c.randomUUID();
  const b = new Uint8Array(16);
  if (c) c.getRandomValues(b);
  else for (let i = 0; i < 16; i++) b[i] = Math.floor(Math.random() * 256);
  b[6] = (b[6] & 0x0f) | 0x40;
  b[8] = (b[8] & 0x3f) | 0x80;
  const h = [...b].map((x) => x.toString(16).padStart(2, '0')).join('');
  return `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20)}`;
}

export function errText(e: unknown): string {
  if (typeof e === 'string') return e;
  if (e instanceof Error) return e.message;
  try {
    return JSON.stringify(e);
  } catch {
    return String(e);
  }
}

/** Esito sintetico di un ActionResult per toast e badge. */
export function resultKind(r: ActionResult): 'ok' | 'warn' | 'error' {
  if (!r.ok && r.verified === null) return 'warn'; // esito sconosciuto (rilettura fallita)
  if (!r.ok) return 'error';
  if (r.mismatches.length || r.warnings.length || r.verified === false) return 'warn';
  return 'ok';
}

/** True se il focus è in un campo di testo (le scorciatoie a tasto singolo vanno ignorate). */
export function isTypingTarget(t: EventTarget | null): boolean {
  const el = t as HTMLElement | null;
  if (!el || !el.tagName) return false;
  const tag = el.tagName;
  if (tag === 'TEXTAREA' || tag === 'SELECT') return true;
  if (tag === 'INPUT') {
    const type = (el as HTMLInputElement).type;
    return !['checkbox', 'radio', 'button', 'submit', 'range', 'color'].includes(type);
  }
  return el.isContentEditable;
}

/**
 * Comandi raw considerati pericolosi (stessa regola del backend, IPC.md `send_raw`):
 * XRESET, XTEST*, XPUT, XHELP e le forme «tutte le porte» 9999 che modificano la routing.
 * Le letture X…9999 (XLOS9999, XRXPWR9999, XTXPWR9999) non sono considerate pericolose qui;
 * se il backend le rifiuta comunque, la UI chiede conferma e riprova con force=true.
 */
export function isDangerousCommand(cmd: string): boolean {
  const c = cmd.trim().toUpperCase();
  if (/^(XRESET|XTEST|XPUT|XHELP)/.test(c)) return true;
  if (/^X/.test(c)) return false;
  // Il ';' finale del Macro non è ammesso dalla logica di scenario (spec §3.1): le porte
  // scritte dopo il ';' danno ER0017 e il resto del Macro viene eseguito comunque.
  if (c.startsWith('M') && c.includes(';')) return true;
  return /9999/.test(c);
}

export function clamp(n: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, n));
}

export function pad4(n: number): string {
  return String(n).padStart(4, '0');
}

export function hex(n: number, digits = 4): string {
  return '0x' + (n >>> 0).toString(16).toUpperCase().padStart(digits, '0');
}

/** Filtro testuale su porta: numero, nome, gruppo. */
export function matchPort(q: string, port: number, l: PortLabel | undefined): boolean {
  const s = q.trim().toLowerCase();
  if (!s) return true;
  if (String(port) === s || `p${port}` === s) return true;
  if (/^\d+$/.test(s)) return String(port).startsWith(s);
  return (
    (l?.name ?? '').toLowerCase().includes(s) ||
    (l?.group ?? '').toLowerCase().includes(s) ||
    `porta ${port}`.includes(s)
  );
}
