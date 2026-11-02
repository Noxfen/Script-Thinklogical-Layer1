//! Tipi degli eventi emessi dal client (stato connessione, info, routing, log).
//! Riesportati da [`crate::client`].

use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::protocol::{MatrixInfo, Routing};

/// Stato della connessione.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnStatus {
    /// Nessuna sessione (dopo `shutdown`).
    Disconnected,
    /// Apertura del canale in corso.
    Connecting,
    /// Canale aperto, handshake in corso.
    Handshaking,
    /// Sessione pronta.
    Connected,
    /// Sessione persa, in attesa del prossimo tentativo (backoff).
    Reconnecting,
    /// Ultimo tentativo fallito in modo non transitorio (es. handshake rifiutato);
    /// il client continua comunque a riprovare.
    Error,
}

impl fmt::Display for ConnStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ConnStatus::Disconnected => "disconnected",
            ConnStatus::Connecting => "connecting",
            ConnStatus::Handshaking => "handshaking",
            ConnStatus::Connected => "connected",
            ConnStatus::Reconnecting => "reconnecting",
            ConnStatus::Error => "error",
        })
    }
}

/// Stato della connessione con estremo, ultimo errore e istante del cambio.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnState {
    /// Stato.
    pub status: ConnStatus,
    /// Estremo (`192.168.13.15:17567`, `COM3 @ 9600`).
    pub endpoint: String,
    /// Ultimo errore (in italiano), se presente.
    pub error: Option<String>,
    /// Istante del cambio di stato (ms epoch).
    pub since_ms: u64,
}

/// Origine di un aggiornamento della routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RoutingSource {
    /// Lettura `XGET` (autoritativa).
    Xget,
    /// Datagramma UDP 17564.
    Udp,
    /// Applicazione locale di un comando di commutazione accettato (`OK`): **non verificata**.
    Local,
}

impl fmt::Display for RoutingSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            RoutingSource::Xget => "xget",
            RoutingSource::Udp => "udp",
            RoutingSource::Local => "local",
        })
    }
}

/// Routing con origine e istante di aggiornamento.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutingUpdate {
    /// Stato degli incroci.
    pub routing: Routing,
    /// Origine.
    pub source: RoutingSource,
    /// Istante (ms epoch).
    pub updated_at_ms: u64,
}

/// Livello di una voce di log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    /// Informazione.
    Info,
    /// Avviso.
    Warn,
    /// Errore.
    Error,
    /// Riga inviata alla matrice.
    Tx,
    /// Riga ricevuta dalla matrice.
    Rx,
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            LogLevel::Info => "info",
            LogLevel::Warn => "warn",
            LogLevel::Error => "error",
            LogLevel::Tx => "tx",
            LogLevel::Rx => "rx",
        })
    }
}

/// Canale di una voce di log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogChannel {
    /// Sessione TCP.
    Tcp,
    /// Sessione seriale.
    Serial,
    /// Ricevitore UDP 17564.
    Udp,
    /// Logica del client.
    App,
}

impl fmt::Display for LogChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            LogChannel::Tcp => "tcp",
            LogChannel::Serial => "serial",
            LogChannel::Udp => "udp",
            LogChannel::App => "app",
        })
    }
}

/// Voce di log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogRecord {
    /// Istante (ms epoch).
    pub ts_ms: u64,
    /// Livello.
    pub level: LogLevel,
    /// Canale.
    pub channel: LogChannel,
    /// Testo (righe TX/RX troncate a 512 caratteri).
    pub message: String,
    /// Latenza del comando (solo per le righe RX di risposta).
    pub latency_ms: Option<u64>,
}

/// Evento del client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "camelCase")]
pub enum ClientEvent {
    /// Cambio di stato della connessione.
    Conn(ConnState),
    /// Profilo della matrice (dopo l'handshake o quando cambia `losSupported`).
    Info(MatrixInfo),
    /// Nuova routing.
    Routing(RoutingUpdate),
    /// Voce di log.
    Log(LogRecord),
}

/// Destinazione delle voci di log prodotte dalla sessione.
pub type LogSink = Arc<dyn Fn(LogRecord) + Send + Sync>;
