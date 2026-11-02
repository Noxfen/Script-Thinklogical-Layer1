//! Sessione di comando: un canale aperto, un comando in volo, timeout, handshake
//! (spec §5.1, §5.2).
//!
//! Dopo un timeout o un errore di I/O la sessione è **compromessa** (nessun correlatore fra
//! comandi e risposte): [`Session::is_broken`] diventa `true` e ogni comando successivo
//! fallisce subito; va chiusa e riaperta.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::events::{LogChannel, LogLevel, LogRecord, LogSink};
use crate::protocol::command::{self as cmds, parse_command, ParsedCommand, ALL_PORTS};
use crate::protocol::info::{self, MatrixInfo};
use crate::protocol::{error_description, parse_response, ParseError, Response};
use crate::transport::{LineTransport, TransportConfig, TransportError};
use crate::util::{now_ms, truncate};

/// Lunghezza massima delle righe TX/RX nel log.
pub const LOG_LINE_MAX: usize = 512;

/// Opzioni della sessione (timeout in ms, spec §5.1).
#[derive(Clone)]
pub struct SessionOptions {
    /// Timeout di apertura TCP quando il trasporto non ne indica uno (3000).
    pub connect_timeout_ms: u64,
    /// Timeout dei comandi ordinari (2000).
    pub cmd_timeout_ms: u64,
    /// Base dei comandi di stato (`XGET`, `XSTATUSIO`, `XLOS9999`, `XRXPWR9999`,
    /// `XTXPWR9999`), a cui si aggiungono 10 ms × N (2000).
    pub status_timeout_base_ms: u64,
    /// `--delay` della matrice: per ogni coppia di un Macro si aggiunge questo tempo (300).
    pub delay_ms: u64,
    /// Destinazione del log TX/RX.
    pub logger: Option<LogSink>,
}

impl Default for SessionOptions {
    fn default() -> Self {
        SessionOptions {
            connect_timeout_ms: 3000,
            cmd_timeout_ms: 2000,
            status_timeout_base_ms: 2000,
            delay_ms: 300,
            logger: None,
        }
    }
}

impl std::fmt::Debug for SessionOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionOptions")
            .field("connect_timeout_ms", &self.connect_timeout_ms)
            .field("cmd_timeout_ms", &self.cmd_timeout_ms)
            .field("status_timeout_base_ms", &self.status_timeout_base_ms)
            .field("delay_ms", &self.delay_ms)
            .finish()
    }
}

/// Esito di un comando.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CmdOutcome {
    /// Riga inviata (o rifiutata prima dell'invio).
    pub command: String,
    /// Risposta scomposta, se arrivata.
    pub response: Option<Response>,
    /// Errore di trasporto/timeout/rifiuto locale (in italiano). `None` se è arrivata una
    /// risposta, anche `ERnnnn`.
    pub error: Option<String>,
    /// Latenza in ms (invio → risposta).
    pub latency_ms: u64,
}

impl CmdOutcome {
    /// Esito di un comando non inviato.
    pub fn failed(command: impl Into<String>, error: impl Into<String>) -> CmdOutcome {
        CmdOutcome {
            command: command.into(),
            response: None,
            error: Some(error.into()),
            latency_ms: 0,
        }
    }

    /// `true` se è arrivata una risposta `OK`.
    pub fn ok(&self) -> bool {
        self.error.is_none() && self.response.as_ref().is_some_and(|r| r.ok)
    }

    /// Dati della risposta `OK`.
    pub fn data(&self) -> Option<&str> {
        self.response.as_ref().filter(|r| r.ok).and_then(|r| r.data.as_deref())
    }

    /// Codice `ERnnnn` (stringa a 4 cifre).
    pub fn error_code(&self) -> Option<&str> {
        self.response.as_ref().and_then(|r| r.error_code.as_deref())
    }

