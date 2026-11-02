//! Costruzione e decodifica dei comandi ASCII TLX (spec §3.1, §3.3–3.6).
//!
//! Due livelli:
//! * **builder lato client** (`ci`, `di`, `do_`, `macro_`, …): applicano le regole TT
//!   (§3.1) e rifiutano tutto ciò che la logica di scenario non deve mai inviare
//!   (porta 0, porta > N, loopback, `9999` mescolato ad altre porte, coppia Macro `00000000`…).
//!   Il `9999` è consentito solo tramite le funzioni esplicite [`di_all`] e [`do_all`].
//! * **decodificatore lato matrice** ([`parse_command`]): interpreta una riga come farebbe la
//!   matrice, con i codici d'errore del costruttore (§3.7). È usato dal simulatore e dal
//!   client per applicare localmente le commutazioni riuscite.

use super::routing::Routing;

/// Valore «tutte le porte».
pub const ALL_PORTS: u16 = 9999;
/// Numero massimo di coppie per blocco Macro (politica TT, spec §3.3).
pub const MACRO_MAX_PAIRS: usize = 50;

/// Comandi informativi senza argomenti.
pub const XVERSION: &str = "XVERSION";
/// Modello (`TLX48Switch`…).
pub const XTYPE: &str = "XTYPE";
/// Vecchio nome di `XTYPE` (API V4).
pub const XDCSTYPE: &str = "XDCSTYPE";
/// Numero di porte N.
pub const XMAXPORTS: &str = "XMAXPORTS";
/// Numero di slot I/O.
pub const XMAXCARDS: &str = "XMAXCARDS";
/// Inventario schede.
pub const XINSTALLED: &str = "XINSTALLED";
/// `NU,NB,0`.
pub const XPORTCONFIG: &str = "XPORTCONFIG";
/// Bitmap delle opzioni.
pub const XOPTIONS: &str = "XOPTIONS";
/// Bitmap degli allarmi.
pub const XALARM: &str = "XALARM";
/// Timestamp dell'ultimo make/break.
pub const XLASTEVENT: &str = "XLASTEVENT";
/// Stato completo a gruppi (formato di `XPUT`).
pub const XGET: &str = "XGET";
/// Stato completo a coppie ingresso→uscita.
pub const XSTATUSIO: &str = "XSTATUSIO";
/// Stato completo a coppie uscita←ingresso.
pub const XSTATUSOI: &str = "XSTATUSOI";
/// LOS di tutte le porte (Rev. M).
pub const XLOS_ALL: &str = "XLOS9999";
/// Potenza RX di tutte le porte in µW (Rev. M).
pub const XRXPWR_ALL: &str = "XRXPWR9999";
/// Potenza TX di tutte le porte in µW (Rev. M).
pub const XTXPWR_ALL: &str = "XTXPWR9999";
/// Chiusura ordinata della sessione TCP.
pub const XQUIT: &str = "XQUIT";
/// `CR` in coda alle risposte.
pub const XCRON: &str = "XCRON";
/// Niente `CR` in coda alle risposte.
pub const XCROFF: &str = "XCROFF";

/// Errore di validazione di un comando lato client (regole TT, spec §3.1).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CmdError {
    /// Nessuna porta indicata.
    #[error("nessuna porta indicata")]
    Empty,
    /// Porta `0000`.
    #[error("la porta 0 non è valida in un comando")]
    PortZero,
    /// Porta oltre `XMAXPORTS`.
    #[error("porta {port} fuori intervallo (1–{max})")]
    OutOfRange {
        /// Porta richiesta.
        port: u16,
        /// N della matrice.
        max: u16,
    },
    /// `9999` usato fuori da `di_all()` / `do_all()`.
    #[error("il valore 9999 (tutte le porte) non è ammesso qui: usare la funzione esplicita")]
    AllPortsNotAllowed,
    /// `CIxOx`.
    #[error("loopback non ammesso: ingresso e uscita {0} coincidono")]
    Loopback(u16),
    /// Porta ripetuta nello stesso comando.
    #[error("porta {0} ripetuta nello stesso comando")]
    Duplicate(u16),
    /// Coppia Macro `00000000`.
    #[error("coppia Macro 0000/0000 non ammessa")]
    MacroPairZero,
    /// `N` della matrice non noto (handshake non eseguito).
    #[error("numero di porte della matrice non noto")]
    UnknownMaxPorts,
    /// Stringa Macro non decodificabile.
    #[error("Macro non valida: {0}")]
    InvalidMacro(String),
}

fn check_port(max: u16, p: u16) -> Result<(), CmdError> {
    if max == 0 {
        return Err(CmdError::UnknownMaxPorts);
    }
    if p == ALL_PORTS {
        return Err(CmdError::AllPortsNotAllowed);
    }
    if p == 0 {
        return Err(CmdError::PortZero);
    }
    if p > max {
        return Err(CmdError::OutOfRange { port: p, max });
    }
    Ok(())
}

