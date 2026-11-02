//! Stato degli incroci della matrice, vista per uscite (spec §3.4, §4, §6.1).

use serde::{Deserialize, Serialize};

use super::command::{ParsedCommand, ALL_PORTS};
use super::response::ParseError;

/// Stato degli incroci: `outputs[k]` = ingresso collegato all'uscita `k+1` (`0` = scollegata).
/// La lunghezza di `outputs` è sempre `max_ports`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Routing {
    /// N della matrice.
    pub max_ports: u16,
    /// Ingresso per ogni uscita (indice 0 = uscita 1).
    pub outputs: Vec<u16>,
}

fn invalid(msg: impl Into<String>) -> ParseError {
    ParseError::InvalidData(msg.into())
}

/// Rifiuta i dati non ASCII **prima** di qualunque taglio per indice di byte.
///
/// `transport.rs` costruisce la riga con `String::from_utf8_lossy`: ogni byte non valido
/// (rumore su RS-232, segmento TCP corrotto) diventa un `U+FFFD` che occupa 3 byte. I
/// controlli di lunghezza e i tagli dei parser di stato ragionano in byte, quindi un
/// indice cadrebbe **dentro** quel carattere e lo slicing andrebbe in panic. La §3.2
/// regola 7 vuole che una riga inattesa si logghi e si scarti: qui diventa un errore.
fn require_ascii(data: &str, what: &str) -> Result<(), ParseError> {
    if !data.is_ascii() {
        return Err(invalid(format!("{what}: dati non ASCII '{data}'")));
    }
    Ok(())
}

fn parse_port4(s: &str, max: u16, what: &str) -> Result<u16, ParseError> {
    if s.len() != 4 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(invalid(format!("{what} non a 4 cifre: '{s}'")));
    }
    let p: u16 = s.parse().map_err(|_| invalid(format!("{what} non numerico: '{s}'")))?;
    if p == 0 || p > max {
        return Err(invalid(format!("{what} {p} fuori intervallo (1–{max})")));
    }
    Ok(p)
}

impl Routing {
    /// Matrice con tutte le uscite scollegate.
    pub fn new(max_ports: u16) -> Self {
        Routing { max_ports, outputs: vec![0; max_ports as usize] }
    }

    /// Ingresso collegato all'uscita (0 = nessuno o uscita fuori intervallo).
    pub fn input_of(&self, output: u16) -> u16 {
        if output == 0 {
            return 0;
        }
        self.outputs.get(output as usize - 1).copied().unwrap_or(0)
    }

    /// Imposta l'ingresso di un'uscita (0 = scollega). Ignora uscite fuori intervallo.
    pub fn set(&mut self, output: u16, input: u16) {
        if output >= 1 && output <= self.max_ports {
            self.outputs[output as usize - 1] = input;
        }
    }

    /// Uscite alimentate dall'ingresso, in ordine crescente.
    pub fn outputs_of(&self, input: u16) -> Vec<u16> {
        if input == 0 {
            return Vec::new();
        }
        self.outputs
            .iter()
            .enumerate()
            .filter(|(_, &i)| i == input)
            .map(|(k, _)| k as u16 + 1)
            .collect()
    }

    /// Sinonimo di [`Routing::outputs_of`] (nome usato nella specifica di sviluppo):
    /// restituisce le **uscite** alimentate dall'ingresso.
    pub fn inputs_of(&self, input: u16) -> Vec<u16> {
        self.outputs_of(input)
    }

    /// Tutte le connessioni `(ingresso, uscita)`, ordinate per ingresso e poi per uscita.
    pub fn connections(&self) -> Vec<(u16, u16)> {
        let mut v: Vec<(u16, u16)> = self
            .outputs
            .iter()
            .enumerate()
            .filter(|(_, &i)| i != 0)
            .map(|(k, &i)| (i, k as u16 + 1))
            .collect();
        v.sort_unstable();
        v
    }

    /// Decodifica i dati di `XGET` (formato **a gruppi**: `I<in><out><out>…I<in>…`).
    /// Dati vuoti = nessuna connessione.
    pub fn from_xget(data: &str, max_ports: u16) -> Result<Routing, ParseError> {
        require_ascii(data, "XGET")?;
        let mut r = Routing::new(max_ports);
        let data = data.trim();
        if data.is_empty() {
            return Ok(r);
        }
        let first = data.as_bytes()[0];
        if first != b'I' && first != b'i' {
            return Err(invalid(format!("XGET: atteso 'I' all'inizio di '{data}'")));
        }
        for group in data[1..].split(['I', 'i']) {
            if group.is_empty() || group.len() % 4 != 0 {
                return Err(invalid(format!("XGET: gruppo malformato 'I{group}'")));
            }
            let input = parse_port4(&group[..4], max_ports, "ingresso")?;
            let mut k = 4;
            while k < group.len() {
                let output = parse_port4(&group[k..k + 4], max_ports, "uscita")?;
                if r.input_of(output) != 0 {
                    return Err(invalid(format!("XGET: uscita {output} ripetuta")));
                }
                r.set(output, input);
                k += 4;
            }
        }
        Ok(r)
    }

