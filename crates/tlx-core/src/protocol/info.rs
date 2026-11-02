//! Comandi informativi e diagnostici: `XVERSION`, `XTYPE`, `XMAXPORTS`, `XMAXCARDS`,
//! `XINSTALLED`, `XPORTCONFIG`, `XOPTIONS`, `XALARM`, `XLOS`, `XRXPWR`/`XTXPWR`,
//! `XLASTEVENT`, `XCTIME` (spec §3.4–3.6).
//!
//! Tutte le funzioni `parse_*` ricevono i **dati** della risposta (fra `OK` e `#`).

use serde::{Deserialize, Serialize};

use super::response::ParseError;

fn invalid(msg: impl Into<String>) -> ParseError {
    ParseError::InvalidData(msg.into())
}

/// Profilo della matrice ricavato dall'handshake.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatrixInfo {
    /// Versione del programma API (`XVERSION`, es. `V5.07.07`).
    pub version: String,
    /// Modello (`XTYPE`, es. `TLX48Switch`); vuoto se il comando non è supportato.
    pub xtype: String,
    /// N (`XMAXPORTS`): base di ogni validazione.
    pub max_ports: u16,
    /// Numero di slot I/O (`XMAXCARDS`); 0 se non letto.
    pub max_cards: u16,
    /// Schede presenti per slot (`XINSTALLED`), da sinistra la scheda della porta 1.
    pub installed: Vec<bool>,
    /// Bitmap grezza di `XOPTIONS`.
    pub options: u16,
    /// Bit 2: la matrice esige comandi in maiuscolo.
    pub upper_only: bool,
    /// Bit 3: `XMAXCARDS` restituisce il numero reale di schede (0 = matrice VX o API V4).
    pub real_card_count: bool,
    /// Bit 4: supporta la modalità P2P.
    pub p2p: bool,
    /// Bit 5: lo stato può essere inviato in multicast.
    pub mcast_status: bool,
    /// Bit 6: supporta `XCTIME`.
    pub xctime: bool,
    /// Comandi Rev. M (`XLOS`, `XRXPWR`, `XTXPWR`) disponibili; `None` = non ancora provato.
    pub los_supported: Option<bool>,
}

impl MatrixInfo {
    /// Applica i bit di `XOPTIONS` ai campi booleani.
    pub fn set_options(&mut self, options: u16) {
        let o = decode_options(options);
        self.options = options;
        self.upper_only = o.upper_only;
        self.real_card_count = o.real_card_count;
        self.p2p = o.p2p;
        self.mcast_status = o.mcast_status;
        self.xctime = o.xctime;
    }
}

/// Bit decodificati di `XOPTIONS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OptionsBits {
    /// Bit 1: `XTEST2` ammesso (TLX320/640).
    pub xtest2: bool,
    /// Bit 2: solo maiuscole.
    pub upper_only: bool,
    /// Bit 3: `XMAXCARDS` reale.
    pub real_card_count: bool,
    /// Bit 4: P2P.
    pub p2p: bool,
    /// Bit 5: multicast per lo stato.
    pub mcast_status: bool,
    /// Bit 6: `XCTIME`.
    pub xctime: bool,
}

/// Decodifica la bitmap di `XOPTIONS` (bit 0 = lsb).
pub fn decode_options(v: u16) -> OptionsBits {
    let b = |n: u16| v & (1 << n) != 0;
    OptionsBits {
        xtest2: b(1),
        upper_only: b(2),
        real_card_count: b(3),
        p2p: b(4),
        mcast_status: b(5),
        xctime: b(6),
    }
}

/// Numero decimale (tipicamente a 4 cifre) di `XMAXPORTS`, `XMAXCARDS`, `XOPTIONS`, `XALARM`.
pub fn parse_number(data: &str) -> Result<u16, ParseError> {
    let d = data.trim();
    if d.is_empty() || d.len() > 5 || !d.bytes().all(|b| b.is_ascii_digit()) {
        return Err(invalid(format!("numero atteso, ricevuto '{d}'")));
    }
    d.parse::<u16>().map_err(|_| invalid(format!("numero fuori intervallo: '{d}'")))
}

/// `XVERSION` → stringa versione (es. `V5.00`).
pub fn parse_xversion(data: &str) -> String {
    data.trim().to_string()
}

/// `XTYPE` → modello (es. `TLX48Switch`).
pub fn parse_xtype(data: &str) -> String {
    data.trim().to_string()
}