fn check_ports(max: u16, ports: &[u16]) -> Result<(), CmdError> {
    if ports.is_empty() {
        return Err(CmdError::Empty);
    }
    for (k, &p) in ports.iter().enumerate() {
        check_port(max, p)?;
        if ports[..k].contains(&p) {
            return Err(CmdError::Duplicate(p));
        }
    }
    Ok(())
}

/// Formatta una porta su 4 cifre.
pub fn port4(p: u16) -> String {
    format!("{p:04}")
}

/// `CIxxxxOyyyy[Oyyyy…]`: collega l'ingresso a una o più uscite (additivo sull'ingresso,
/// sostitutivo sull'uscita). Vieta loopback, porta 0, porta > `max`, 9999, duplicati.
pub fn ci(max: u16, input: u16, outputs: &[u16]) -> Result<String, CmdError> {
    check_port(max, input)?;
    check_ports(max, outputs)?;
    if outputs.contains(&input) {
        return Err(CmdError::Loopback(input));
    }
    let mut s = format!("CI{}", port4(input));
    for &o in outputs {
        s.push('O');
        s.push_str(&port4(o));
    }
    Ok(s)
}

/// `DIxxxx[xxxx…]`: scollega tutte le uscite degli ingressi indicati.
pub fn di(max: u16, inputs: &[u16]) -> Result<String, CmdError> {
    check_ports(max, inputs)?;
    Ok(format!("DI{}", inputs.iter().map(|&p| port4(p)).collect::<String>()))
}

/// `DOyyyy[yyyy…]`: scollega le uscite indicate.
pub fn do_(max: u16, outputs: &[u16]) -> Result<String, CmdError> {
    check_ports(max, outputs)?;
    Ok(format!("DO{}", outputs.iter().map(|&p| port4(p)).collect::<String>()))
}

/// `DI9999`: scollega tutto (comando pericoloso, solo dietro conferma).
pub fn di_all() -> String {
    "DI9999".to_string()
}

/// `DO9999`: scollega tutte le uscite (comando pericoloso, solo dietro conferma).
pub fn do_all() -> String {
    "DO9999".to_string()
}

/// `SIxxxx`: uscite collegate all'ingresso.
pub fn si(max: u16, input: u16) -> Result<String, CmdError> {
    check_port(max, input)?;
    Ok(format!("SI{}", port4(input)))
}

/// `SOyyyy`: ingresso collegato all'uscita.
pub fn so(max: u16, output: u16) -> Result<String, CmdError> {
    check_port(max, output)?;
    Ok(format!("SO{}", port4(output)))
}

/// `XCTIMEnnnn`: tempi cumulativi di connessione della porta.
pub fn xctime(max: u16, port: u16) -> Result<String, CmdError> {
    check_port(max, port)?;
    Ok(format!("XCTIME{}", port4(port)))
}

/// `XPUT` + stringa `XGET` della routing indicata (comando pericoloso: spegne tutto prima).
pub fn xput(routing: &Routing) -> String {
    format!("XPUT{}", routing.to_xget())
}

fn check_macro_pairs(max: u16, pairs: &[(u16, u16)]) -> Result<(), CmdError> {
    if pairs.is_empty() {
        return Err(CmdError::Empty);
    }
    if max == 0 {
        return Err(CmdError::UnknownMaxPorts);
    }
    for &(i, o) in pairs {
        match (i, o) {
            (0, 0) => return Err(CmdError::MacroPairZero),
            (0, o) => check_port(max, o)?,
            (i, 0) => check_port(max, i)?,
            (i, o) => {
                check_port(max, i)?;
                check_port(max, o)?;
                if i == o {
                    return Err(CmdError::Loopback(i));
                }
            }
        }
    }
    Ok(())
}

/// `Miiiioooo…`: Macro (senza `;` finale). Coppie `(ingresso, uscita)`:
/// `(0, o)` = `DO o`, `(i, 0)` = `DI i`, `(i, o)` = `CI i O o`. Il 9999 non è ammesso.
/// Non spezza in blocchi: vedi [`macro_chunks`].
pub fn macro_(max: u16, pairs: &[(u16, u16)]) -> Result<String, CmdError> {
    check_macro_pairs(max, pairs)?;
    let mut s = String::with_capacity(1 + pairs.len() * 8);
    s.push('M');
    for &(i, o) in pairs {
        s.push_str(&port4(i));
        s.push_str(&port4(o));
    }
    Ok(s)
}

/// Come [`macro_`] ma spezza in blocchi di al massimo [`MACRO_MAX_PAIRS`] coppie,
/// **senza riordinare** (spec §5.3).
pub fn macro_chunks(max: u16, pairs: &[(u16, u16)]) -> Result<Vec<String>, CmdError> {
    check_macro_pairs(max, pairs)?;
    pairs.chunks(MACRO_MAX_PAIRS).map(|c| macro_(max, c)).collect()
}

