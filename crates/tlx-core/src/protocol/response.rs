//! Parsing delle righe di risposta dell'API ASCII TLX (spec §3.2).
//!
//! Formato: `R` + 4 caratteri (campo lunghezza, **ignorato**: spesso `R0000`) +
//! `OK[dati]` oppure `ERnnnn`, eventualmente seguiti da `#commento` (modalità verbose).
//! I dati stanno fra `OK` e il primo `#` o la fine della riga.

use serde::{Deserialize, Serialize};

/// Errore di parsing di una risposta o di un campo dati.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    /// Riga vuota.
    #[error("riga vuota")]
    Empty,
    /// La riga non inizia con `R`: non è una risposta dell'API (es. output di `XHELP`).
    #[error("la riga non è una risposta TLX (non inizia con 'R'): {0}")]
    NotResponse(String),
    /// La riga inizia con `R` ma non ha la forma `R????OK…` / `R????ERnnnn…`.
    #[error("risposta malformata: {0}")]
    Malformed(String),
    /// I dati di una risposta `OK` non hanno il formato atteso per quel comando.
    #[error("dati non validi: {0}")]
    InvalidData(String),
}

/// Una risposta dell'API già scomposta.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Response {
    /// Riga grezza, senza terminatore (`\r`/`\n` rimossi).
    pub raw: String,
    /// `true` se la risposta è `OK`, `false` se è `ERnnnn`.
    pub ok: bool,
    /// Dati fra `OK` e il primo `#` (o fine riga). `None` se assenti o vuoti.
    pub data: Option<String>,
    /// Codice d'errore a 4 cifre (es. `"0007"`) per le risposte `ER`.
    pub error_code: Option<String>,
    /// Testo dopo il primo `#` (verbose): diagnostico, non stabile, solo da loggare.
    pub comment: Option<String>,
}

impl Response {
    /// Codice d'errore numerico (es. `7` per `ER0007`).
    pub fn error_code_num(&self) -> Option<u16> {
        self.error_code.as_deref().and_then(|c| c.parse().ok())
    }

    /// Dati come `&str` (stringa vuota se assenti).
    pub fn data_str(&self) -> &str {
        self.data.as_deref().unwrap_or("")
    }
}

/// Scompone una riga di risposta.
///
/// Regole (spec §3.2): il campo lunghezza non si usa; si accettano `LF` e `CRLF`
/// (i `\r` in coda vengono rimossi); i dati terminano al primo `#`; il commento è
/// conservato così com'è (anche se contiene l'eco in minuscolo del comando).
pub fn parse_response(line: &str) -> Result<Response, ParseError> {
    let raw = line.trim_end_matches(['\r', '\n']);
    if raw.is_empty() {
        return Err(ParseError::Empty);
    }
    if !raw.starts_with('R') {
        return Err(ParseError::NotResponse(raw.to_string()));
    }
    // 'R' + 4 caratteri qualsiasi (campo lunghezza inaffidabile).
    let body = match raw.char_indices().nth(5) {
        Some((idx, _)) => &raw[idx..],
        None => return Err(ParseError::Malformed(raw.to_string())),
    };

    if let Some(rest) = body.strip_prefix("OK") {
        let (data, comment) = split_comment(rest);
        return Ok(Response {
            raw: raw.to_string(),
            ok: true,
            data: if data.is_empty() { None } else { Some(data.to_string()) },
            error_code: None,
            comment,
        });
    }
    if let Some(rest) = body.strip_prefix("ER") {
        let code: String = rest.chars().take(4).collect();
        if code.len() != 4 || !code.bytes().all(|b| b.is_ascii_digit()) {
            return Err(ParseError::Malformed(raw.to_string()));
        }
        let after = &rest[4..];
        let comment = after.find('#').map(|i| after[i + 1..].to_string());
        return Ok(Response {
            raw: raw.to_string(),
            ok: false,
            data: None,
            error_code: Some(code),
            comment,
        });
    }
    Err(ParseError::Malformed(raw.to_string()))
}

fn split_comment(s: &str) -> (&str, Option<String>) {
    match s.find('#') {
        Some(i) => (&s[..i], Some(s[i + 1..].to_string())),
        None => (s, None),
    }
}