/// `XMAXPORTS` → N (1..=9998).
pub fn parse_xmaxports(data: &str) -> Result<u16, ParseError> {
    let n = parse_number(data)?;
    if n == 0 || n >= 9999 {
        return Err(invalid(format!("XMAXPORTS non plausibile: {n}")));
    }
    Ok(n)
}

/// `XMAXCARDS` → numero di slot.
pub fn parse_xmaxcards(data: &str) -> Result<u16, ParseError> {
    parse_number(data)
}

/// `XOPTIONS` → bitmap.
pub fn parse_xoptions(data: &str) -> Result<u16, ParseError> {
    parse_number(data)
}

/// `XALARM` → bitmap degli allarmi (0 = nessun allarme).
pub fn parse_xalarm(data: &str) -> Result<u16, ParseError> {
    parse_number(data)
}

/// `XINSTALLED` → una cifra per slot (`1` = scheda presente).
pub fn parse_xinstalled(data: &str) -> Result<Vec<bool>, ParseError> {
    let d = data.trim();
    if d.is_empty() {
        return Err(invalid("XINSTALLED vuoto"));
    }
    d.chars()
        .map(|c| match c {
            '1' => Ok(true),
            '0' => Ok(false),
            _ => Err(invalid(format!("XINSTALLED: carattere inatteso '{c}'"))),
        })
        .collect()
}

/// Risposta di `XPORTCONFIG`: `NU,NB,0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortConfig {
    /// Percorsi unidirezionali.
    pub nu: u16,
    /// Percorsi bidirezionali (sempre 0 sulle TLX).
    pub nb: u16,
    /// Terzo campo (definito come 0).
    pub off: u16,
}

/// `XPORTCONFIG` → `NU,NB,0`.
pub fn parse_xportconfig(data: &str) -> Result<PortConfig, ParseError> {
    let parts: Vec<&str> = data.trim().split(',').collect();
    if parts.len() != 3 {
        return Err(invalid(format!("XPORTCONFIG: attesi 3 campi, ricevuto '{data}'")));
    }
    Ok(PortConfig {
        nu: parse_number(parts[0])?,
        nb: parse_number(parts[1])?,
        off: parse_number(parts[2])?,
    })
}

/// `XLOS` → per porta, `true` = LOS (nessuna luce in ingresso).
/// `expected` = numero di porte atteso (per `XLOS9999` = N), verificato se presente.
pub fn parse_xlos(data: &str, expected: Option<usize>) -> Result<Vec<bool>, ParseError> {
    let d = data.trim();
    let body = d
        .strip_prefix('L')
        .ok_or_else(|| invalid(format!("XLOS: atteso 'L' all'inizio di '{d}'")))?;
    let v: Vec<bool> = body
        .chars()
        .map(|c| match c {
            '1' => Ok(true),
            '0' => Ok(false),
            _ => Err(invalid(format!("XLOS: carattere inatteso '{c}'"))),
        })
        .collect::<Result<_, _>>()?;
    if let Some(n) = expected {
        if v.len() != n {
            return Err(invalid(format!("XLOS: attese {n} porte, ricevute {}", v.len())));
        }
    }
    Ok(v)
}

fn parse_pwr(data: &str, letter: char, expected: Option<usize>) -> Result<Vec<u16>, ParseError> {
    let d = data.trim();
    let body = d
        .strip_prefix(letter)
        .ok_or_else(|| invalid(format!("atteso '{letter}' all'inizio di '{d}'")))?;
    if body.len() % 4 != 0 || !body.bytes().all(|b| b.is_ascii_digit()) {
        return Err(invalid(format!("potenze non a gruppi di 4 cifre: '{d}'")));
    }
    let v: Vec<u16> = body
        .as_bytes()
        .chunks(4)
        .map(|c| c.iter().fold(0u16, |a, d| a * 10 + (d - b'0') as u16))
        .collect();
    if let Some(n) = expected {
        if v.len() != n {
            return Err(invalid(format!("attese {n} porte, ricevute {}", v.len())));
        }
    }
    Ok(v)
}

/// `XRXPWR` → potenza ricevuta per porta in µW (0 = SFP assente o senza diagnostica).
pub fn parse_xrxpwr(data: &str, expected: Option<usize>) -> Result<Vec<u16>, ParseError> {
    parse_pwr(data, 'R', expected)
}

/// `XTXPWR` → potenza trasmessa per porta in µW.
pub fn parse_xtxpwr(data: &str, expected: Option<usize>) -> Result<Vec<u16>, ParseError> {
    parse_pwr(data, 'T', expected)
}