/// Operazione elementare di un Macro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacroOp {
    /// `iiiioooo`: collega l'ingresso all'uscita.
    Connect {
        /// Ingresso.
        input: u16,
        /// Uscita.
        output: u16,
    },
    /// `iiii0000`: scollega l'ingresso da tutto (come `DI`).
    DisconnectInput(u16),
    /// `0000oooo`: scollega l'uscita (come `DO`).
    DisconnectOutput(u16),
    /// `iiii9999`: ingresso su tutte le uscite.
    Broadcast(u16),
    /// `00009999`: scollega tutto.
    DisconnectAll,
    /// `00000000`: nessun effetto (vietato dai builder TT).
    Nop,
}

impl MacroOp {
    /// Interpreta una coppia `(ingresso, uscita)` del Macro.
    pub fn from_pair(i: u16, o: u16) -> MacroOp {
        match (i, o) {
            (0, 0) => MacroOp::Nop,
            (0, ALL_PORTS) => MacroOp::DisconnectAll,
            (0, o) => MacroOp::DisconnectOutput(o),
            (i, 0) => MacroOp::DisconnectInput(i),
            (i, ALL_PORTS) => MacroOp::Broadcast(i),
            (i, o) => MacroOp::Connect { input: i, output: o },
        }
    }
}

/// Macro decodificato.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedMacro {
    /// Coppie nell'ordine scritto.
    pub pairs: Vec<(u16, u16)>,
    /// Operazioni corrispondenti.
    pub ops: Vec<MacroOp>,
    /// `true` se termina con `;`.
    pub semicolon: bool,
}

/// Decodifica una stringa Macro (`M…[;]`), senza controlli d'intervallo.
pub fn decode_macro(cmd: &str) -> Result<DecodedMacro, CmdError> {
    let c = cmd.trim().to_ascii_uppercase();
    let body =
        c.strip_prefix('M').ok_or_else(|| CmdError::InvalidMacro("non inizia con M".into()))?;
    let (digits, semicolon) = match body.find(';') {
        Some(i) => {
            if !body[i + 1..].is_empty() {
                return Err(CmdError::InvalidMacro("porte dopo il ';'".into()));
            }
            (&body[..i], true)
        }
        None => (body, false),
    };
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(CmdError::InvalidMacro("caratteri non numerici".into()));
    }
    if digits.len() % 8 != 0 {
        return Err(CmdError::InvalidMacro("lunghezza non multipla di 8 cifre".into()));
    }
    let pairs: Vec<(u16, u16)> =
        digits.as_bytes().chunks(8).map(|ch| (num4(&ch[..4]), num4(&ch[4..]))).collect();
    let ops = pairs.iter().map(|&(i, o)| MacroOp::from_pair(i, o)).collect();
    Ok(DecodedMacro { pairs, ops, semicolon })
}

fn num4(b: &[u8]) -> u16 {
    b.iter().fold(0u16, |acc, d| acc * 10 + (d - b'0') as u16)
}

// ---------------------------------------------------------------------------
// Decodifica lato matrice
// ---------------------------------------------------------------------------

/// Errore «come la matrice»: codice `ERnnnn` e testo verbose (in inglese, come il firmware).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixError {
    /// Codice numerico (es. 7 → `ER0007`).
    pub code: u16,
    /// Testo per il commento verbose.
    pub detail: String,
}

impl MatrixError {
    fn new(code: u16, detail: impl Into<String>) -> Self {
        MatrixError { code, detail: detail.into() }
    }
}

/// Un comando interpretato come lo interpreterebbe la matrice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedCommand {
    /// `CI`: ingresso e uscite (può contenere 9999 = tutte).
    Connect {
        /// Ingresso.
        input: u16,
        /// Uscite nell'ordine scritto.
        outputs: Vec<u16>,
    },
    /// `DI` (9999 = tutti).
    DisconnectInputs(Vec<u16>),
    /// `DO` (9999 = tutte).
    DisconnectOutputs(Vec<u16>),
    /// `M` (vuoto = test di supporto).
    Macro {
        /// Coppie.
        pairs: Vec<(u16, u16)>,
        /// `;` finale.
        semicolon: bool,
    },
    /// `XPUT` con lo stato da applicare dopo aver spento tutto.
    Put(Routing),
    /// `SIxxxx`.
    StatusIn(u16),
    /// `SOyyyy`.
    StatusOut(u16),
    /// `XGET`.
    Xget,
    /// `XSTATUSIO`.
    XstatusIo,
    /// `XSTATUSOI`.
    XstatusOi,
    /// `XVERSION`.
    Xversion,
    /// `XTYPE` / `XDCSTYPE`.
    Xtype,
    /// `XMAXPORTS`.
    Xmaxports,
    /// `XMAXCARDS`.
    Xmaxcards,
    /// `XINSTALLED`.
    Xinstalled,
    /// `XPORTCONFIG`.
    Xportconfig,
    /// `XOPTIONS`.
    Xoptions,
    /// `XALARM`.
    Xalarm,
    /// `XLASTEVENT`.
    Xlastevent,
    /// `XCTIMEnnnn`.
    Xctime(u16),
    /// `XLOSnnnn`.
    Xlos(u16),
    /// `XGETLOSnnnn`.
    Xgetlos(u16),
    /// `XRXPWRnnnn`.
    Xrxpwr(u16),
    /// `XTXPWRnnnn`.
    Xtxpwr(u16),
    /// `XCRON`.
    Xcron,
    /// `XCROFF`.
    Xcroff,
    /// `XQUIT`.
    Xquit,
    /// `XRESET`.
    Xreset,
    /// `XTEST1/2/3`.
    Xtest(u8),
    /// `XHELP`.
    Xhelp,
}

