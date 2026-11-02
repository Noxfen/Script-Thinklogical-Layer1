//! Pianificazione delle commutazioni: da un'intenzione (collega, scollega, preset, scena,
//! modifiche dalla griglia) alla sequenza di comandi ASCII e allo stato atteso.
//! Puro e testabile: nessun I/O qui. L'esecuzione (state.rs) simula il piano sullo stato
//! appena letto con XGET, rifiuta i piani che toccano uscite bloccate, e dopo l'invio rilegge
//! e confronta l'INTERA matrice.

use crate::config::{BidirStrategy, LinkMode, PortRole};
use std::collections::{BTreeMap, HashMap};

/// Uscita k+1 → ingresso (0 = scollegata).
pub type Outputs = Vec<u16>;

/// Righe lunghe al massimo 50 porte/coppie per comando (spec TT §3.3, V16).
const CHUNK: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    /// Comandi da inviare nell'ordine dato; ci si ferma al primo errore.
    pub commands: Vec<String>,
    /// Intenzione: coppie (ingresso, uscita) che l'utente vuole collegate a fine piano.
    pub intent: Vec<(u16, u16)>,
    /// Avvisi già noti in pianificazione.
    pub warnings: Vec<String>,
}

/// Contesto della matrice per pianificare.
pub struct Ctx<'a> {
    pub max: u16,
    pub roles: &'a HashMap<u16, PortRole>,
    /// La matrice dichiara il supporto P2P (XOPTIONS bit 4): un solo CI per uscita.
    pub p2p: bool,
}

pub fn p4(p: u16) -> String {
    format!("{p:04}")
}

pub fn check_port(p: u16, max: u16) -> Result<(), String> {
    if p == 0 || p > max {
        Err(format!("Porta {p} fuori intervallo (1–{max})"))
    } else {
        Ok(())
    }
}