    /// Genera la stringa di `XGET`/`XPUT` (gruppi per ingresso crescente, uscite crescenti).
    pub fn to_xget(&self) -> String {
        let mut s = String::new();
        let mut current = 0u16;
        for (i, o) in self.connections() {
            if i != current {
                s.push_str(&format!("I{i:04}"));
                current = i;
            }
            s.push_str(&format!("{o:04}"));
        }
        s
    }

    /// Decodifica i dati di `XSTATUSIO` (`I` + coppie `iiiioooo` + `;`).
    pub fn from_statusio(data: &str, max_ports: u16) -> Result<Routing, ParseError> {
        Self::from_pairs(data, max_ports, 'I')
    }

    /// Decodifica i dati di `XSTATUSOI` (`O` + coppie `ooooiiii` + `;`).
    pub fn from_statusoi(data: &str, max_ports: u16) -> Result<Routing, ParseError> {
        Self::from_pairs(data, max_ports, 'O')
    }

    fn from_pairs(data: &str, max_ports: u16, lead: char) -> Result<Routing, ParseError> {
        require_ascii(data, "coppie di stato")?;
        let d = data.trim();
        let body = d
            .strip_prefix(lead)
            .or_else(|| d.strip_prefix(lead.to_ascii_lowercase()))
            .ok_or_else(|| invalid(format!("atteso '{lead}' all'inizio di '{d}'")))?;
        let body = body.strip_suffix(';').unwrap_or(body);
        if body.len() % 8 != 0 {
            return Err(invalid(format!("coppie malformate in '{d}'")));
        }
        let mut r = Routing::new(max_ports);
        for k in (0..body.len()).step_by(8) {
            let a = parse_port4(&body[k..k + 4], max_ports, "porta")?;
            let b = parse_port4(&body[k + 4..k + 8], max_ports, "porta")?;
            let (input, output) = if lead == 'I' { (a, b) } else { (b, a) };
            if r.input_of(output) != 0 {
                return Err(invalid(format!("uscita {output} ripetuta")));
            }
            r.set(output, input);
        }
        Ok(r)
    }

    /// Dati di `XSTATUSIO` (`I` + coppie + `;`).
    pub fn to_statusio(&self) -> String {
        let mut s = String::from("I");
        for (i, o) in self.connections() {
            s.push_str(&format!("{i:04}{o:04}"));
        }
        s.push(';');
        s
    }

    /// Dati di `XSTATUSOI` (`O` + coppie uscita/ingresso + `;`).
    pub fn to_statusoi(&self) -> String {
        let mut s = String::from("O");
        for (k, &i) in self.outputs.iter().enumerate() {
            if i != 0 {
                s.push_str(&format!("{:04}{i:04}", k + 1));
            }
        }
        s.push(';');
        s
    }

    /// Semantica di `CI`: additivo sull'ingresso, sostitutivo sull'uscita; `9999` = tutte.
    pub fn apply_ci(&mut self, input: u16, outputs: &[u16]) {
        for &o in outputs {
            if o == ALL_PORTS {
                self.outputs.iter_mut().for_each(|x| *x = input);
            } else {
                self.set(o, input);
            }
        }
    }

    /// Semantica di `DI`: scollega tutte le uscite alimentate dagli ingressi; `9999` = tutto.
    pub fn apply_di(&mut self, inputs: &[u16]) {
        for &i in inputs {
            if i == ALL_PORTS {
                self.clear();
            } else if i != 0 {
                self.outputs.iter_mut().filter(|x| **x == i).for_each(|x| *x = 0);
            }
        }
    }

    /// Semantica di `DO`: scollega le uscite; `9999` = tutte.
    pub fn apply_do(&mut self, outputs: &[u16]) {
        for &o in outputs {
            if o == ALL_PORTS {
                self.clear();
            } else {
                self.set(o, 0);
            }
        }
    }