impl ParsedCommand {
    /// `true` se il comando modifica gli incroci.
    pub fn is_switching(&self) -> bool {
        matches!(
            self,
            ParsedCommand::Connect { .. }
                | ParsedCommand::DisconnectInputs(_)
                | ParsedCommand::DisconnectOutputs(_)
                | ParsedCommand::Macro { .. }
                | ParsedCommand::Put(_)
                | ParsedCommand::Xreset
                | ParsedCommand::Xtest(_)
        )
    }

    /// `true` per i comandi di stato con risposta proporzionale a N.
    pub fn is_bulk_status(&self) -> bool {
        matches!(self, ParsedCommand::Xget | ParsedCommand::XstatusIo | ParsedCommand::XstatusOi)
            || matches!(
                self,
                ParsedCommand::Xlos(ALL_PORTS)
                    | ParsedCommand::Xgetlos(ALL_PORTS)
                    | ParsedCommand::Xrxpwr(ALL_PORTS)
                    | ParsedCommand::Xtxpwr(ALL_PORTS)
            )
    }
}

/// Prefisso di un comando `X` con argomento numerico e costruttore del comando.
type NumCmd = (&'static str, fn(u16) -> ParsedCommand);

fn out_of_range(kind: &str, p: u16, max: u16) -> String {
    format!("{kind} port number {p} is out of range (1 thru {max})")
}

/// Estrae le porte a 4 cifre di una stringa di sole cifre.
fn digits_to_ports(s: &str) -> Result<Vec<u16>, MatrixError> {
    if let Some(c) = s.chars().find(|c| !c.is_ascii_digit()) {
        let code = if c.is_ascii_alphabetic() { 1 } else { 8 };
        return Err(MatrixError::new(code, format!("Invalid character '{c}'")));
    }
    if !s.len().is_multiple_of(4) {
        return Err(MatrixError::new(1, "Port numbers must be 4 digits"));
    }
    Ok(s.as_bytes().chunks(4).map(num4).collect())
}

fn single_port_arg(rest: &str) -> Result<u16, MatrixError> {
    if rest.len() != 4 || !rest.bytes().all(|b| b.is_ascii_digit()) {
        return Err(MatrixError::new(1, "Syntax error: expected a 4 digit port number"));
    }
    Ok(num4(rest.as_bytes()))
}

/// Interpreta una riga di comando come la matrice (maiuscole/minuscole indifferenti).
/// `max` = N della matrice. Gli errori riportano i codici del costruttore (§3.7).
pub fn parse_command(line: &str, max: u16) -> Result<ParsedCommand, MatrixError> {
    let c = line.trim().to_ascii_uppercase();
    if c.is_empty() {
        return Err(MatrixError::new(1, "Empty command"));
    }
    if let Some(bad) = c.chars().find(|ch| !(ch.is_ascii_alphanumeric() || *ch == ';')) {
        return Err(MatrixError::new(8, format!("Invalid character '{bad}'")));
    }
    // Il `;` è legittimo solo in coda al Macro (§3.3): altrove è un carattere non valido,
    // quindi 0008 come ogni altro carattere non alfanumerico (§3.7), non 0001. Il
    // controllo sta **prima** dei comandi `X` perché `XGET;` cadeva nel ramo «comando
    // ignoto» e rispondeva 0001. I comandi `M…` passano indenni: il `;` finale e l'errore
    // 0017 per le porte dopo il `;` sono gestiti dal ramo del Macro.
    if c.contains(';') && !c.starts_with('M') {
        return Err(MatrixError::new(8, "Invalid character ';'"));
    }

    // Comandi X con argomento numerico (prima di quelli esatti: XGETLOS prima di XGET).
    let num_cmds: [NumCmd; 5] = [
        ("XGETLOS", ParsedCommand::Xgetlos),
        ("XLOS", ParsedCommand::Xlos),
        ("XRXPWR", ParsedCommand::Xrxpwr),
        ("XTXPWR", ParsedCommand::Xtxpwr),
        ("XCTIME", ParsedCommand::Xctime),
    ];
    for (prefix, ctor) in num_cmds {
        if let Some(rest) = c.strip_prefix(prefix) {
            let p = single_port_arg(rest)?;
            let zero_ok = prefix == "XGETLOS";
            if p == ALL_PORTS {
                if prefix == "XCTIME" {
                    return Err(MatrixError::new(9, "A port value of 9999 is not allowed"));
                }
            } else if (p == 0 && !zero_ok) || p > max {
                return Err(MatrixError::new(21, format!("Port {p} out of range (1 thru {max})")));
            }
            return Ok(ctor(p));
        }
    }
    if let Some(rest) = c.strip_prefix("XPUT") {
        return Routing::from_xget(rest, max)
            .map(ParsedCommand::Put)
            .map_err(|e| MatrixError::new(1, format!("Syntax error in XPUT: {e}")));
    }
    if let Some(rest) = c.strip_prefix("XTEST") {
        return match rest {
            "1" => Ok(ParsedCommand::Xtest(1)),
            "2" => Ok(ParsedCommand::Xtest(2)),
            "3" => Ok(ParsedCommand::Xtest(3)),
            _ => Err(MatrixError::new(1, "Unknown command")),
        };
    }
    let exact = match c.as_str() {
        "XGET" => Some(ParsedCommand::Xget),
        "XSTATUSIO" => Some(ParsedCommand::XstatusIo),
        "XSTATUSOI" => Some(ParsedCommand::XstatusOi),
        "XVERSION" => Some(ParsedCommand::Xversion),
        "XTYPE" | "XDCSTYPE" => Some(ParsedCommand::Xtype),
        "XMAXPORTS" => Some(ParsedCommand::Xmaxports),
        "XMAXCARDS" => Some(ParsedCommand::Xmaxcards),
        "XINSTALLED" => Some(ParsedCommand::Xinstalled),
        "XPORTCONFIG" => Some(ParsedCommand::Xportconfig),
        "XOPTIONS" => Some(ParsedCommand::Xoptions),
        "XALARM" => Some(ParsedCommand::Xalarm),
        "XLASTEVENT" => Some(ParsedCommand::Xlastevent),
        "XCRON" => Some(ParsedCommand::Xcron),
        "XCROFF" => Some(ParsedCommand::Xcroff),
        "XQUIT" => Some(ParsedCommand::Xquit),
        "XRESET" => Some(ParsedCommand::Xreset),
        "XHELP" => Some(ParsedCommand::Xhelp),
        _ => None,
    };
    if let Some(p) = exact {
        return Ok(p);
    }
    if c.starts_with('X') {
        return Err(MatrixError::new(1, "Unknown command"));
    }
    if let Some(rest) = c.strip_prefix("CI") {
        return parse_ci(rest, max);
    }
    if let Some(rest) = c.strip_prefix("DI") {
        let ports = digits_to_ports(rest)?;
        if ports.is_empty() {
            return Err(MatrixError::new(2, "Missing input port"));
        }
        for &p in &ports {
            if p != ALL_PORTS && (p == 0 || p > max) {
                return Err(MatrixError::new(6, out_of_range("Input", p, max)));
            }
        }
        return Ok(ParsedCommand::DisconnectInputs(ports));
    }
    if let Some(rest) = c.strip_prefix("DO") {
        let ports = digits_to_ports(rest)?;
        if ports.is_empty() {
            return Err(MatrixError::new(3, "Missing output port"));
        }
        for &p in &ports {
            if p != ALL_PORTS && (p == 0 || p > max) {
                return Err(MatrixError::new(7, out_of_range("Output", p, max)));
            }
        }
        return Ok(ParsedCommand::DisconnectOutputs(ports));
    }
    if let Some(rest) = c.strip_prefix("SI") {
        let p = parse_status_port(rest, max, true)?;
        return Ok(ParsedCommand::StatusIn(p));
    }
    if let Some(rest) = c.strip_prefix("SO") {
        let p = parse_status_port(rest, max, false)?;
        return Ok(ParsedCommand::StatusOut(p));
    }
    if let Some(rest) = c.strip_prefix('M') {
        let (digits, semicolon) = match rest.find(';') {
            Some(i) => {
                if !rest[i + 1..].is_empty() {
                    return Err(MatrixError::new(
                        17,
                        "Port numbers are not allowed after ';' on a unidirectional switch",
                    ));
                }
                (&rest[..i], true)
            }
            None => (rest, false),
        };
        if let Some(ch) = digits.chars().find(|ch| !ch.is_ascii_digit()) {
            return Err(MatrixError::new(8, format!("Invalid character '{ch}'")));
        }
        if digits.len() % 8 != 0 {
            return Err(MatrixError::new(1, "Macro pairs must be 8 digits"));
        }
        let pairs: Vec<(u16, u16)> =
            digits.as_bytes().chunks(8).map(|ch| (num4(&ch[..4]), num4(&ch[4..]))).collect();
        for &(i, o) in &pairs {
            if i == ALL_PORTS {
                return Err(MatrixError::new(
                    9,
                    "A port value of 9999 is not allowed for an input",
                ));
            }
            if i > max {
                return Err(MatrixError::new(6, out_of_range("Input", i, max)));
            }
            if o != ALL_PORTS && o > max {
                return Err(MatrixError::new(7, out_of_range("Output", o, max)));
            }
        }
        return Ok(ParsedCommand::Macro { pairs, semicolon });
    }
    Err(MatrixError::new(1, "Unknown command"))
}

fn parse_status_port(rest: &str, max: u16, input: bool) -> Result<u16, MatrixError> {
    let ports = digits_to_ports(rest)?;
    let (missing, multi, range, kind) =
        if input { (2, 4, 6, "Input") } else { (3, 5, 7, "Output") };
    match ports.as_slice() {
        [] => Err(MatrixError::new(missing, format!("Missing {} port", kind.to_lowercase()))),
        [p] => {
            if *p == ALL_PORTS {
                Err(MatrixError::new(9, "A port value of 9999 is not allowed in this command"))
            } else if *p == 0 || *p > max {
                Err(MatrixError::new(range, out_of_range(kind, *p, max)))
            } else {
                Ok(*p)
            }
        }
        _ => Err(MatrixError::new(
            multi,
            format!("Only one {} port is allowed", kind.to_lowercase()),
        )),
    }
}

fn parse_ci(rest: &str, max: u16) -> Result<ParsedCommand, MatrixError> {
    // Segmenti: (lettera, cifre). Il primo segmento è l'ingresso implicito dopo "CI".
    let mut segs: Vec<(char, String)> = vec![('I', String::new())];
    for ch in rest.chars() {
        if ch.is_ascii_digit() {
            segs.last_mut().expect("almeno un segmento").1.push(ch);
        } else if ch == 'I' || ch == 'O' {
            segs.push((ch, String::new()));
        } else {
            return Err(MatrixError::new(1, format!("Unexpected character '{ch}'")));
        }
    }
    let inputs: Vec<&String> = segs.iter().filter(|s| s.0 == 'I').map(|s| &s.1).collect();
    let outputs: Vec<&String> = segs.iter().filter(|s| s.0 == 'O').map(|s| &s.1).collect();
    if inputs.len() > 1 {
        return Err(MatrixError::new(4, "Only one input port is allowed"));
    }
    if inputs[0].is_empty() {
        return Err(MatrixError::new(2, "Missing input port"));
    }
    if outputs.is_empty() || outputs.iter().any(|o| o.is_empty()) {
        return Err(MatrixError::new(3, "Missing output port"));
    }
    if inputs[0].len() != 4 || outputs.iter().any(|o| o.len() != 4) {
        return Err(MatrixError::new(1, "Port numbers must be 4 digits"));
    }
    let input = num4(inputs[0].as_bytes());
    if input == ALL_PORTS {
        return Err(MatrixError::new(9, "A port value of 9999 is not allowed for an input"));
    }
    if input == 0 || input > max {
        return Err(MatrixError::new(6, out_of_range("Input", input, max)));
    }
    let mut outs = Vec::with_capacity(outputs.len());
    for o in outputs {
        let p = num4(o.as_bytes());
        if p != ALL_PORTS && (p == 0 || p > max) {
            return Err(MatrixError::new(7, out_of_range("Output", p, max)));
        }
        outs.push(p);
    }
    Ok(ParsedCommand::Connect { input, outputs: outs })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_ci() {
        assert_eq!(ci(24, 4, &[7]).unwrap(), "CI0004O0007");
        assert_eq!(ci(24, 10, &[10 + 1, 12]).unwrap(), "CI0010O0011O0012");
        assert_eq!(ci(24, 4, &[4]), Err(CmdError::Loopback(4)));
        assert_eq!(ci(24, 0, &[4]), Err(CmdError::PortZero));
        assert_eq!(ci(24, 4, &[0]), Err(CmdError::PortZero));
        assert_eq!(ci(24, 4, &[25]), Err(CmdError::OutOfRange { port: 25, max: 24 }));
        assert_eq!(ci(24, 4, &[9999]), Err(CmdError::AllPortsNotAllowed));
        assert_eq!(ci(24, 4, &[1, 9999]), Err(CmdError::AllPortsNotAllowed));
        assert_eq!(ci(24, 9999, &[1]), Err(CmdError::AllPortsNotAllowed));
        assert_eq!(ci(24, 4, &[]), Err(CmdError::Empty));
        assert_eq!(ci(24, 4, &[5, 5]), Err(CmdError::Duplicate(5)));
        assert_eq!(ci(0, 4, &[5]), Err(CmdError::UnknownMaxPorts));
    }

    #[test]
    fn builder_di_do_si_so() {
        assert_eq!(di(24, &[15, 25 - 1]).unwrap(), "DI00150024");
        assert_eq!(do_(24, &[11]).unwrap(), "DO0011");
        assert_eq!(di(24, &[9999]), Err(CmdError::AllPortsNotAllowed));
        assert_eq!(do_(24, &[]), Err(CmdError::Empty));
        assert_eq!(di_all(), "DI9999");
        assert_eq!(do_all(), "DO9999");
        assert_eq!(si(24, 4).unwrap(), "SI0004");
        assert_eq!(so(24, 24).unwrap(), "SO0024");
        assert!(so(24, 25).is_err());
        assert_eq!(xctime(24, 1).unwrap(), "XCTIME0001");
    }

    #[test]
    fn builder_macro_e_decoder_round_trip() {
        let pairs = [(15, 0), (25, 0), (25, 14), (0, 10)];
        let m = macro_(320, &pairs).unwrap();
        assert_eq!(m, "M00150000002500000025001400000010");
        let d = decode_macro(&m).unwrap();
        assert_eq!(d.pairs, pairs);
        assert!(!d.semicolon);
        assert_eq!(
            d.ops,
            vec![
                MacroOp::DisconnectInput(15),
                MacroOp::DisconnectInput(25),
                MacroOp::Connect { input: 25, output: 14 },
                MacroOp::DisconnectOutput(10),
            ]
        );
        assert_eq!(macro_(24, &[(0, 0)]), Err(CmdError::MacroPairZero));
        assert_eq!(macro_(24, &[(3, 3)]), Err(CmdError::Loopback(3)));
        assert_eq!(macro_(24, &[(3, 9999)]), Err(CmdError::AllPortsNotAllowed));
        assert_eq!(macro_(24, &[(0, 9999)]), Err(CmdError::AllPortsNotAllowed));
        assert_eq!(macro_(24, &[(30, 1)]), Err(CmdError::OutOfRange { port: 30, max: 24 }));
        assert_eq!(macro_(24, &[]), Err(CmdError::Empty));
    }

    #[test]
    fn macro_a_blocchi() {
        let pairs: Vec<(u16, u16)> = (1..=120).map(|o| (if o == 1 { 2 } else { 1 }, o)).collect();
        let chunks = macro_chunks(128, &pairs).unwrap();
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].len(), 1 + 50 * 8);
        assert_eq!(chunks[2].len(), 1 + 20 * 8);
        let rejoined: Vec<(u16, u16)> =
            chunks.iter().flat_map(|c| decode_macro(c).unwrap().pairs).collect();
        assert_eq!(rejoined, pairs);
    }

    #[test]
    fn appendice_c_corretta() {
        // §13.1 #1: DI0015, DI0025, CI0025O0014
        let d = decode_macro("M001500000025000000250014").unwrap();
        assert_eq!(
            d.ops,
            vec![
                MacroOp::DisconnectInput(15),
                MacroOp::DisconnectInput(25),
                MacroOp::Connect { input: 25, output: 14 },
            ]
        );
        assert_eq!(
            macro_(320, &[(15, 0), (25, 0), (25, 14)]).unwrap(),
            "M001500000025000000250014"
        );
        // §13.1 #2: DI0009, DO0010, DO0012, CI0014O0009, CI0016O0011
        let d = decode_macro("M0009000000000010000000120014000900160011").unwrap();
        assert_eq!(
            d.ops,
            vec![
                MacroOp::DisconnectInput(9),
                MacroOp::DisconnectOutput(10),
                MacroOp::DisconnectOutput(12),
                MacroOp::Connect { input: 14, output: 9 },
                MacroOp::Connect { input: 16, output: 11 },
            ]
        );
        assert_eq!(
            macro_(320, &[(9, 0), (0, 10), (0, 12), (14, 9), (16, 11)]).unwrap(),
            "M0009000000000010000000120014000900160011"
        );
        // Gli esempi originali (sbagliati) del manuale NON decodificano nell'intento.
        let wrong = decode_macro("M001500000015000000150014").unwrap();
        assert!(!wrong.ops.contains(&MacroOp::DisconnectInput(25)));
        let wrong = decode_macro("M0009000000100000001200000014000900160011").unwrap();
        assert!(wrong.ops.contains(&MacroOp::DisconnectInput(10)));
    }

    #[test]
    fn decode_macro_casi_limite() {
        assert_eq!(decode_macro("M").unwrap().pairs, vec![]);
        let d = decode_macro("m00009999;").unwrap();
        assert!(d.semicolon);
        assert_eq!(d.ops, vec![MacroOp::DisconnectAll]);
        assert!(decode_macro("M00010002;00030004").is_err());
        assert!(decode_macro("M0001000").is_err());
        assert!(decode_macro("X0001").is_err());
        assert_eq!(decode_macro("M00049999").unwrap().ops, vec![MacroOp::Broadcast(4)]);
    }

    #[test]
    fn parse_command_codici_errore() {
        let e = parse_command("CI0004O0087", 80).unwrap_err();
        assert_eq!(e.code, 7);
        assert_eq!(e.detail, "Output port number 87 is out of range (1 thru 80)");
        assert_eq!(parse_command("CI0090O0001", 80).unwrap_err().code, 6);
        assert_eq!(parse_command("CI9999O0001", 80).unwrap_err().code, 9);
        assert_eq!(parse_command("CI0001", 80).unwrap_err().code, 3);
        assert_eq!(parse_command("CIO0001", 80).unwrap_err().code, 2);
        assert_eq!(parse_command("CI0001I0002O0003", 80).unwrap_err().code, 4);
        assert_eq!(parse_command("CI001O0002", 80).unwrap_err().code, 1);
        assert_eq!(parse_command("CI0001 O0002", 80).unwrap_err().code, 8);
        assert_eq!(parse_command("DI", 80).unwrap_err().code, 2);
        assert_eq!(parse_command("DO", 80).unwrap_err().code, 3);
        assert_eq!(parse_command("DO0081", 80).unwrap_err().code, 7);
        assert_eq!(parse_command("DI0000", 80).unwrap_err().code, 6);
        assert_eq!(parse_command("SI9999", 80).unwrap_err().code, 9);
        assert_eq!(parse_command("SI00010002", 80).unwrap_err().code, 4);
        assert_eq!(parse_command("SO00010002", 80).unwrap_err().code, 5);
        assert_eq!(parse_command("M00010002;00030004", 80).unwrap_err().code, 17);
        assert_eq!(parse_command("M99990001", 80).unwrap_err().code, 9);
        assert_eq!(parse_command("M0001000", 80).unwrap_err().code, 1);
        assert_eq!(parse_command("XFOO", 80).unwrap_err().code, 1);
        assert_eq!(parse_command("FOO", 80).unwrap_err().code, 1);
        assert_eq!(parse_command("XLOS0081", 80).unwrap_err().code, 21);
        assert_eq!(parse_command("XCTIME9999", 80).unwrap_err().code, 9);
    }

    #[test]
    fn parse_command_ok() {
        assert_eq!(
            parse_command("ci0005o0010", 24).unwrap(),
            ParsedCommand::Connect { input: 5, outputs: vec![10] }
        );
        assert_eq!(
            parse_command("CI0004O9999", 24).unwrap(),
            ParsedCommand::Connect { input: 4, outputs: vec![9999] }
        );
        assert_eq!(parse_command("Si0004", 24).unwrap(), ParsedCommand::StatusIn(4));
        assert_eq!(
            parse_command("DI9999", 24).unwrap(),
            ParsedCommand::DisconnectInputs(vec![9999])
        );
        assert_eq!(
            parse_command("M", 24).unwrap(),
            ParsedCommand::Macro { pairs: vec![], semicolon: false }
        );
        assert_eq!(parse_command("XLOS9999", 24).unwrap(), ParsedCommand::Xlos(9999));
        assert_eq!(parse_command("XGETLOS0000", 24).unwrap(), ParsedCommand::Xgetlos(0));
        assert_eq!(parse_command("xdcstype", 24).unwrap(), ParsedCommand::Xtype);
        assert_eq!(parse_command("XTEST3", 24).unwrap(), ParsedCommand::Xtest(3));
        match parse_command("xputI00010003I00020004", 24).unwrap() {
            ParsedCommand::Put(r) => assert_eq!(r.connections(), vec![(1, 3), (2, 4)]),
            other => panic!("{other:?}"),
        }
        assert!(
            matches!(parse_command("XPUT", 24).unwrap(), ParsedCommand::Put(r) if r.connections().is_empty())
        );
        assert!(parse_command("XGET", 24).unwrap().is_bulk_status());
        assert!(parse_command("M00010002", 24).unwrap().is_switching());
    }

    /// Rilievo 5: il `;` è un carattere non valido (0008, §3.7) anche sui comandi `X`,
    /// dove prima cadeva nel ramo «comando ignoto» (0001). Il Macro non cambia.
    #[test]
    fn punto_e_virgola_fuori_dal_macro_da_0008() {
        for cmd in [
            "XGET;",
            "XGET;;",
            ";XGET",
            "XSTATUSIO;",
            "XLOS0014;",
            "XPUT;",
            "XTEST1;",
            "CI0001O0002;",
            "DI0001;",
            "SI0001;",
            ";",
        ] {
            let e = parse_command(cmd, 24).unwrap_err();
            assert_eq!(e.code, 8, "{cmd} doveva dare 0008, ha dato {}", e.code);
        }
        // Il Macro resta identico: `;` finale ammesso, porte dopo il `;` = 0017.
        assert_eq!(
            parse_command("M00010002;", 24).unwrap(),
            ParsedCommand::Macro { pairs: vec![(1, 2)], semicolon: true }
        );
        assert_eq!(
            parse_command("m00009999;", 24).unwrap(),
            ParsedCommand::Macro { pairs: vec![(0, 9999)], semicolon: true }
        );
        assert_eq!(
            parse_command("M;", 24).unwrap(),
            ParsedCommand::Macro { pairs: vec![], semicolon: true }
        );
        assert_eq!(parse_command("M00010002;00030004", 24).unwrap_err().code, 17);
    }
}