/// Collegamento fra due porte secondo la modalità.
pub fn plan_link(
    a: u16,
    b: u16,
    mode: LinkMode,
    strategy: BidirStrategy,
    ctx: &Ctx,
) -> Result<Plan, String> {
    check_port(a, ctx.max)?;
    check_port(b, ctx.max)?;
    if a == b && mode != LinkMode::Oneway {
        return Err("Le due porte devono essere diverse".into());
    }
    let mut pl = Plan::default();
    let (pa, pb) = (p4(a), p4(b));
    match mode {
        LinkMode::Bidir => {
            // Logica dello script Python: DI a, DI b, CI a O b, CI b O a (esclusiva).
            pl.commands = match strategy {
                BidirStrategy::Sequence => vec![
                    format!("DI{pa}"),
                    format!("DI{pb}"),
                    format!("CI{pa}O{pb}"),
                    format!("CI{pb}O{pa}"),
                ],
                BidirStrategy::Macro => vec![format!("M{pa}0000{pb}0000{pa}{pb}{pb}{pa}")],
            };
            pl.intent = vec![(a, b), (b, a)];
        }
        LinkMode::Kvm => {
            // a = TX (PC), b = RX (postazione). DI sull'ingresso dell'RX prima del ritorno: la
            // tastiera non deve restare su altri PC (manuale API App. C; spec TT §4.1).
            let ra = ctx.roles.get(&a).copied();
            let rb = ctx.roles.get(&b).copied();
            if ra == Some(PortRole::Rx) || rb == Some(PortRole::Tx) {
                return Err(format!(
                    "Postazione KVM al contrario: la porta {a} deve essere il PC (TX) e la {b} la postazione (RX). Inverti le porte."
                ));
            }
            pl.commands = match strategy {
                BidirStrategy::Sequence => {
                    vec![format!("DI{pb}"), format!("CI{pa}O{pb}"), format!("CI{pb}O{pa}")]
                }
                BidirStrategy::Macro => vec![format!("M{pb}0000{pa}{pb}{pb}{pa}")],
            };
            pl.intent = vec![(a, b), (b, a)];
        }
        LinkMode::Oneway => {
            pl.commands = vec![format!("CI{pa}O{pb}")];
            pl.intent = vec![(a, b)];
        }
    }
    Ok(pl)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscMode {
    Input,
    Output,
    Both,
}

/// Uscite alimentate dall'ingresso `inp` nello stato `cur`.
pub fn outputs_of(cur: &Outputs, inp: u16) -> Vec<u16> {
    cur.iter()
        .enumerate()
        .filter(|(_, &i)| i == inp && inp != 0)
        .map(|(k, _)| k as u16 + 1)
        .collect()
}

pub fn plan_disconnect(port: u16, mode: DiscMode, ctx: &Ctx) -> Result<Plan, String> {
    check_port(port, ctx.max)?;
    let mut pl = Plan::default();
    if matches!(mode, DiscMode::Input | DiscMode::Both) {
        pl.commands.push(format!("DI{}", p4(port)));
    }
    if matches!(mode, DiscMode::Output | DiscMode::Both) {
        pl.commands.push(format!("DO{}", p4(port)));
    }
    Ok(pl)
}

fn do_chunks(outs: &[u16]) -> Vec<String> {
    outs.chunks(CHUNK)
        .map(|c| format!("DO{}", c.iter().map(|p| p4(*p)).collect::<String>()))
        .collect()
}

/// «Scollega tutto»: DO su ogni uscita collegata e non bloccata (niente DI9999: le forme 9999
/// sono vietate dalla logica di scenario, spec TT §3.1, e scavalcherebbero i blocchi).
pub fn plan_disconnect_all(cur: &Outputs, locked: &[u16]) -> Result<Plan, String> {
    let busy: Vec<u16> = (1..=cur.len() as u16)
        .filter(|o| cur[*o as usize - 1] != 0 && !locked.contains(o))
        .collect();
    if busy.is_empty() {
        return Err("Nessuna uscita collegata da scollegare".into());
    }
    let mut pl = Plan { commands: do_chunks(&busy), ..Default::default() };
    let kept: Vec<u16> = locked
        .iter()
        .copied()
        .filter(|o| cur.get(*o as usize - 1).copied().unwrap_or(0) != 0)
        .collect();
    if !kept.is_empty() {
        pl.warnings.push(format!("Uscite bloccate lasciate collegate: {}", join(&kept)));
    }
    Ok(pl)
}

/// Modifiche puntuali: (ingresso, uscita), ingresso 0 = scollega l'uscita.
/// Raggruppa per ingresso in un `CI` multi-uscita (uno per uscita se la matrice supporta P2P);
/// le scollegate in `DO`. Prima le disconnessioni, poi le connessioni.
pub fn plan_routes(changes: &[(u16, u16)], ctx: &Ctx) -> Result<Plan, String> {
    if changes.is_empty() {
        return Err("Nessuna modifica da applicare".into());
    }
    let mut by_out: BTreeMap<u16, u16> = BTreeMap::new();
    for &(i, o) in changes {
        check_port(o, ctx.max)?;
        if i != 0 {
            check_port(i, ctx.max)?;
        }
        by_out.insert(o, i); // l'ultima modifica per uscita vince
    }
    let mut pl = Plan::default();
    let mut dos = vec![];
    let mut by_in: BTreeMap<u16, Vec<u16>> = BTreeMap::new();
    for (&o, &i) in &by_out {
        if i == 0 {
            dos.push(o);
        } else {
            by_in.entry(i).or_default().push(o);
            pl.intent.push((i, o));
        }
    }
    pl.commands.extend(do_chunks(&dos));
    for (i, os) in &by_in {
        let per = if ctx.p2p { 1 } else { CHUNK };
        for c in os.chunks(per) {
            pl.commands.push(format!(
                "CI{}{}",
                p4(*i),
                c.iter().map(|o| format!("O{}", p4(*o))).collect::<String>()
            ));
        }
    }
    Ok(pl)
}

/// Uscite che una scena porterebbe allo stato `target`, saltando le bloccate.
pub fn plan_scene(
    target: &Outputs,
    cur: &Outputs,
    locked: &[u16],
    ctx: &Ctx,
) -> Result<Plan, String> {
    if target.len() != cur.len() {
        return Err(format!(
            "La situazione salvata è per una matrice da {} porte, quella connessa ne ha {}",
            target.len(),
            cur.len()
        ));
    }
    let mut changes = vec![];
    let mut skipped = vec![];
    for o in 1..=cur.len() as u16 {
        let k = o as usize - 1;
        if target[k] != cur[k] {
            if locked.contains(&o) {
                skipped.push(o);
            } else {
                changes.push((target[k], o));
            }
        }
    }
    if changes.is_empty() {
        let mut pl = Plan::default();
        pl.warnings.push("La matrice è già in questa situazione".into());
        return Ok(pl);
    }
    let mut pl = plan_routes(&changes, ctx)?;
    // intenzione = tutta la scena, non solo le differenze
    pl.intent = target
        .iter()
        .enumerate()
        .filter(|(k, &i)| i != 0 && !locked.contains(&(*k as u16 + 1)))
        .map(|(k, &i)| (i, k as u16 + 1))
        .collect();
    if !skipped.is_empty() {
        pl.warnings.push(format!("Uscite bloccate non ripristinate: {}", join(&skipped)));
    }
    Ok(pl)
}

/// Stato bersaglio di una scena dai suoi link: b←a; per bidir/kvm anche a←b.
pub fn scene_target(links: &[(u16, u16, LinkMode)], max: u16) -> Result<Outputs, String> {
    let mut t = vec![0u16; max as usize];
    for &(a, b, m) in links {
        check_port(a, max)?;
        check_port(b, max)?;
        t[b as usize - 1] = a;
        if m != LinkMode::Oneway {
            t[a as usize - 1] = b;
        }
    }
    Ok(t)
}

/// Unisce più piani (preset con più collegamenti) mantenendo l'ordine.
pub fn concat(plans: Vec<Plan>) -> Plan {
    let mut out = Plan::default();
    for p in plans {
        out.commands.extend(p.commands);
        out.intent.extend(p.intent);
        out.warnings.extend(p.warnings);
    }
    out
}

fn ports4(t: &str) -> Vec<u16> {
    t.as_bytes().chunks(4).filter_map(|c| std::str::from_utf8(c).ok()?.parse().ok()).collect()
}

/// Simulazione della semantica della matrice (API p.9–10; spec TT §3.3, §4.2):
/// CI additivo sull'ingresso, sostitutivo sull'uscita; DI spegne le uscite dell'ingresso;
/// DO spegne l'uscita; Macro: i=0000 → DO o, o=0000 → DI i, o=9999 → broadcast.
/// Non modella P2P/Restricted/Partition: per questo dopo l'invio si rilegge tutta la matrice.
pub fn simulate(cur: &Outputs, commands: &[String]) -> Outputs {
    let mut s = cur.clone();
    let n = s.len() as u16;
    for c in commands {
        if let Some(rest) = c.strip_prefix("CI") {
            let mut parts = rest.split('O');
            let Some(i) = parts.next().and_then(|x| x.parse::<u16>().ok()) else { continue };
            for o in parts.filter_map(|x| x.parse::<u16>().ok()) {
                if o == 9999 {
                    s.iter_mut().for_each(|v| *v = i);
                } else if (1..=n).contains(&o) {
                    s[o as usize - 1] = i;
                }
            }
        } else if let Some(rest) = c.strip_prefix("DI") {
            for i in ports4(rest) {
                s.iter_mut().filter(|v| i == 9999 || **v == i).for_each(|v| *v = 0);
            }
        } else if let Some(rest) = c.strip_prefix("DO") {
            for o in ports4(rest) {
                if o == 9999 {
                    s.iter_mut().for_each(|v| *v = 0);
                } else if (1..=n).contains(&o) {
                    s[o as usize - 1] = 0;
                }
            }
        } else if let Some(rest) = c.strip_prefix('M') {
            let p = ports4(rest.split(';').next().unwrap_or(""));
            for pair in p.chunks(2) {
                if let [i, o] = *pair {
                    match (i, o) {
                        (0, 9999) => s.iter_mut().for_each(|v| *v = 0),
                        (0, o) if (1..=n).contains(&o) => s[o as usize - 1] = 0,
                        (i, 0) => s.iter_mut().filter(|v| **v == i).for_each(|v| *v = 0),
                        (i, 9999) => s.iter_mut().for_each(|v| *v = i),
                        (i, o) if (1..=n).contains(&o) => s[o as usize - 1] = i,
                        _ => {}
                    }
                }
            }
        }
    }
    s
}

/// Uscite che cambiano fra due stati.
pub fn changed(a: &Outputs, b: &Outputs) -> Vec<u16> {
    (1..=a.len().min(b.len()) as u16).filter(|o| a[*o as usize - 1] != b[*o as usize - 1]).collect()
}

/// Uscite bloccate che il piano cambierebbe.
pub fn locked_hits(before: &Outputs, expected: &Outputs, locked: &[u16]) -> Vec<u16> {
    changed(before, expected).into_iter().filter(|o| locked.contains(o)).collect()
}

/// Coppie dell'intenzione che la sequenza stessa annulla (link successivi dello stesso preset).
pub fn broken_intent(intent: &[(u16, u16)], expected: &Outputs) -> Vec<(u16, u16)> {
    intent
        .iter()
        .copied()
        .filter(|(i, o)| expected.get(*o as usize - 1).copied().unwrap_or(0) != *i)
        .collect()
}

/// Confronto completo letto/atteso. Ritorna (descrizioni, uscite diverse).
pub fn verify(expected: &Outputs, read: &Outputs) -> Vec<String> {
    let fmt = |x: u16| if x == 0 { "nessuno".to_string() } else { format!("ingresso {x}") };
    changed(expected, read)
        .into_iter()
        .map(|o| {
            let k = o as usize - 1;
            format!("Uscita {o}: atteso {}, letto {}", fmt(expected[k]), fmt(read[k]))
        })
        .collect()
}

/// Avvisi di sicurezza: un ingresso di una porta RX (ritorno tastiera/mouse) che alimenta più
/// uscite significa una tastiera che comanda più PC (manuale API p.9, p.26).
/// I ruoli vengono dalle etichette e dai link «Postazione KVM» dei preset; senza ruoli
/// l'avviso non è possibile (TX con più spettatori e RX su più PC sono indistinguibili).
pub fn safety_warnings(cur: &Outputs, roles: &HashMap<u16, PortRole>) -> Vec<String> {
    let mut count: BTreeMap<u16, Vec<u16>> = BTreeMap::new();
    for (k, &i) in cur.iter().enumerate() {
        if i != 0 {
            count.entry(i).or_default().push(k as u16 + 1);
        }
    }
    count
        .into_iter()
        .filter(|(i, outs)| outs.len() > 1 && roles.get(i) == Some(&PortRole::Rx))
        .map(|(i, outs)| {
            format!(
                "Attenzione: la tastiera della porta {i} comanda più PC (uscite {})",
                join(&outs)
            )
        })
        .collect()
}

/// Piano di annulla: riporta le uscite `touched` allo stato `prev`.
pub fn plan_restore(prev: &Outputs, touched: &[u16], ctx: &Ctx) -> Result<Plan, String> {
    let changes: Vec<(u16, u16)> =
        touched.iter().map(|&o| (prev.get(o as usize - 1).copied().unwrap_or(0), o)).collect();
    plan_routes(&changes, ctx)
}

/// Stato corrente → link leggibili di una scena: coppie reciproche a fan 1 → bidir, il resto oneway.
pub fn routing_to_links(cur: &Outputs) -> Vec<(u16, u16, LinkMode)> {
    let mut used = vec![false; cur.len()];
    let mut out = vec![];
    for (k, &i) in cur.iter().enumerate() {
        let o = k as u16 + 1;
        if i == 0 || used[k] {
            continue;
        }
        let back = i != o && cur.get(i as usize - 1).copied().unwrap_or(0) == o;
        if back
            && outputs_of(cur, i).len() == 1
            && outputs_of(cur, o).len() == 1
            && !used[i as usize - 1]
        {
            used[k] = true;
            used[i as usize - 1] = true;
            out.push((i.min(o), i.max(o), LinkMode::Bidir));
        } else {
            used[k] = true;
            out.push((i, o, LinkMode::Oneway));
        }
    }
    out
}

pub fn join(v: &[u16]) -> String {
    v.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty(n: usize) -> Outputs {
        vec![0; n]
    }
    fn ctx(roles: &HashMap<u16, PortRole>, max: u16) -> Ctx<'_> {
        Ctx { max, roles, p2p: false }
    }

    #[test]
    fn bidir_sequence_matches_python_script() {
        let r = HashMap::new();
        let p = plan_link(1, 5, LinkMode::Bidir, BidirStrategy::Sequence, &ctx(&r, 24)).unwrap();
        assert_eq!(p.commands, vec!["DI0001", "DI0005", "CI0001O0005", "CI0005O0001"]);
    }

    #[test]
    fn bidir_macro_equivalent_and_exclusive() {
        let r = HashMap::new();
        let mut cur = empty(24);
        cur[6] = 1; // 1 -> 7 (altro monitor che guardava 1)
        cur[4] = 9; // 9 -> 5
        let seq = plan_link(1, 5, LinkMode::Bidir, BidirStrategy::Sequence, &ctx(&r, 24)).unwrap();
        let mac = plan_link(1, 5, LinkMode::Bidir, BidirStrategy::Macro, &ctx(&r, 24)).unwrap();
        assert_eq!(mac.commands, vec!["M00010000000500000001000500050001"]);
        let after = simulate(&cur, &seq.commands);
        assert_eq!(after, simulate(&cur, &mac.commands));
        assert_eq!((after[4], after[0], after[6]), (1, 5, 0));
        assert_eq!(
            changed(&cur, &after),
            vec![1, 5, 7],
            "l'uscita 7 è un effetto collaterale visibile"
        );
    }

    #[test]
    fn kvm_keeps_other_viewers_and_checks_roles() {
        let mut r = HashMap::new();
        let mut cur = empty(24);
        cur[6] = 1;
        let p = plan_link(1, 5, LinkMode::Kvm, BidirStrategy::Sequence, &ctx(&r, 24)).unwrap();
        assert_eq!(p.commands, vec!["DI0005", "CI0001O0005", "CI0005O0001"]);
        let after = simulate(&cur, &p.commands);
        assert_eq!((after[6], after[4], after[0]), (1, 1, 5));
        let m = plan_link(1, 5, LinkMode::Kvm, BidirStrategy::Macro, &ctx(&r, 24)).unwrap();
        assert_eq!(m.commands, vec!["M000500000001000500050001"]);
        assert_eq!(simulate(&cur, &m.commands), after);
        r.insert(1, PortRole::Rx);
        assert!(plan_link(1, 5, LinkMode::Kvm, BidirStrategy::Sequence, &ctx(&r, 24)).is_err());
    }

    #[test]
    fn link_validation() {
        let r = HashMap::new();
        let c = ctx(&r, 24);
        assert!(plan_link(3, 3, LinkMode::Bidir, BidirStrategy::Sequence, &c).is_err());
        assert!(plan_link(0, 3, LinkMode::Bidir, BidirStrategy::Sequence, &c).is_err());
        assert!(plan_link(1, 25, LinkMode::Oneway, BidirStrategy::Sequence, &c).is_err());
        // o←o è legittimo (lato R e lato T sono fibre diverse: manuale App. C, CI0010O0010)
        assert!(plan_link(10, 10, LinkMode::Oneway, BidirStrategy::Sequence, &c).is_ok());
    }

    #[test]
    fn locks_detected_through_collateral_di() {
        let r = HashMap::new();
        let mut cur = empty(24);
        cur[11] = 1; // 1 -> 12, uscita 12 bloccata
        let p = plan_link(1, 5, LinkMode::Bidir, BidirStrategy::Sequence, &ctx(&r, 24)).unwrap();
        let exp = simulate(&cur, &p.commands);
        assert_eq!(locked_hits(&cur, &exp, &[12]), vec![12]);
    }

    #[test]
    fn routes_grouped_and_p2p_split() {
        let r = HashMap::new();
        let p = plan_routes(&[(3, 10), (3, 11), (0, 4), (7, 2), (3, 12)], &ctx(&r, 24)).unwrap();
        assert_eq!(p.commands, vec!["DO0004", "CI0003O0010O0011O0012", "CI0007O0002"]);
        let s = simulate(&empty(24), &p.commands);
        assert_eq!(&s[9..12], &[3, 3, 3]);
        let c = Ctx { max: 24, roles: &r, p2p: true };
        let p = plan_routes(&[(3, 10), (3, 11)], &c).unwrap();
        assert_eq!(p.commands, vec!["CI0003O0010", "CI0003O0011"]);
    }

    #[test]
    fn disconnect_all_uses_do_and_skips_locks() {
        let mut cur = empty(8);
        cur[1] = 3;
        cur[4] = 1;
        let p = plan_disconnect_all(&cur, &[2]).unwrap();
        assert_eq!(p.commands, vec!["DO0005"]);
        assert_eq!(p.warnings.len(), 1);
        assert!(plan_disconnect_all(&empty(8), &[]).is_err());
        let big: Outputs = (1..=120).map(|_| 1).collect();
        assert_eq!(plan_disconnect_all(&big, &[]).unwrap().commands.len(), 3);
    }

    #[test]
    fn preset_conflict_detected() {
        let r = HashMap::new();
        let c = ctx(&r, 24);
        let a = plan_link(1, 7, LinkMode::Oneway, BidirStrategy::Sequence, &c).unwrap();
        let b = plan_link(1, 5, LinkMode::Bidir, BidirStrategy::Sequence, &c).unwrap();
        let pl = concat(vec![a, b]);
        let exp = simulate(&empty(24), &pl.commands);
        assert_eq!(broken_intent(&pl.intent, &exp), vec![(1, 7)]);
    }

    #[test]
    fn scene_roundtrip_exact() {
        let r = HashMap::new();
        let mut snap = empty(12);
        snap[4] = 1;
        snap[0] = 5; // 1↔5
        snap[6] = 2;
        snap[7] = 2; // 2 → 7, 8 (multicast)
        snap[9] = 10; // 10 ← 10 (loopback legittimo)
        let links = routing_to_links(&snap);
        let target = scene_target(&links, 12).unwrap();
        assert_eq!(target, snap);
        let mut cur = empty(12);
        cur[2] = 9;
        cur[5] = 1; // 1 -> 6 spettatore aggiunto dopo
        let pl = plan_scene(&target, &cur, &[], &ctx(&r, 12)).unwrap();
        assert_eq!(simulate(&cur, &pl.commands), snap);
        // con uscita 3 bloccata resta com'è
        let pl = plan_scene(&target, &cur, &[3], &ctx(&r, 12)).unwrap();
        assert_eq!(simulate(&cur, &pl.commands)[2], 9);
        assert!(plan_scene(&target, &empty(24), &[], &ctx(&r, 24)).is_err());
    }

    #[test]
    fn restore_and_verify() {
        let r = HashMap::new();
        let mut prev = empty(8);
        prev[4] = 1;
        prev[0] = 5;
        prev[6] = 1;
        let mut now = empty(8);
        now[4] = 2;
        let touched = changed(&prev, &now);
        let p = plan_restore(&prev, &touched, &ctx(&r, 8)).unwrap();
        assert_eq!(simulate(&now, &p.commands), prev);
        assert!(verify(&prev, &prev).is_empty());
        assert_eq!(verify(&prev, &now).len(), 3);
    }

    #[test]
    fn warnings_keyboard_on_many_pcs() {
        let mut cur = empty(8);
        cur[0] = 5;
        cur[3] = 5;
        let mut roles = HashMap::new();
        roles.insert(5, PortRole::Rx);
        assert_eq!(safety_warnings(&cur, &roles).len(), 1);
        roles.insert(5, PortRole::Tx);
        assert!(safety_warnings(&cur, &roles).is_empty());
    }
}