    /// Messaggio d'errore leggibile in italiano, `None` se `OK`.
    pub fn error_message(&self) -> Option<String> {
        if let Some(e) = &self.error {
            return Some(e.clone());
        }
        match &self.response {
            Some(r) if !r.ok => {
                let code = r.error_code_num().unwrap_or(0);
                Some(format!(
                    "la matrice ha risposto ER{:04} ({}) a «{}»",
                    code,
                    error_description(code),
                    self.command
                ))
            }
            Some(_) => None,
            None => Some("nessuna risposta".into()),
        }
    }
}

/// Errore di handshake.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HandshakeError {
    /// L'estremo risponde ma non è (o non è compatibile con) un'API TLX.
    #[error("handshake rifiutato: {0}")]
    Rejected(String),
    /// Sessione persa durante l'handshake (timeout, chiusura, I/O).
    #[error("sessione persa durante l'handshake: {0}")]
    Session(String),
}

/// Sessione aperta verso una matrice.
pub struct Session {
    transport: LineTransport,
    opts: SessionOptions,
    broken: bool,
    max_ports: u16,
    info: Option<MatrixInfo>,
    channel: LogChannel,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("endpoint", &self.transport.endpoint())
            .field("broken", &self.broken)
            .field("max_ports", &self.max_ports)
            .finish()
    }
}

fn is_status_command(p: &ParsedCommand) -> bool {
    p.is_bulk_status()
}

impl Session {
    /// Apre il canale (sulla seriale svuota il buffer d'ingresso).
    pub async fn open(
        cfg: &TransportConfig,
        opts: SessionOptions,
    ) -> Result<Session, TransportError> {
        // `connect_timeout_ms = 0` significa «usa il default», non «1 ms»: con 1 ms la
        // matrice non si raggiunge mai e il messaggio direbbe «non risponde entro 1 ms».
        let to = if opts.connect_timeout_ms == 0 { 3000 } else { opts.connect_timeout_ms.max(200) };
        let transport = LineTransport::open(cfg, Duration::from_millis(to)).await?;
        let channel = if transport.is_serial() { LogChannel::Serial } else { LogChannel::Tcp };
        let s = Session { transport, opts, broken: false, max_ports: 0, info: None, channel };
        s.log(LogLevel::Info, format!("Canale aperto verso {}", s.transport.endpoint()), None);
        Ok(s)
    }

    fn log(&self, level: LogLevel, message: String, latency_ms: Option<u64>) {
        if let Some(l) = &self.opts.logger {
            let channel = match level {
                LogLevel::Tx | LogLevel::Rx => self.channel,
                _ => LogChannel::App,
            };
            l(LogRecord { ts_ms: now_ms(), level, channel, message, latency_ms });
        }
    }

    /// `true` dopo un timeout, un errore di I/O, la chiusura dall'altro capo o `XQUIT`.
    pub fn is_broken(&self) -> bool {
        self.broken
    }

    /// N letto all'handshake (0 prima).
    pub fn max_ports(&self) -> u16 {
        self.max_ports
    }

    /// Profilo letto all'handshake.
    pub fn info(&self) -> Option<&MatrixInfo> {
        self.info.as_ref()
    }

    /// Estremo del canale.
    pub fn endpoint(&self) -> &str {
        self.transport.endpoint()
    }

    /// Canale di log (`tcp` o `serial`).
    pub fn channel(&self) -> LogChannel {
        self.channel
    }