/// `XLASTEVENT` → stringa opaca (timestamp testuale): solo un indizio di variazione.
pub fn parse_xlastevent(data: &str) -> String {
    data.to_string()
}

/// Risposta di `XCTIMEnnnn`: `pppp,tI,tO`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct XcTime {
    /// Porta.
    pub port: u16,
    /// Tempo cumulato lato ingresso (s, o giorni con `--xctime-day`).
    pub input_time: u64,
    /// Tempo cumulato lato uscita.
    pub output_time: u64,
}

/// `XCTIME` → porta e tempi cumulativi.
pub fn parse_xctime(data: &str) -> Result<XcTime, ParseError> {
    let parts: Vec<&str> = data.trim().split(',').collect();
    if parts.len() != 3 {
        return Err(invalid(format!("XCTIME: attesi 3 campi, ricevuto '{data}'")));
    }
    let n = |s: &str| s.trim().parse::<u64>().map_err(|_| invalid(format!("XCTIME: '{s}'")));
    Ok(XcTime {
        port: parse_number(parts[0])?,
        input_time: n(parts[1])?,
        output_time: n(parts[2])?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeri() {
        assert_eq!(parse_xmaxports("0048").unwrap(), 48);
        assert_eq!(parse_xmaxports("0320").unwrap(), 320);
        assert_eq!(parse_xmaxports("0640").unwrap(), 640);
        assert!(parse_xmaxports("0000").is_err());
        assert!(parse_xmaxports("abc").is_err());
        assert_eq!(parse_xmaxcards("0003").unwrap(), 3);
        assert_eq!(parse_xalarm("0522").unwrap(), 522);
        assert_eq!(parse_xoptions("0007").unwrap(), 7);
        assert!(parse_number("").is_err());
    }

    #[test]
    fn opzioni() {
        let o = decode_options(120);
        assert!(!o.xtest2 && !o.upper_only);
        assert!(o.real_card_count && o.p2p && o.mcast_status && o.xctime);
        let o = decode_options(7);
        assert!(o.xtest2 && o.upper_only && !o.real_card_count);
        let mut i = MatrixInfo::default();
        i.set_options(0x0C);
        assert!(i.upper_only && i.real_card_count && !i.p2p);
        assert_eq!(i.options, 12);
    }

    #[test]
    fn stringhe_e_installed() {
        assert_eq!(parse_xversion("V5.00"), "V5.00");
        assert_eq!(parse_xtype("TLX48Switch"), "TLX48Switch");
        assert_eq!(parse_xinstalled("111").unwrap(), vec![true; 3]);
        let v = parse_xinstalled("00000000000100000000").unwrap();
        assert_eq!(v.len(), 20);
        assert_eq!(v.iter().filter(|b| **b).count(), 1);
        assert!(v[11]);
        assert!(parse_xinstalled("12").is_err());
        assert_eq!(parse_xlastevent("Thu Jul  1 11:23:52 2010"), "Thu Jul  1 11:23:52 2010");
    }

    #[test]
    fn portconfig() {
        assert_eq!(
            parse_xportconfig("0048,0000,0000").unwrap(),
            PortConfig { nu: 48, nb: 0, off: 0 }
        );
        assert!(parse_xportconfig("0048,0000").is_err());
    }

    #[test]
    fn los_e_potenze() {
        assert_eq!(parse_xlos("L0", Some(1)).unwrap(), vec![false]);
        let v = parse_xlos("L111111111111101111111111", Some(24)).unwrap();
        assert_eq!(v.len(), 24);
        assert!(!v[13]);
        assert_eq!(v.iter().filter(|b| !**b).count(), 1);
        assert!(parse_xlos("L1110", Some(24)).is_err());
        assert!(parse_xlos("X0", None).is_err());

        assert_eq!(parse_xtxpwr("T0451", None).unwrap(), vec![451]);
        assert_eq!(parse_xrxpwr("R0451", None).unwrap(), vec![451]);
        let mut s = String::from("T");
        for p in 1..=24 {
            s.push_str(if p == 14 { "0451" } else { "0000" });
        }
        let v = parse_xtxpwr(&s, Some(24)).unwrap();
        assert_eq!(v[13], 451);
        assert_eq!(v.iter().filter(|x| **x == 0).count(), 23);
        assert!(parse_xrxpwr(&s, None).is_err());
        assert!(parse_xtxpwr("T045", None).is_err());
    }

    #[test]
    fn xctime() {
        let t = parse_xctime("0001,612817,612817").unwrap();
        assert_eq!(t, XcTime { port: 1, input_time: 612817, output_time: 612817 });
    }
}