    /// Semantica del Macro, coppia per coppia nell'ordine scritto:
    /// `(0, 9999)` scollega tutto; `(0, o)` = `DO o`; `(i, 0)` = `DI i`;
    /// `(i, 9999)` = broadcast; altrimenti `CI i O o`.
    pub fn apply_macro(&mut self, pairs: &[(u16, u16)]) {
        for &(i, o) in pairs {
            match (i, o) {
                (0, 0) => {}
                (0, ALL_PORTS) => self.clear(),
                (0, o) => self.set(o, 0),
                (i, 0) => self.apply_di(&[i]),
                (i, o) => self.apply_ci(i, &[o]),
            }
        }
    }

    /// Semantica di `XPUT`: spegne tutto e applica lo stato indicato.
    pub fn apply_put(&mut self, other: &Routing) {
        self.clear();
        for (i, o) in other.connections() {
            self.set(o, i);
        }
    }

    /// Scollega tutte le uscite.
    pub fn clear(&mut self) {
        self.outputs.iter_mut().for_each(|x| *x = 0);
    }

    /// Applica un comando di commutazione già decodificato. Restituisce `true` se il comando
    /// è stato applicato; `false` per i comandi non di commutazione o dall'effetto non
    /// modellabile (`XRESET`, `XTEST*`: occorre rileggere lo stato).
    pub fn apply_command(&mut self, cmd: &ParsedCommand) -> bool {
        match cmd {
            ParsedCommand::Connect { input, outputs } => self.apply_ci(*input, outputs),
            ParsedCommand::DisconnectInputs(v) => self.apply_di(v),
            ParsedCommand::DisconnectOutputs(v) => self.apply_do(v),
            ParsedCommand::Macro { pairs, .. } => self.apply_macro(pairs),
            ParsedCommand::Put(r) => self.apply_put(r),
            _ => return false,
        }
        true
    }