    /// Timeout calcolato per un comando (spec §5.1): comandi di stato
    /// `base + 10 ms × N`; Macro `cmd + coppie × delay`; sulla seriale si aggiunge il tempo
    /// di trasmissione (≈1,2 ms per carattere a 9600 baud, scalato sulla velocità).
    pub fn timeout_for(&self, cmd: &str) -> Duration {
        let n = if self.max_ports > 0 { self.max_ports as u64 } else { 1280 };
        let c = cmd.trim().to_ascii_uppercase();
        let parsed = parse_command(&c, 9998).ok();
        let (base_ms, expected_bytes) = match &parsed {
            Some(p) if is_status_command(p) => {
                let bytes = match p {
                    ParsedCommand::Xget => 9 * n + 16,
                    ParsedCommand::XstatusIo | ParsedCommand::XstatusOi => 8 * n + 16,
                    ParsedCommand::Xlos(_) => n + 16,
                    ParsedCommand::Xgetlos(_) => 25 * n + 16,
                    _ => 4 * n + 16,
                };
                (self.opts.status_timeout_base_ms + 10 * n, bytes)
            }
            Some(ParsedCommand::Macro { pairs, .. }) => {
                (self.opts.cmd_timeout_ms + pairs.len() as u64 * self.opts.delay_ms, 64)
            }
            Some(ParsedCommand::Put(_)) => {
                (self.opts.status_timeout_base_ms + 10 * n + self.opts.delay_ms, 64)
            }
            Some(ParsedCommand::Connect { outputs, .. }) if outputs.contains(&ALL_PORTS) => {
                (self.opts.cmd_timeout_ms + self.opts.delay_ms, 64)
            }
            _ => (self.opts.cmd_timeout_ms, 96),
        };
        let mut ms = base_ms;
        if self.transport.is_serial() {
            // 9600 baud ≈ 1 ms/carattere: margine del 20 %.
            ms += ((c.len() as u64 + 1 + expected_bytes) * 12).div_ceil(10);
        }
        Duration::from_millis(ms.max(1))
    }