/// Descrizione in italiano di un codice d'errore `ERnnnn` (API p.23, spec §3.7).
pub fn error_description(code: u16) -> &'static str {
    match code {
        1 => "errore di sintassi o comando sconosciuto",
        2 => "manca il campo della porta d'ingresso",
        3 => "manca il campo della porta d'uscita",
        4 => "più campi d'ingresso dove ne è ammesso uno solo",
        5 => "più campi d'uscita dove ne è ammesso uno solo",
        6 => "porta d'ingresso fuori intervallo",
        7 => "porta d'uscita fuori intervallo",
        8 => "carattere non valido nel comando",
        9 => "il valore 9999 non è ammesso in questo comando",
        11 => "errore interno della matrice (fork): contattare il costruttore",
        13 => "memoria insufficiente nella matrice: contattare il costruttore",
        14 => "errore di I/O su file nella matrice: contattare il costruttore",
        15 => "il processo di controllo della matrice non risponde: contattare il costruttore",
        16 => "il comando cita più chassis",
        17 => "Macro con porte dopo il ';' (matrice unidirezionale)",
        20 => "errore di allocazione di memoria nella matrice: contattare il costruttore",
        21 => "porta fuori intervallo",
        10 | 12 | 18 | 19 => "codice non usato dall'API V5",
        _ => "codice d'errore sconosciuto",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ok_semplice_e_verbose() {
        let r = parse_response("R0003OK").unwrap();
        assert!(r.ok);
        assert_eq!(r.data, None);
        assert_eq!(r.comment, None);

        let r = parse_response("R0015OK#CI0004O0007").unwrap();
        assert!(r.ok);
        assert_eq!(r.data, None);
        assert_eq!(r.comment.as_deref(), Some("CI0004O0007"));
    }

    #[test]
    fn errore_semplice_e_verbose() {
        let r = parse_response("R0007ER0007").unwrap();
        assert!(!r.ok);
        assert_eq!(r.error_code.as_deref(), Some("0007"));
        assert_eq!(r.error_code_num(), Some(7));
        let r = parse_response(
            "R0072ER0007#Output port number 87 is out of range (1 thru 80): 'CI0004O0087'",
        )
        .unwrap();
        assert!(!r.ok);
        assert_eq!(r.error_code.as_deref(), Some("0007"));
        assert!(r.comment.unwrap().starts_with("Output port number 87"));
    }

    #[test]
    fn dati_con_commento_eco_minuscolo() {
        let r = parse_response("R0000OKI0150O0151#Si0150").unwrap();
        assert_eq!(r.data.as_deref(), Some("I0150O0151"));
        assert_eq!(r.comment.as_deref(), Some("Si0150"));
        let r = parse_response("R0000OKI0150O0151#So0151").unwrap();
        assert_eq!(r.data.as_deref(), Some("I0150O0151"));
        let r = parse_response("R0000OK#Do0151").unwrap();
        assert_eq!(r.data, None);
        assert_eq!(r.comment.as_deref(), Some("Do0151"));
        let r = parse_response("R0015OK#cI0001O0002").unwrap();
        assert_eq!(r.comment.as_deref(), Some("cI0001O0002"));
    }

    #[test]
    fn crlf_e_lunghezza_ignorata() {
        let r = parse_response("R0000OKI001000470048I00120015I00250033I00410003\r\n").unwrap();
        assert_eq!(r.data.as_deref(), Some("I001000470048I00120015I00250033I00410003"));
        assert_eq!(r.raw, "R0000OKI001000470048I00120015I00250033I00410003");
        // campo lunghezza non numerico: ignorato
        let r = parse_response("RxxxxOK0048\r").unwrap();
        assert_eq!(r.data.as_deref(), Some("0048"));
    }

    #[test]
    fn fixture_appendice_b() {
        let casi: &[(&str, Option<&str>, Option<&str>)] = &[
            ("R0012OKI00010001", Some("I00010001"), None),
            ("R0008OK#xput", None, Some("xput")),
            ("R0000OK#xquit", None, Some("xquit")),
            ("R0027OKI0010012300120015;", Some("I0010012300120015;"), None),
            ("R0005OKI;", Some("I;"), None),
            ("R0027OKO0015001201230010;", Some("O0015001201230010;"), None),
            ("R0011OKO;", Some("O;"), None),
            ("R0009OKV5.00", Some("V5.00"), None),
            ("R0014OKTLX48Switch", Some("TLX48Switch"), None),
            ("R0006OK111", Some("111"), None),
            ("R0017OK0003#XMAXCARDS", Some("0003"), Some("XMAXCARDS")),
            ("R0007OK0048", Some("0048"), None),
            (
                "R0032OK0001,612817,612817#xctime0001",
                Some("0001,612817,612817"),
                Some("xctime0001"),
            ),
            ("R0027OKThu Jul  1 11:23:52 2010", Some("Thu Jul  1 11:23:52 2010"), None),
            ("R0017OK0048,0000,0000", Some("0048,0000,0000"), None),
            ("R0007OK0522", Some("0522"), None),
            (
                "R0062OK0120#xoptions:Actual_Card_Count P2P Multicast Connect_Time",
                Some("0120"),
                Some("xoptions:Actual_Card_Count P2P Multicast Connect_Time"),
            ),
            ("R0005OKL0", Some("L0"), None),
            ("R0008OKT0451", Some("T0451"), None),
            ("R0008OKR0451", Some("R0451"), None),
            ("R0003OK", None, None),
        ];
        for (riga, dati, commento) in casi {
            let r = parse_response(riga).unwrap_or_else(|e| panic!("{riga}: {e}"));
            assert!(r.ok, "{riga}");
            assert_eq!(r.data.as_deref(), *dati, "{riga}");
            assert_eq!(r.comment.as_deref(), *commento, "{riga}");
        }
    }

    #[test]
    fn righe_non_valide() {
        assert_eq!(parse_response(""), Err(ParseError::Empty));
        assert_eq!(parse_response("\r\n"), Err(ParseError::Empty));
        assert!(matches!(
            parse_response("Version: V5.07.07 (build: 8)"),
            Err(ParseError::NotResponse(_))
        ));
        assert!(matches!(parse_response("R00"), Err(ParseError::Malformed(_))));
        assert!(matches!(parse_response("R0003XX"), Err(ParseError::Malformed(_))));
        assert!(matches!(parse_response("R0007ER00"), Err(ParseError::Malformed(_))));
        assert!(matches!(parse_response("R0007ERabcd"), Err(ParseError::Malformed(_))));
    }
}
