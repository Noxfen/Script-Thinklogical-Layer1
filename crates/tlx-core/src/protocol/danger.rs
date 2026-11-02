//! Riconoscimento dei comandi pericolosi (spec §3.1, §3.7): da eseguire solo dietro
//! conferma esplicita dell'operatore.

/// Se il comando è pericoloso restituisce il motivo (in italiano), altrimenti `None`.
///
/// Pericolosi: `XRESET`, `XTEST*`, `XPUT` (con o senza argomenti), `XHELP`, ogni forma con
/// `9999` dei comandi di commutazione/stato (`CI…O9999`, `DI9999`, `DO9999`, `SI9999`,
/// `SO9999`, `M…9999`, `M00009999`) e il `;` finale del Macro.
pub fn is_dangerous(cmd: &str) -> Option<&'static str> {
    let c = cmd.trim().to_ascii_uppercase();
    if c.starts_with("XRESET") {
        return Some("XRESET riporta l'hardware del crosspoint allo stato di accensione");
    }
    if c.starts_with("XTEST") {
        return Some("XTEST riconfigura l'intera matrice collegando tutte le porte");
    }
    if c.starts_with("XPUT") {
        return Some("XPUT spegne tutte le uscite prima di applicare il nuovo stato");
    }
    if c.starts_with("XHELP") {
        return Some("XHELP risponde su più righe senza R/OK e compromette la sessione");
    }
    if let Some(body) = c.strip_prefix('M') {
        if body.contains(';') {
            return Some("il ';' finale del Macro non è ammesso dalla logica di scenario");
        }
        let digits: Vec<u8> = body.bytes().filter(|b| b.is_ascii_digit()).collect();
        if digits.chunks(4).any(|ch| ch == b"9999") {
            return Some("Macro con 9999: agisce su tutte le porte");
        }
        return None;
    }
    let is_port_cmd = ["CI", "DI", "DO", "SI", "SO"].iter().any(|p| c.starts_with(p));
    if is_port_cmd {
        // Ogni gruppo di cifre fra lettere, a passi di 4.
        let has_all = c[2..]
            .split(|ch: char| !ch.is_ascii_digit())
            .any(|run| run.as_bytes().chunks(4).any(|ch| ch == b"9999"));
        if has_all {
            return Some(match &c[..2] {
                "CI" => "CI con 9999: collega l'ingresso a tutte le uscite",
                "DI" => "DI9999: scollega tutti gli ingressi",
                "DO" => "DO9999: scollega tutte le uscite",
                _ => "9999 (tutte le porte) non ammesso in questo comando",
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pericolosi() {
        for c in [
            "XRESET",
            "xreset",
            "XTEST1",
            "XTEST2",
            "XTEST3",
            "XPUT",
            "XPUTI00010003",
            "xput",
            "XHELP",
            "CI0001O9999",
            "CI0001O0002O9999",
            "ci0001o9999",
            "DI9999",
            "DO9999",
            "DI00019999",
            "SI9999",
            "SO9999",
            "M00009999",
            "M00019999",
            "M00010002;",
            "M;",
            "M999900010",
        ] {
            assert!(is_dangerous(c).is_some(), "{c} dovrebbe essere pericoloso");
        }
    }

    #[test]
    fn non_pericolosi() {
        for c in [
            "CI0001O0002",
            "DI0001",
            "DO0005",
            "SI0004",
            "SO0004",
            "M001500000025000000250014",
            "XGET",
            "XSTATUSIO",
            "XLOS9999",
            "XRXPWR9999",
            "XTXPWR9999",
            "XVERSION",
            "XLASTEVENT",
            "XQUIT",
            "XCRON",
            "CI0999O0001",
            "M00990999",
        ] {
            assert!(is_dangerous(c).is_none(), "{c} non dovrebbe essere pericoloso");
        }
    }
}