    /// Differenze verso `other`: `(uscita, ingresso_attuale, ingresso_in_other)` per ogni
    /// uscita che cambia. Se le dimensioni differiscono, le uscite mancanti valgono 0.
    pub fn diff(&self, other: &Routing) -> Vec<(u16, u16, u16)> {
        let n = self.max_ports.max(other.max_ports);
        (1..=n)
            .filter_map(|o| {
                let a = self.input_of(o);
                let b = other.input_of(o);
                (a != b).then_some((o, a, b))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xget_fixture_cfg_p15() {
        let r = Routing::from_xget("I001000470048I00120015I00250033I00410003", 48).unwrap();
        assert_eq!(r.connections(), vec![(10, 47), (10, 48), (12, 15), (25, 33), (41, 3)]);
        assert_eq!(r.to_xget(), "I001000470048I00120015I00250033I00410003");
    }

    #[test]
    fn xget_fixture_api_p11_round_trip_multicast() {
        let s = "I00010003I00020004I00100010I00110012";
        let r = Routing::from_xget(s, 24).unwrap();
        assert_eq!(r.input_of(3), 1);
        assert_eq!(r.input_of(4), 2);
        assert_eq!(r.input_of(10), 10);
        assert_eq!(r.input_of(12), 11);
        assert_eq!(r.to_xget(), s);

        // ingresso 10 in multicast su 10, 11, 12 (esempio XPUT del manuale)
        let s = "I00010003I00020004I0010001000110012";
        let r = Routing::from_xget(s, 24).unwrap();
        assert_eq!(r.outputs_of(10), vec![10, 11, 12]);
        assert_eq!(r.inputs_of(10), vec![10, 11, 12]);
        assert_eq!(r.to_xget(), s);
        let r2 = Routing::from_xget(&r.to_xget(), 24).unwrap();
        assert_eq!(r, r2);

        let r = Routing::from_xget("I01230145", 320).unwrap();
        assert_eq!(r.connections(), vec![(123, 145)]);
        assert_eq!(Routing::from_xget("", 24).unwrap(), Routing::new(24));
        assert_eq!(Routing::from_xget("I00010001", 24).unwrap().connections(), vec![(1, 1)]);
    }

    #[test]
    fn xget_errori() {
        assert!(Routing::from_xget("00010002", 24).is_err());
        assert!(Routing::from_xget("I0001000", 24).is_err());
        assert!(Routing::from_xget("I00010025", 24).is_err());
        assert!(Routing::from_xget("I00000001", 24).is_err());
        assert!(Routing::from_xget("I00010002I00030002", 24).is_err());
        assert!(Routing::from_xget("I", 24).is_err());
        assert!(Routing::from_xget("I0001000x", 24).is_err());
    }

    #[test]
    fn statusio_e_statusoi() {
        let r = Routing::from_statusio("I0010012300120015;", 160).unwrap();
        assert_eq!(r.connections(), vec![(10, 123), (12, 15)]);
        let r2 = Routing::from_statusoi("O0015001201230010;", 160).unwrap();
        assert_eq!(r, r2);
        assert_eq!(Routing::from_statusio("I;", 24).unwrap(), Routing::new(24));
        assert_eq!(Routing::from_statusoi("O;", 24).unwrap(), Routing::new(24));
        assert_eq!(r.to_statusio(), "I0010012300120015;");
        assert_eq!(r.to_statusoi(), "O0015001201230010;");
        assert!(Routing::from_statusio("O0001;", 24).is_err());
        assert!(Routing::from_statusio("I000100;", 24).is_err());
    }

    #[test]
    fn semantica_ci_di_do() {
        let mut r = Routing::new(24);
        r.apply_ci(5, &[7]);
        r.apply_ci(5, &[8]); // additivo sull'ingresso
        assert_eq!(r.outputs_of(5), vec![7, 8]);
        r.apply_ci(6, &[7]); // sostitutivo sull'uscita
        assert_eq!(r.outputs_of(5), vec![8]);
        assert_eq!(r.input_of(7), 6);
        r.apply_di(&[5]);
        assert_eq!(r.outputs_of(5), Vec::<u16>::new());
        r.apply_do(&[7]);
        assert_eq!(r.input_of(7), 0);
        r.apply_ci(3, &[9999]);
        assert!(r.outputs.iter().all(|&i| i == 3));
        r.apply_di(&[9999]);
        assert_eq!(r, Routing::new(24));
    }

    #[test]
    fn semantica_macro() {
        let mut r = Routing::new(320);
        r.apply_ci(14, &[15, 25]);
        r.apply_ci(15, &[14]);
        // Appendice C corretta #1: sposta la tastiera da RX1 (15) a RX2 (25)
        r.apply_macro(&[(15, 0), (25, 0), (25, 14)]);
        assert_eq!(r.input_of(14), 25);
        assert!(r.outputs_of(15).is_empty());
        r.apply_macro(&[(0, 15)]);
        assert_eq!(r.input_of(15), 0);
        r.apply_macro(&[(7, 9999)]);
        assert!(r.outputs.iter().all(|&i| i == 7));
        r.apply_macro(&[(0, 9999)]);
        assert!(r.connections().is_empty());
    }

    #[test]
    fn put_e_diff() {
        let mut r = Routing::new(24);
        r.apply_ci(1, &[2, 3]);
        let target = Routing::from_xget("I00040005", 24).unwrap();
        let d = r.diff(&target);
        assert_eq!(d, vec![(2, 1, 0), (3, 1, 0), (5, 0, 4)]);
        r.apply_put(&target);
        assert_eq!(r, target);
        assert!(r.diff(&target).is_empty());
    }

    #[test]
    fn collegamento_bidirezionale_stile_legacy() {
        let mut r = Routing::new(24);
        r.apply_ci(1, &[9]);
        r.apply_di(&[1]);
        r.apply_di(&[5]);
        r.apply_ci(1, &[5]);
        r.apply_ci(5, &[1]);
        assert_eq!(r.input_of(5), 1);
        assert_eq!(r.input_of(1), 5);
        assert_eq!(r.input_of(9), 0);
    }

    /// Rilievo 1: i byte non validi che `from_utf8_lossy` trasforma in `U+FFFD` (3 byte)
    /// facevano cadere lo slicing per byte. Devono dare `Err`, non un panic.
    #[test]
    fn dati_non_ascii_scartati_senza_panic() {
        // 12 byte, divisibile per 4: la vecchia versione tagliava dentro il U+FFFD.
        assert!(Routing::from_xget("I00\u{FFFD}0000000", 24).is_err());
        assert!(Routing::from_xget("I\u{FFFD}", 24).is_err());
        assert!(Routing::from_xget("\u{FFFD}", 24).is_err());
        // 24 byte dopo la lettera (3 coppie da 8 byte): la lunghezza passava il controllo.
        let corpo: String = "00\u{FFFD}000".repeat(3);
        assert_eq!(corpo.len(), 24);
        assert!(Routing::from_statusio(&format!("I{corpo};"), 160).is_err());
        assert!(Routing::from_statusoi(&format!("O{corpo};"), 160).is_err());
        // non ASCII anche fuori dai gruppi a 4 cifre (accento, carattere multibyte).
        assert!(Routing::from_xget("I0001000è", 24).is_err());
        assert!(Routing::from_statusio("Ià", 24).is_err());
    }
}