    /// Invia **esattamente** la riga indicata (spazi esterni rimossi, nessuna conversione
    /// in maiuscolo) e attende la risposta. `timeout = None` → [`Session::timeout_for`].
    ///
    /// Rifiuta senza inviare: righe vuote, righe con ritorni a capo interni (sarebbero più
    /// comandi in volo) e `XHELP` (risposta su più righe senza `R`). Le righe ricevute che
    /// non iniziano con `R` vengono loggate e scartate. Su timeout/I/O la sessione diventa
    /// compromessa.
    pub async fn command(&mut self, cmd: &str, timeout: Option<Duration>) -> CmdOutcome {
        let line = cmd.trim();
        if self.broken {
            return CmdOutcome::failed(line, "Sessione compromessa: in attesa di riconnessione");
        }
        if line.is_empty() {
            return CmdOutcome::failed(line, "Comando vuoto");
        }
        if line.contains(['\n', '\r']) {
            return CmdOutcome::failed(line, "Il comando non può contenere ritorni a capo");
        }
        if line.to_ascii_uppercase().starts_with("XHELP") {
            return CmdOutcome::failed(
                line,
                "XHELP non è supportato: risponde su più righe senza R/OK",
            );
        }
        let to = timeout.unwrap_or_else(|| self.timeout_for(line));
        self.log(LogLevel::Tx, truncate(line, LOG_LINE_MAX), None);
        let start = Instant::now();
        if let Err(e) = self.transport.send_line(line).await {
            self.broken = true;
            let msg = format!("Invio fallito: {e}");
            self.log(LogLevel::Error, msg.clone(), None);
            return CmdOutcome::failed(line, msg);
        }
        let deadline = start + to;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self.transport.read_line(remaining).await {
                Ok(raw) => {
                    let latency = start.elapsed().as_millis() as u64;
                    match parse_response(&raw) {
                        Ok(resp) => {
                            self.log(LogLevel::Rx, truncate(&raw, LOG_LINE_MAX), Some(latency));
                            if line.eq_ignore_ascii_case(cmds::XQUIT) {
                                // la matrice chiude il canale dopo la risposta
                                self.broken = true;
                            }
                            return CmdOutcome {
                                command: line.to_string(),
                                response: Some(resp),
                                error: None,
                                latency_ms: latency,
                            };
                        }
                        Err(ParseError::Empty) => continue,
                        Err(e) => {
                            self.log(
                                LogLevel::Warn,
                                format!(
                                    "Riga inattesa scartata ({e}): {}",
                                    truncate(&raw, LOG_LINE_MAX)
                                ),
                                None,
                            );
                            continue;
                        }
                    }
                }
                Err(e) => {
                    self.broken = true;
                    let latency = start.elapsed().as_millis() as u64;
                    let msg = match e {
                        TransportError::Timeout => format!(
                            "Timeout: nessuna risposta a «{line}» entro {} ms (sessione compromessa)",
                            to.as_millis()
                        ),
                        other => format!("{other} (sessione compromessa)"),
                    };
                    self.log(LogLevel::Error, msg.clone(), Some(latency));
                    return CmdOutcome {
                        command: line.to_string(),
                        response: None,
                        error: Some(msg),
                        latency_ms: latency,
                    };
                }
            }
        }
    }

    async fn hs_req(&mut self, cmd: &str) -> Result<Response, HandshakeError> {
        let out = self.command(cmd, None).await;
        match (out.response, out.error) {
            (Some(r), None) => Ok(r),
            (_, Some(e)) => Err(HandshakeError::Session(e)),
            (None, None) => Err(HandshakeError::Session("nessuna risposta".into())),
        }
    }

    fn warn(&self, msg: String) {
        self.log(LogLevel::Warn, msg, None);
    }

    /// Handshake (spec §5.2, versione tollerante per uso operatore):
    /// `XVERSION` (deve essere `R…OK`), `XOPTIONS`, `XTYPE` (ripiego `XDCSTYPE`), `XMAXPORTS`
    /// (obbligatorio), `XMAXCARDS`, `XINSTALLED`, `XPORTCONFIG` (NB ≠ 0 → rifiuto), sonda
    /// `XLOS9999` (qualunque `ERnnnn` → `los_supported = false`, non è un errore).
    /// Il bit 3 di `XOPTIONS` a 0 produce solo un avviso nel log.
    pub async fn handshake(&mut self) -> Result<MatrixInfo, HandshakeError> {
        let mut info = MatrixInfo::default();

        let r = self.hs_req(cmds::XVERSION).await?;
        if !r.ok {
            return Err(HandshakeError::Rejected(format!(
                "XVERSION ha risposto «{}»: l'estremo non è un'API TLX",
                r.raw
            )));
        }
        info.version = info::parse_xversion(r.data_str());
        self.log(LogLevel::Info, format!("Versione API: {}", info.version), None);

        let r = self.hs_req(cmds::XOPTIONS).await?;
        if r.ok {
            match info::parse_xoptions(r.data_str()) {
                Ok(o) => info.set_options(o),
                Err(e) => self.warn(format!("XOPTIONS non interpretabile ({e}): opzioni ignorate")),
            }
        } else {
            self.warn(format!("XOPTIONS non supportato ({}): opzioni ignorate", r.raw));
        }
        if !info.real_card_count {
            self.warn(
                "XOPTIONS bit 3 = 0: XMAXCARDS potrebbe restituire metà delle schede \
                 (matrice VX o API V4). Si prosegue."
                    .into(),
            );
        }

        let r = self.hs_req(cmds::XTYPE).await?;
        if r.ok {
            info.xtype = info::parse_xtype(r.data_str());
        } else {
            let r2 = self.hs_req(cmds::XDCSTYPE).await?;
            if r2.ok {
                info.xtype = info::parse_xtype(r2.data_str());
            } else {
                self.warn("XTYPE/XDCSTYPE non supportati: modello sconosciuto".into());
            }
        }

        let r = self.hs_req(cmds::XMAXPORTS).await?;
        if !r.ok {
            return Err(HandshakeError::Rejected(format!("XMAXPORTS ha risposto «{}»", r.raw)));
        }
        info.max_ports = info::parse_xmaxports(r.data_str())
            .map_err(|e| HandshakeError::Rejected(format!("XMAXPORTS: {e}")))?;
        self.max_ports = info.max_ports;

        let r = self.hs_req(cmds::XMAXCARDS).await?;
        if r.ok {
            match info::parse_xmaxcards(r.data_str()) {
                Ok(n) => info.max_cards = n,
                Err(e) => self.warn(format!("XMAXCARDS: {e}")),
            }
        } else {
            self.warn(format!("XMAXCARDS non supportato ({})", r.raw));
        }

        let r = self.hs_req(cmds::XINSTALLED).await?;
        if r.ok {
            match info::parse_xinstalled(r.data_str()) {
                Ok(v) => {
                    if info.max_cards > 0 && v.len() != info.max_cards as usize {
                        self.warn(format!(
                            "XINSTALLED riporta {} slot ma XMAXCARDS ne dichiara {}",
                            v.len(),
                            info.max_cards
                        ));
                    }
                    let missing = v.iter().filter(|b| !**b).count();
                    if missing > 0 {
                        self.warn(format!("{missing} slot I/O senza scheda"));
                    }
                    info.installed = v;
                }
                Err(e) => self.warn(format!("XINSTALLED: {e}")),
            }
        } else {
            self.warn(format!("XINSTALLED non supportato ({})", r.raw));
        }

        let r = self.hs_req(cmds::XPORTCONFIG).await?;
        if r.ok {
            match info::parse_xportconfig(r.data_str()) {
                Ok(pc) => {
                    if pc.nb != 0 {
                        return Err(HandshakeError::Rejected(format!(
                            "XPORTCONFIG dichiara {} percorsi bidirezionali: non è una TLX",
                            pc.nb
                        )));
                    }
                    if pc.nu != info.max_ports {
                        self.warn(format!(
                            "XPORTCONFIG NU = {} diverso da XMAXPORTS = {}",
                            pc.nu, info.max_ports
                        ));
                    }
                }
                Err(e) => self.warn(format!("XPORTCONFIG: {e}")),
            }
        } else {
            self.warn(format!("XPORTCONFIG non supportato ({})", r.raw));
        }

        let r = self.hs_req(cmds::XLOS_ALL).await?;
        if r.ok {
            match info::parse_xlos(r.data_str(), Some(info.max_ports as usize)) {
                Ok(_) => info.los_supported = Some(true),
                Err(e) => {
                    self.warn(format!("XLOS9999 con dati inattesi ({e}): diagnostica LOS sospesa"));
                    info.los_supported = Some(false);
                }
            }
        } else {
            self.log(
                LogLevel::Info,
                format!(
                    "XLOS9999 → {}: firmware senza comandi Rev. M, diagnostica ottica non disponibile",
                    r.raw
                ),
                None,
            );
            info.los_supported = Some(false);
        }

        self.log(
            LogLevel::Info,
            format!(
                "Handshake completato: {} {}, {} porte, {} schede",
                if info.xtype.is_empty() { "TLX" } else { &info.xtype },
                info.version,
                info.max_ports,
                info.max_cards
            ),
            None,
        );
        self.info = Some(info.clone());
        Ok(info)
    }

    /// Chiude la sessione. Su TCP invia prima `XQUIT` (esito ignorato, attesa ≤ 500 ms).
    pub async fn close(mut self) {
        if !self.broken && !self.transport.is_serial() {
            let _ = self.command(cmds::XQUIT, Some(Duration::from_millis(500))).await;
        }
        self.transport.shutdown().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcome_helpers() {
        let o = CmdOutcome {
            command: "CI0004O0087".into(),
            response: Some(parse_response("R0007ER0007").unwrap()),
            error: None,
            latency_ms: 3,
        };
        assert!(!o.ok());
        assert_eq!(o.error_code(), Some("0007"));
        assert!(o.error_message().unwrap().contains("ER0007"));
        let o = CmdOutcome::failed("XGET", "Matrice non connessa");
        assert!(!o.ok());
        assert_eq!(o.error_message().as_deref(), Some("Matrice non connessa"));
    }
}
