//! Client ad alto livello, thread-safe, per la GUI.
//!
//! [`MatrixClient`] è un handle clonabile verso un task *attore* che possiede la
//! [`Session`] e serializza tutti i comandi in una coda (**un solo comando in volo**,
//! spec §2.3, §5.1). L'attore:
//! * apre il canale, esegue l'handshake e una prima `XGET`;
//! * a coda vuota invia `XLASTEVENT` ogni `heartbeat_s` e, se il valore cambia, rilegge con `XGET`;
//! * se non arrivano datagrammi UDP da `bcast_missing_periods` periodi, lo segnala una volta
//!   nel log e rilegge con `XGET` ogni `poll_xget_s`;
//! * se la sessione si rompe passa a `reconnecting` e riprova **per sempre** con backoff
//!   (`[1, 2, 5, 10]` s, poi sempre l'ultimo valore); i comandi andati in timeout **non**
//!   vengono mai ripetuti; mentre è disconnesso i comandi falliscono subito con
//!   «Matrice non connessa».
//!
//! Un task separato ascolta UDP 17564 (se configurato) e aggiorna la routing in cache, con la
//! regola di precedenza di §5.4: i datagrammi ricevuti entro un periodo di broadcast dopo
//! una `XGET` o una commutazione vengono ignorati. Il task UDP appartiene al client (non
//! all'attore): è [`MatrixClient::shutdown`] che lo spegne e libera la porta 17564.
//!
//! Un terzo task sorveglia l'attore: se termina senza che sia stato chiesto un arresto
//! (errore non previsto, panic in debug) lo stato passa a `error` con la causa e l'attore
//! viene riavviato, perché una finestra che dice «connesso» ma non comanda più niente è la
//! condizione più pericolosa in sala controllo.
//!
//! Tutto ciò che accade è notificato con [`ClientEvent`] su un canale `broadcast`.

use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::task::{AbortHandle, JoinHandle};
use tokio::time::Instant;

pub use crate::events::{
    ClientEvent, ConnState, ConnStatus, LogChannel, LogLevel, LogRecord, LogSink, RoutingSource,
    RoutingUpdate,
};
use crate::protocol::bcast::{parse_17564, BcastAssembler, BCAST_PORT};
use crate::protocol::command::{self as cmds, parse_command, CmdError, ParsedCommand};
use crate::protocol::info::{parse_xalarm, parse_xlos, parse_xrxpwr, parse_xtxpwr};
use crate::protocol::{error_description, MatrixInfo, Routing};
use crate::session::{CmdOutcome, HandshakeError, Session, SessionOptions};
use crate::transport::TransportConfig;
use crate::util::now_ms;

/// Messaggio dei comandi rifiutati mentre la matrice non è connessa.
pub const NOT_CONNECTED: &str = "Matrice non connessa";

/// Configurazione del ricevitore UDP 17564.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UdpConfig {
    /// Porta di ascolto (17564; configurabile per i test).
    pub bind_port: u16,
    /// Gruppo multicast da agganciare (es. 239.255.13.9); `None` = solo broadcast.
    pub mcast_group: Option<Ipv4Addr>,
    /// Mittenti accettati (IP sorgente **o** IP nel datagramma). Se vuoto e il canale di
    /// comando è TCP verso un indirizzo IPv4, il client lo riempie da solo con
    /// `{X, X+100, X+1}` (virtuale, primario, backup: spec §1.2, §5.4); se l'host è un nome
    /// DNS o il canale è seriale resta vuoto, cioè **qualunque mittente**, e il client lo
    /// segnala come avviso.
    pub accept_from: Vec<Ipv4Addr>,
}

impl Default for UdpConfig {
    fn default() -> Self {
        UdpConfig { bind_port: BCAST_PORT, mcast_group: None, accept_from: Vec::new() }
    }
}

/// Configurazione del client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ClientConfig {
    /// Canale di comando.
    pub transport: TransportConfig,
    /// Timeout di apertura (usato se il trasporto TCP indica 0) (3000 ms).
    pub connect_timeout_ms: u64,
    /// Timeout dei comandi ordinari (2000 ms).
    pub cmd_timeout_ms: u64,
    /// Base dei comandi di stato, + 10 ms × N (2000 ms).
    pub status_timeout_base_ms: u64,
    /// Heartbeat `XLASTEVENT` a coda vuota (15 s; 0 = disattivato).
    pub heartbeat_s: u64,
    /// `XGET` periodica quando l'UDP tace da 3 periodi (60 s; 0 = disattivata).
    pub poll_xget_s: u64,
    /// Backoff di riconnessione in secondi; dopo l'ultimo valore si ripete l'ultimo.
    pub reconnect_backoff_s: Vec<u64>,
    /// `--delay` della matrice (300 ms): timeout dei Macro e attesa prima delle riletture.
    pub delay_ms: u64,
    /// Periodo di broadcast UDP assunto (4000 ms): finestra di precedenza e di ricomposizione.
    pub bcast_period_ms: u64,
    /// Periodi di broadcast senza datagrammi oltre i quali si considera morto il broadcast
    /// e si passa alla rilettura periodica con `XGET` (spec §5.4, `bcast_missing_periods`: 3).
    pub bcast_missing_periods: u32,
    /// Ricevitore UDP 17564 (`None` = disattivato).
    pub udp: Option<UdpConfig>,
}

impl Default for ClientConfig {
    fn default() -> Self {
        ClientConfig {
            transport: TransportConfig::default(),
            connect_timeout_ms: 3000,
            cmd_timeout_ms: 2000,
            status_timeout_base_ms: 2000,
            heartbeat_s: 15,
            poll_xget_s: 60,
            reconnect_backoff_s: vec![1, 2, 5, 10],
            delay_ms: 300,
            bcast_period_ms: 4000,
            bcast_missing_periods: 3,
            udp: None,
        }
    }
}

/// Riporta un valore di configurazione nell'intervallo ammesso e, se lo ha corretto,
/// aggiunge la nota da scrivere nel log.
fn limita(nome: &str, v: &mut u64, min: u64, max: u64, unita: &str, note: &mut Vec<String>) {
    let nuovo = (*v).clamp(min, max);
    if nuovo != *v {
        note.push(format!(
            "Configurazione: {nome} = {v} {unita} fuori dall'intervallo ammesso ({min}–{max} {unita}): si usa {nuovo} {unita}"
        ));
        *v = nuovo;
    }
}

impl ClientConfig {
    /// Copia della configurazione con i valori riportati in intervalli sensati, più
    /// l'elenco delle correzioni da scrivere nel log.
    ///
    /// `config.json` è un file modificabile a mano: un valore assurdo non deve poter
    /// chiudere l'applicazione (`heartbeat_s` enorme finirebbe in `Instant + Duration`,
    /// che va in panic in overflow) né tempestare la matrice di tentativi di
    /// riconnessione (`reconnect_backoff_s` a 0). Non usa mai `unwrap`/`expect`.
    pub fn sanitized(&self) -> (ClientConfig, Vec<String>) {
        let mut c = self.clone();
        let mut note = Vec::new();
        if c.connect_timeout_ms == 0 {
            note.push(
                "Configurazione: connectTimeoutMs = 0 non significa «nessun timeout»: si usano 3000 ms \
                 per l'apertura del canale"
                    .to_string(),
            );
            c.connect_timeout_ms = 3000;
        }
        limita("connectTimeoutMs", &mut c.connect_timeout_ms, 200, 120_000, "ms", &mut note);
        limita("cmdTimeoutMs", &mut c.cmd_timeout_ms, 200, 120_000, "ms", &mut note);
        limita("statusTimeoutBaseMs", &mut c.status_timeout_base_ms, 200, 120_000, "ms", &mut note);
        limita("delayMs", &mut c.delay_ms, 0, 10_000, "ms", &mut note);
        limita("bcastPeriodMs", &mut c.bcast_period_ms, 100, 60_000, "ms", &mut note);
        if c.heartbeat_s != 0 {
            limita("heartbeatS", &mut c.heartbeat_s, 1, 3600, "s", &mut note);
        }
        if c.poll_xget_s != 0 {
            limita("pollXgetS", &mut c.poll_xget_s, 1, 3600, "s", &mut note);
        }
        // Nel trasporto TCP 0 significa «usa connectTimeoutMs»: si corregge solo il resto.
        if let TransportConfig::Tcp { connect_timeout_ms, .. } = &mut c.transport {
            if *connect_timeout_ms != 0 {
                limita(
                    "transport.connectTimeoutMs",
                    connect_timeout_ms,
                    200,
                    120_000,
                    "ms",
                    &mut note,
                );
            }
        }
        if c.reconnect_backoff_s.is_empty() {
            note.push(
                "Configurazione: reconnectBackoffS vuoto: si usano i valori di specifica 1, 2, 5, 10 s"
                    .to_string(),
            );
            c.reconnect_backoff_s = vec![1, 2, 5, 10];
        } else {
            for i in 0..c.reconnect_backoff_s.len() {
                let v = c.reconnect_backoff_s[i];
                let nuovo = v.clamp(1, 600);
                if nuovo != v {
                    note.push(format!(
                        "Configurazione: reconnectBackoffS[{i}] = {v} s fuori dall'intervallo ammesso (1–600 s): \
                         si usa {nuovo} s; un'attesa più breve tempesterebbe la matrice di tentativi"
                    ));
                    c.reconnect_backoff_s[i] = nuovo;
                }
            }
        }
        let periodi = c.bcast_missing_periods.clamp(1, 100);
        if periodi != c.bcast_missing_periods {
            note.push(format!(
                "Configurazione: bcastMissingPeriods = {} fuori dall'intervallo ammesso (1–100): si usa {periodi}",
                c.bcast_missing_periods
            ));
            c.bcast_missing_periods = periodi;
        }
        (c, note)
    }

    /// Durata del comando più lungo che la matrice può tenere in volo (`XGET` su TLX1280,
    /// Macro da 50 coppie), più il margine della chiusura: è l'attesa massima della risposta
    /// a uno `shutdown`, altrimenti si abortirebbe l'attore mentre un comando è ancora in corso.
    pub fn max_cmd_timeout(&self) -> Duration {
        let n = 1280u64; // peggior caso: non sappiamo ancora quante porte ha la matrice
        let xget = self.status_timeout_base_ms.saturating_add(10u64.saturating_mul(n));
        let macro_50 = self.cmd_timeout_ms.saturating_add(50u64.saturating_mul(self.delay_ms));
        let ms = xget.max(macro_50).saturating_add(self.delay_ms).saturating_add(1000);
        Duration::from_millis(ms.clamp(1500, 60_000))
    }
}

/// Diagnostica ottica e allarmi. `None` = non disponibile (firmware senza Rev. M,
/// `ERnnnn`, dati non interpretabili).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostics {
    /// Bitmap `XALARM` (0 = nessun allarme).
    pub alarm: Option<u16>,
    /// LOS per porta (`true` = nessuna luce in ingresso).
    pub los: Option<Vec<bool>>,
    /// Potenza ricevuta per porta (µW).
    pub rx_uw: Option<Vec<u16>>,
    /// Potenza trasmessa per porta (µW).
    pub tx_uw: Option<Vec<u16>>,
}

/// Errore dei metodi del client (messaggi in italiano, mostrabili).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ClientError {
    /// Nessuna sessione pronta.
    #[error("Matrice non connessa")]
    NotConnected,
    /// Comando rifiutato dal validatore TT prima dell'invio.
    #[error("Comando non valido: {0}")]
    Invalid(#[from] CmdError),
    /// La matrice ha risposto `ERnnnn`, oppure timeout/errore di trasporto.
    #[error("{0}")]
    Command(String),
    /// Risposta `OK` con dati non interpretabili.
    #[error("Risposta non interpretabile: {0}")]
    Parse(String),
    /// Il client è stato arrestato.
    #[error("Client arrestato")]
    Stopped,
}

enum Request {
    Cmd { line: String, reply: oneshot::Sender<CmdOutcome> },
    Reconnect,
    Shutdown { reply: oneshot::Sender<()> },
}

// ---------------------------------------------------------------------------
// Stato condiviso
// ---------------------------------------------------------------------------

struct SharedState {
    conn: ConnState,
    info: Option<MatrixInfo>,
    routing: Option<RoutingUpdate>,
    quiet_until: Option<Instant>,
    last_udp: Option<Instant>,
    udp_size_warned: bool,
    /// `true` dopo l'avviso «il broadcast tace»: l'avviso non si ripete a ogni periodo.
    udp_missing_warned: bool,
}

struct Shared {
    events: broadcast::Sender<ClientEvent>,
    st: Mutex<SharedState>,
    bcast_period: Duration,
    /// Periodi di silenzio oltre i quali il broadcast è considerato morto (spec §5.4).
    bcast_missing_periods: u32,
    /// Porta di ascolto UDP, se il ricevitore è configurato (serve nei messaggi di log).
    udp_port: Option<u16>,
    /// Avvio del client: riferimento per il silenzio quando non è mai arrivato nulla.
    udp_since: Instant,
    /// `true` quando l'arresto è stato chiesto: la sorveglianza non riavvia più l'attore.
    stopping: AtomicBool,
    /// Attore in esecuzione, per abortirlo in `shutdown`.
    actor: Mutex<Option<AbortHandle>>,
}

impl Shared {
    fn lock(&self) -> std::sync::MutexGuard<'_, SharedState> {
        self.st.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn emit(&self, ev: ClientEvent) {
        let _ = self.events.send(ev);
    }

    fn log(
        &self,
        level: LogLevel,
        channel: LogChannel,
        message: impl Into<String>,
        latency_ms: Option<u64>,
    ) {
        self.emit(ClientEvent::Log(LogRecord {
            ts_ms: now_ms(),
            level,
            channel,
            message: message.into(),
            latency_ms,
        }));
    }

    fn set_conn(&self, status: ConnStatus, error: Option<String>) {
        let st = {
            let mut g = self.lock();
            g.conn.status = status;
            g.conn.error = error;
            g.conn.since_ms = now_ms();
            g.conn.clone()
        };
        self.emit(ClientEvent::Conn(st));
    }

    fn set_info(&self, info: MatrixInfo) {
        self.lock().info = Some(info.clone());
        self.emit(ClientEvent::Info(info));
    }

    fn mark_los_unsupported(&self) {
        let changed = {
            let mut g = self.lock();
            match g.info.as_mut() {
                Some(i) if i.los_supported != Some(false) => {
                    i.los_supported = Some(false);
                    Some(i.clone())
                }
                _ => None,
            }
        };
        if let Some(i) = changed {
            self.log(
                LogLevel::Warn,
                LogChannel::App,
                "Comandi Rev. M non supportati: diagnostica ottica disattivata",
                None,
            );
            self.emit(ClientEvent::Info(i));
        }
    }

    fn quiet(&self) {
        self.lock().quiet_until = Some(fra(Instant::now(), self.bcast_period));
    }

    /// Registra l'attore corrente (la sorveglianza lo sostituisce a ogni riavvio).
    fn set_actor(&self, h: AbortHandle) {
        *self.actor.lock().unwrap_or_else(|p| p.into_inner()) = Some(h);
    }

    fn set_routing(&self, routing: Routing, source: RoutingSource) {
        let upd = RoutingUpdate { routing, source, updated_at_ms: now_ms() };
        self.lock().routing = Some(upd.clone());
        self.emit(ClientEvent::Routing(upd));
    }

    /// Da quanto tempo non arriva un datagramma di stato (dall'avvio, se non ne è mai
    /// arrivato nessuno).
    fn udp_silence(&self) -> Duration {
        match self.lock().last_udp {
            Some(t) => t.elapsed(),
            None => self.udp_since.elapsed(),
        }
    }

    fn udp_fresh(&self) -> bool {
        let window = self.bcast_period * self.bcast_missing_periods;
        self.lock().last_udp.is_some_and(|t| t.elapsed() < window)
    }

    /// Sorveglianza del broadcast (spec §5.4, *Watchdog*): **un solo** avviso quando i
    /// datagrammi smettono di arrivare e uno quando tornano. Senza questo avviso la
    /// latenza dello stato passa da un periodo a `poll_xget_s` in silenzio.
    fn udp_watchdog(&self) {
        let silenzio = self.udp_silence();
        let morto = silenzio >= self.bcast_period * self.bcast_missing_periods;
        let transizione = {
            let mut g = self.lock();
            if morto && !g.udp_missing_warned {
                g.udp_missing_warned = true;
                Some(true)
            } else if !morto && g.udp_missing_warned {
                g.udp_missing_warned = false;
                Some(false)
            } else {
                None
            }
        };
        match transizione {
            Some(true) => self.log(
                LogLevel::Warn,
                LogChannel::Udp,
                format!(
                    "Nessun datagramma di stato sulla porta UDP {} da {} s: si passa alla rilettura periodica \
                     con XGET, quindi una commutazione fatta da un altro controllore può comparire con \
                     decine di secondi di ritardo. Causa tipica su Windows: il firewall blocca la porta {} \
                     in ingresso (l'ascolto riesce comunque, per questo non c'è altro errore).",
                    self.udp_port.unwrap_or(BCAST_PORT),
                    silenzio.as_secs(),
                    self.udp_port.unwrap_or(BCAST_PORT),
                ),
                None,
            ),
            Some(false) => self.log(
                LogLevel::Info,
                LogChannel::Udp,
                "Datagrammi di stato UDP di nuovo in arrivo: lo stato torna ad aggiornarsi a ogni periodo \
                 di broadcast",
                None,
            ),
            None => {}
        }
    }

    /// Avvisa **una volta** che arrivano datagrammi di una matrice che non è la nostra.
    fn warn_udp_altra_matrice(&self, from: SocketAddr, fino_a: u16, nostre: u16) {
        let primo = {
            let mut g = self.lock();
            let p = !g.udp_size_warned;
            g.udp_size_warned = true;
            p
        };
        if primo {
            self.log(
                LogLevel::Warn,
                LogChannel::Udp,
                format!(
                    "Datagramma di stato da {from} con porte fino a {fino_a}, la nostra matrice ne ha {nostre}: \
                     non è la nostra matrice, i suoi datagrammi vengono ignorati"
                ),
                None,
            );
        }
    }

    /// Registra l'arrivo di un datagramma **accettato**, non di una fotografia composta.
    ///
    /// L'assemblatore non emette niente finché l'handshake non ha letto `XMAXPORTS` (e sul
    /// TLX1280 finché non sono arrivati entrambi i blocchi), mentre il ricevitore UDP parte
    /// prima della sessione TCP: contare le fotografie farebbe segnalare un «broadcast
    /// assente» inesistente a ogni avvio.
    fn note_udp(&self) {
        self.lock().last_udp = Some(Instant::now());
    }

    fn on_udp_routing(&self, r: Routing) {
        let now = Instant::now();
        let (cambiate, era_comandato) = {
            let g = self.lock();
            if g.quiet_until.is_some_and(|q| now < q) {
                return;
            }
            match &g.routing {
                Some(cur) => (cur.routing.diff(&r).len(), cur.source == RoutingSource::Local),
                None => (r.max_ports as usize, false),
            }
        };
        if cambiate > 0 {
            self.log(
                LogLevel::Info,
                LogChannel::Udp,
                format!("Stato aggiornato dal broadcast UDP: {cambiate} uscite cambiate"),
                None,
            );
        } else if era_comandato {
            // Spec §5.3 punto 5 e §5.4: all'operatore si riporta lo stato **verificato**,
            // mai quello «comandato». Il datagramma che coincide con il comando è la
            // conferma della matrice: va pubblicato anche se non cambia nessuna uscita,
            // altrimenti la finestra resta su «comandato» e invecchia all'infinito.
            self.log(
                LogLevel::Info,
                LogChannel::Udp,
                "Commutazione confermata dal broadcast UDP: lo stato mostrato è verificato",
                None,
            );
        }
        if cambiate > 0 || era_comandato {
            self.set_routing(r, RoutingSource::Udp);
        }
    }

    fn logger(self: &Arc<Self>) -> LogSink {
        let me = self.clone();
        Arc::new(move |rec: LogRecord| me.emit(ClientEvent::Log(rec)))
    }
}

// ---------------------------------------------------------------------------
// Handle pubblico
// ---------------------------------------------------------------------------

struct Inner {
    tx: mpsc::Sender<Request>,
    shared: Arc<Shared>,
    /// Task di sorveglianza (possiede l'attore e lo riavvia).
    supervisor: Mutex<Option<JoinHandle<()>>>,
    /// Task di fondo: il ricevitore UDP 17564. Deve stare qui e non dentro l'attore,
    /// altrimenti un attore abortito non lo ferma mai e la porta 17564 resta legata.
    background: Mutex<Vec<JoinHandle<()>>>,
    /// Attesa massima della risposta allo `shutdown` (comando più lungo possibile).
    shutdown_wait: Duration,
}

impl Drop for Inner {
    /// Quando l'ultima copia dell'handle viene distrutta l'attore termina da solo
    /// (canale chiuso), ma i task di fondo no: un `JoinHandle` lasciato cadere non
    /// ferma il task. Senza questo il ricevitore UDP resterebbe legato alla porta.
    fn drop(&mut self) {
        let tasks = std::mem::take(&mut *self.background.lock().unwrap_or_else(|p| p.into_inner()));
        for t in tasks {
            t.abort();
        }
    }
}

/// Handle verso una matrice. Clonabile; tutte le copie condividono la stessa sessione.
/// Quando l'ultima copia viene distrutta l'attore chiude la sessione e termina.
#[derive(Clone)]
pub struct MatrixClient {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for MatrixClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MatrixClient").field("conn", &self.conn_state()).finish()
    }
}

impl MatrixClient {
    /// Avvia il client e ritorna subito (sincrono). Connessione, handshake, `XGET` iniziale e
    /// riconnessioni avvengono in background e sono notificati con gli eventi.
    /// Va chiamato all'interno di un runtime tokio.
    pub fn connect(cfg: ClientConfig) -> MatrixClient {
        let (cfg, correzioni) = cfg.sanitized();
        let shutdown_wait = cfg.max_cmd_timeout();
        let (events, _) = broadcast::channel(1024);
        let shared = Arc::new(Shared {
            events,
            st: Mutex::new(SharedState {
                conn: ConnState {
                    status: ConnStatus::Connecting,
                    endpoint: cfg.transport.endpoint(),
                    error: None,
                    since_ms: now_ms(),
                },
                info: None,
                routing: None,
                quiet_until: None,
                last_udp: None,
                udp_size_warned: false,
                udp_missing_warned: false,
            }),
            bcast_period: Duration::from_millis(cfg.bcast_period_ms.max(100)),
            bcast_missing_periods: cfg.bcast_missing_periods.max(1),
            udp_port: cfg.udp.as_ref().map(|u| u.bind_port),
            udp_since: Instant::now(),
            stopping: AtomicBool::new(false),
            actor: Mutex::new(None),
        });
        for c in correzioni {
            shared.log(LogLevel::Warn, LogChannel::App, c, None);
        }
        // Il ricevitore UDP è un task del client, non dell'attore: solo così `shutdown`
        // può fermarlo e liberare la porta 17564.
        let mut background = Vec::new();
        if let Some(mut u) = cfg.udp.clone() {
            let (livello, msg) = derive_accept_from(&mut u, &cfg.transport);
            shared.log(livello, LogChannel::Udp, msg, None);
            background.push(tokio::spawn(udp_listener(u, shared.clone())));
        }
        let (tx, rx) = mpsc::channel(64);
        let rx = Arc::new(tokio::sync::Mutex::new(rx));
        let supervisor = tokio::spawn(supervise(cfg, shared.clone(), rx));
        MatrixClient {
            inner: Arc::new(Inner {
                tx,
                shared,
                supervisor: Mutex::new(Some(supervisor)),
                background: Mutex::new(background),
                shutdown_wait,
            }),
        }
    }

    /// Iscrizione agli eventi (gli eventi precedenti non vengono ripetuti: leggere lo stato
    /// corrente con [`conn_state`](Self::conn_state), [`info`](Self::info),
    /// [`routing`](Self::routing)).
    pub fn subscribe(&self) -> broadcast::Receiver<ClientEvent> {
        self.inner.shared.events.subscribe()
    }

    /// Stato corrente della connessione.
    pub fn conn_state(&self) -> ConnState {
        self.inner.shared.lock().conn.clone()
    }

    /// `true` se la sessione è pronta.
    pub fn is_connected(&self) -> bool {
        self.conn_state().status == ConnStatus::Connected
    }

    /// Profilo della matrice (dall'ultimo handshake).
    pub fn info(&self) -> Option<MatrixInfo> {
        self.inner.shared.lock().info.clone()
    }

    /// Ultima routing nota (cache: `xget`, `udp` o `local`).
    pub fn routing(&self) -> Option<RoutingUpdate> {
        self.inner.shared.lock().routing.clone()
    }

    fn max_ports(&self) -> Result<u16, ClientError> {
        if !self.is_connected() {
            return Err(ClientError::NotConnected);
        }
        self.info().map(|i| i.max_ports).filter(|n| *n > 0).ok_or(ClientError::NotConnected)
    }

    /// Attende lo stato `connected` per al massimo `timeout`.
    pub async fn wait_connected(&self, timeout: Duration) -> Result<(), ClientError> {
        let mut rx = self.subscribe();
        let deadline = Instant::now() + timeout;
        loop {
            let st = self.conn_state();
            if st.status == ConnStatus::Connected {
                return Ok(());
            }
            if st.status == ConnStatus::Disconnected && self.inner.tx.is_closed() {
                return Err(ClientError::Stopped);
            }
            match tokio::time::timeout_at(deadline, rx.recv()).await {
                Err(_) => {
                    return Err(ClientError::Command(format!(
                        "Connessione non riuscita entro {} ms{}",
                        timeout.as_millis(),
                        st.error.map(|e| format!(": {e}")).unwrap_or_default()
                    )))
                }
                Ok(Err(broadcast::error::RecvError::Closed)) => return Err(ClientError::Stopped),
                Ok(_) => continue,
            }
        }
    }

    /// Invia **esattamente** la riga indicata (nessuna validazione né conversione in
    /// maiuscolo) attraverso la coda. Non fallisce mai: gli errori sono nel `CmdOutcome`
    /// (`error` = trasporto/timeout/non connessa, `response.ok == false` = `ERnnnn`).
    /// Le commutazioni riuscite aggiornano la routing in cache (origine `local`),
    /// `XGET` la aggiorna con origine `xget`.
    pub async fn raw(&self, line: &str) -> CmdOutcome {
        let (reply, rx) = oneshot::channel();
        if self.inner.tx.send(Request::Cmd { line: line.to_string(), reply }).await.is_err() {
            return CmdOutcome::failed(line.trim(), "Client arrestato");
        }
        rx.await.unwrap_or_else(|_| CmdOutcome::failed(line.trim(), "Client arrestato"))
    }

    async fn checked(&self, line: String) -> Result<CmdOutcome, ClientError> {
        let out = self.raw(&line).await;
        if out.error.as_deref() == Some(NOT_CONNECTED) {
            return Err(ClientError::NotConnected);
        }
        Ok(out)
    }

    /// Legge lo stato completo con `XGET` (aggiorna anche la cache).
    pub async fn refresh(&self) -> Result<Routing, ClientError> {
        let out = self.raw(cmds::XGET).await;
        if out.error.as_deref() == Some(NOT_CONNECTED) {
            return Err(ClientError::NotConnected);
        }
        if !out.ok() {
            return Err(ClientError::Command(out.error_message().unwrap_or_default()));
        }
        let n = self.info().map(|i| i.max_ports).unwrap_or(0);
        Routing::from_xget(out.data().unwrap_or(""), n)
            .map_err(|e| ClientError::Parse(e.to_string()))
    }

    /// `CI`: collega `input` alle `outputs` (validazione TT). `Ok` anche su `ERnnnn`:
    /// controllare [`CmdOutcome::ok`].
    pub async fn ci(&self, input: u16, outputs: &[u16]) -> Result<CmdOutcome, ClientError> {
        let line = cmds::ci(self.max_ports()?, input, outputs)?;
        self.checked(line).await
    }

    /// `DI`: scollega tutte le uscite degli ingressi indicati.
    pub async fn di(&self, inputs: &[u16]) -> Result<CmdOutcome, ClientError> {
        let line = cmds::di(self.max_ports()?, inputs)?;
        self.checked(line).await
    }

    /// `DO`: scollega le uscite indicate.
    pub async fn do_(&self, outputs: &[u16]) -> Result<CmdOutcome, ClientError> {
        let line = cmds::do_(self.max_ports()?, outputs)?;
        self.checked(line).await
    }

    /// `DI9999`: scollega tutto (chiedere conferma all'operatore prima).
    pub async fn di_all(&self) -> Result<CmdOutcome, ClientError> {
        self.max_ports()?;
        self.checked(cmds::di_all()).await
    }

    /// Macro: spezzato in blocchi da ≤ 50 coppie inviati in ordine; ci si ferma al primo
    /// blocco non `OK` (i successivi non vengono inviati). Restituisce un esito per blocco
    /// inviato.
    pub async fn macro_(&self, pairs: &[(u16, u16)]) -> Result<Vec<CmdOutcome>, ClientError> {
        let chunks = cmds::macro_chunks(self.max_ports()?, pairs)?;
        let mut v = Vec::with_capacity(chunks.len());
        for c in chunks {
            let out = self.checked(c).await?;
            let good = out.ok();
            v.push(out);
            if !good {
                break;
            }
        }
        Ok(v)
    }

    /// Diagnostica: `XALARM`, poi `XLOS9999`, `XRXPWR9999`, `XTXPWR9999` (saltati se il
    /// firmware non ha i comandi Rev. M). Errore solo se la sessione non è disponibile.
    pub async fn diagnostics(&self) -> Result<Diagnostics, ClientError> {
        let n = self.max_ports()? as usize;
        let mut d = Diagnostics::default();
        let transport_err = |o: &CmdOutcome| -> Option<ClientError> {
            o.error.as_ref().map(|e| {
                if e == NOT_CONNECTED {
                    ClientError::NotConnected
                } else {
                    ClientError::Command(e.clone())
                }
            })
        };
        let out = self.raw(cmds::XALARM).await;
        if let Some(e) = transport_err(&out) {
            return Err(e);
        }
        if out.ok() {
            d.alarm = parse_xalarm(out.data().unwrap_or("")).ok();
        }
        let los_ok = || self.info().and_then(|i| i.los_supported) != Some(false);
        if los_ok() {
            let out = self.raw(cmds::XLOS_ALL).await;
            if let Some(e) = transport_err(&out) {
                return Err(e);
            }
            if out.ok() {
                d.los = parse_xlos(out.data().unwrap_or(""), Some(n)).ok();
            }
        }
        if los_ok() {
            let out = self.raw(cmds::XRXPWR_ALL).await;
            if let Some(e) = transport_err(&out) {
                return Err(e);
            }
            if out.ok() {
                d.rx_uw = parse_xrxpwr(out.data().unwrap_or(""), Some(n)).ok();
            }
        }
        if los_ok() {
            let out = self.raw(cmds::XTXPWR_ALL).await;
            if let Some(e) = transport_err(&out) {
                return Err(e);
            }
            if out.ok() {
                d.tx_uw = parse_xtxpwr(out.data().unwrap_or(""), Some(n)).ok();
            }
        }
        Ok(d)
    }

    /// Forza un nuovo tentativo di connessione immediato (chiude la sessione se aperta).
    pub async fn reconnect_now(&self) {
        let _ = self.inner.tx.send(Request::Reconnect).await;
    }

    /// Arresta il client: `XQUIT` su TCP, chiusura della sessione, stop di tutti i task.
    /// Idempotente.
    ///
    /// L'attesa della risposta dell'attore arriva fino al comando più lungo possibile
    /// (`XGET` su TLX1280 ≈ 15 s): l'attore preleva lo `Shutdown` solo fra due comandi.
    /// I task di fondo (ricevitore UDP 17564) vengono fermati **prima** dell'attesa, così
    /// la porta resta libera e lo stato non viene più sovrascritto anche se il chiamante
    /// annulla questa attesa.
    pub async fn shutdown(&self) {
        let (reply, rx) = oneshot::channel();
        self.inner.shared.stopping.store(true, Ordering::SeqCst);
        let sent = tokio::time::timeout(
            Duration::from_millis(300),
            self.inner.tx.send(Request::Shutdown { reply }),
        )
        .await;
        let background =
            std::mem::take(&mut *self.inner.background.lock().unwrap_or_else(|p| p.into_inner()));
        let fermati = background.len();
        for t in background {
            t.abort();
        }
        if fermati > 0 {
            self.inner.shared.log(
                LogLevel::Info,
                LogChannel::Udp,
                format!(
                    "Ricevitore dello stato UDP fermato: la porta {} è libera e lo stato non viene più aggiornato",
                    self.inner.shared.udp_port.unwrap_or(BCAST_PORT)
                ),
                None,
            );
        }
        if matches!(sent, Ok(Ok(()))) {
            let _ = tokio::time::timeout(self.inner.shutdown_wait, rx).await;
        }
        let supervisor = self.inner.supervisor.lock().unwrap_or_else(|p| p.into_inner()).take();
        if let Some(h) = supervisor {
            h.abort();
        }
        // La sorveglianza ha generato l'attore in un task proprio: abortire la sorveglianza
        // non lo tocca.
        let actor = self.inner.shared.actor.lock().unwrap_or_else(|p| p.into_inner()).take();
        if let Some(a) = actor {
            a.abort();
        }
        let already = self.conn_state().status == ConnStatus::Disconnected;
        if !already {
            self.inner.shared.set_conn(ConnStatus::Disconnected, None);
        }
    }
}

// ---------------------------------------------------------------------------
// Attore
// ---------------------------------------------------------------------------

enum Interrupt {
    Stop(Option<oneshot::Sender<()>>),
    Reconnect,
}

/// Esegue `fut` rispondendo nel frattempo «Matrice non connessa» ai comandi in coda.
async fn serve_until<F: Future>(
    fut: F,
    rx: &mut mpsc::Receiver<Request>,
    reconnect_interrupts: bool,
) -> Result<F::Output, Interrupt> {
    tokio::pin!(fut);
    loop {
        tokio::select! {
            out = &mut fut => return Ok(out),
            req = rx.recv() => match req {
                None => return Err(Interrupt::Stop(None)),
                Some(Request::Shutdown { reply }) => return Err(Interrupt::Stop(Some(reply))),
                Some(Request::Reconnect) => {
                    if reconnect_interrupts {
                        return Err(Interrupt::Reconnect);
                    }
                }
                Some(Request::Cmd { line, reply }) => {
                    let _ = reply.send(CmdOutcome::failed(line.trim(), NOT_CONNECTED));
                }
            }
        }
    }
}

struct EstablishError {
    rejected: bool,
    message: String,
}

struct ConnCtx {
    last_activity: Instant,
    last_xget: Instant,
    last_event: Option<String>,
    /// Ultima verifica della sorveglianza del broadcast UDP.
    last_udp_check: Instant,
}

/// Somma un ritardo a un istante **senza mai andare in panic**: `Instant + Duration` va in
/// panic in overflow e con `panic = "abort"` chiuderebbe l'applicazione, finestra compresa.
fn fra(base: Instant, d: Duration) -> Instant {
    base.checked_add(d).or_else(|| base.checked_add(Duration::from_secs(3600))).unwrap_or(base)
}

fn backoff(cfg: &ClientConfig, attempt: usize) -> Duration {
    let list: &[u64] =
        if cfg.reconnect_backoff_s.is_empty() { &[1, 2, 5, 10] } else { &cfg.reconnect_backoff_s };
    let s = list[attempt.min(list.len() - 1)];
    // Mai sotto un secondo: `0` sarebbe una raffica di tentativi contro la matrice
    // (spec §5.1 prescrive 1, 2, 5, 10 s). `ClientConfig::sanitized` corregge già i
    // valori, questo è il paracadute per chi usa `backoff` con una configurazione grezza.
    Duration::from_secs(s.max(1))
}

/// Sorveglianza dell'attore: se `run_actor` termina senza che sia stato chiesto un arresto
/// (errore non previsto, panic in debug) lo stato passa a `error` con la causa e l'attore
/// viene riavviato. Senza questo la finestra resterebbe su «connesso», `wait_connected`
/// ritornerebbe subito `Ok` e ogni comando fallirebbe con «Client arrestato»: l'app
/// sembrerebbe viva senza comandare più niente.
///
/// Questo task non esegue nulla che possa andare in panic: nessun `unwrap`, nessun indice.
async fn supervise(
    cfg: ClientConfig,
    shared: Arc<Shared>,
    rx: Arc<tokio::sync::Mutex<mpsc::Receiver<Request>>>,
) {
    let mut riavvii = 0u32;
    loop {
        let task = tokio::spawn(run_actor(cfg.clone(), shared.clone(), rx.clone()));
        shared.set_actor(task.abort_handle());
        let esito = task.await;
        if shared.stopping.load(Ordering::SeqCst) {
            return;
        }
        let causa = match esito {
            Err(e) if e.is_cancelled() => return,
            Err(e) => format!("il gestore della sessione è morto per un errore interno ({e})"),
            Ok(()) => {
                "il gestore della sessione è terminato senza che fosse stato chiesto l'arresto"
                    .to_string()
            }
        };
        riavvii = riavvii.saturating_add(1);
        shared.log(
            LogLevel::Error,
            LogChannel::App,
            format!(
                "Guasto interno del programma: {causa}. Fino al riavvio la matrice non riceve comandi \
                 e lo stato mostrato non è aggiornato: riavvio del gestore (tentativo {riavvii})."
            ),
            None,
        );
        shared.set_conn(ConnStatus::Error, Some(format!("{causa}; riavvio in corso")));
        // Attesa crescente (1…10 s): se il guasto si ripete non si consuma la macchina.
        tokio::time::sleep(Duration::from_secs(u64::from(riavvii).min(10))).await;
    }
}

async fn run_actor(
    cfg: ClientConfig,
    shared: Arc<Shared>,
    rx: Arc<tokio::sync::Mutex<mpsc::Receiver<Request>>>,
) {
    // Un solo attore alla volta: il riavvio aspetta che il precedente abbia mollato la coda.
    let mut guard = rx.lock().await;
    let rx = &mut *guard;
    let mut attempt = 0usize;
    let stop_reply: Option<oneshot::Sender<()>>;
    'outer: loop {
        shared.set_conn(ConnStatus::Connecting, None);
        let res = serve_until(establish(&cfg, &shared), rx, false).await;
        match res {
            Err(Interrupt::Stop(r)) => {
                stop_reply = r;
                break 'outer;
            }
            Err(Interrupt::Reconnect) => {}
            Ok(Ok((session, ctx))) => {
                attempt = 0;
                match connected_loop(session, ctx, &cfg, &shared, rx).await {
                    Exit::Stop(r) => {
                        stop_reply = r;
                        break 'outer;
                    }
                    Exit::Reconnect => continue 'outer,
                    Exit::Lost(reason) => {
                        shared.log(
                            LogLevel::Warn,
                            LogChannel::App,
                            format!("Sessione persa: {reason}"),
                            None,
                        );
                        shared.set_conn(ConnStatus::Reconnecting, Some(reason));
                    }
                }
            }
            Ok(Err(e)) => {
                let status = if e.rejected { ConnStatus::Error } else { ConnStatus::Reconnecting };
                shared.log(LogLevel::Error, LogChannel::App, e.message.clone(), None);
                shared.set_conn(status, Some(e.message));
            }
        }
        let wait = backoff(&cfg, attempt);
        attempt = attempt.saturating_add(1);
        match serve_until(tokio::time::sleep(wait), rx, true).await {
            Ok(()) => {}
            Err(Interrupt::Reconnect) => attempt = 0,
            Err(Interrupt::Stop(r)) => {
                stop_reply = r;
                break 'outer;
            }
        }
    }
    // Uscita voluta (`Shutdown`, oppure tutti gli handle distrutti): la sorveglianza non
    // deve riavviare l'attore.
    shared.stopping.store(true, Ordering::SeqCst);
    shared.set_conn(ConnStatus::Disconnected, None);
    if let Some(r) = stop_reply {
        let _ = r.send(());
    }
}

async fn establish(
    cfg: &ClientConfig,
    shared: &Arc<Shared>,
) -> Result<(Session, ConnCtx), EstablishError> {
    let opts = SessionOptions {
        connect_timeout_ms: cfg.connect_timeout_ms,
        cmd_timeout_ms: cfg.cmd_timeout_ms,
        status_timeout_base_ms: cfg.status_timeout_base_ms,
        delay_ms: cfg.delay_ms,
        logger: Some(shared.logger()),
    };
    let mut s = Session::open(&cfg.transport, opts)
        .await
        .map_err(|e| EstablishError { rejected: false, message: e.to_string() })?;
    shared.set_conn(ConnStatus::Handshaking, None);
    let info = match s.handshake().await {
        Ok(i) => i,
        Err(e) => {
            let rejected = matches!(e, HandshakeError::Rejected(_));
            if rejected {
                s.close().await;
            }
            return Err(EstablishError { rejected, message: e.to_string() });
        }
    };
    shared.set_info(info.clone());
    let out = s.command(cmds::XGET, None).await;
    if !out.ok() {
        return Err(EstablishError {
            rejected: false,
            message: format!("XGET iniziale fallita: {}", out.error_message().unwrap_or_default()),
        });
    }
    match Routing::from_xget(out.data().unwrap_or(""), info.max_ports) {
        Ok(r) => {
            shared.set_routing(r, RoutingSource::Xget);
            shared.quiet();
        }
        Err(e) => {
            return Err(EstablishError {
                rejected: true,
                message: format!("XGET non interpretabile: {e}"),
            });
        }
    }
    let mut ctx = ConnCtx {
        last_activity: Instant::now(),
        last_xget: Instant::now(),
        last_event: None,
        last_udp_check: Instant::now(),
    };
    let ev = s.command(cmds::XLASTEVENT, None).await;
    if s.is_broken() {
        return Err(EstablishError {
            rejected: false,
            message: ev.error_message().unwrap_or_default(),
        });
    }
    if ev.ok() {
        ctx.last_event = Some(ev.data().unwrap_or("").to_string());
    }
    ctx.last_activity = Instant::now();
    Ok((s, ctx))
}

/// Codici `ERnnnn` che segnalano un **guasto della matrice** e non un comando sbagliato
/// (spec §3.7; testi in `protocol::response::error_description`): fork fallita, memoria
/// insufficiente, I/O su file, processo di controllo che non risponde, allocazione.
const PLATFORM_ERRORS: [u16; 5] = [11, 13, 14, 15, 20];

enum Exit {
    Stop(Option<oneshot::Sender<()>>),
    Reconnect,
    Lost(String),
}

async fn connected_loop(
    mut session: Session,
    mut ctx: ConnCtx,
    cfg: &ClientConfig,
    shared: &Arc<Shared>,
    rx: &mut mpsc::Receiver<Request>,
) -> Exit {
    shared.set_conn(ConnStatus::Connected, None);
    // `sanitized` limita già questi valori; `fra` è il paracadute contro l'overflow di
    // `Instant + Duration`, che con `panic = "abort"` chiuderebbe l'applicazione.
    let hb = Duration::from_secs(cfg.heartbeat_s.min(3600));
    let poll = Duration::from_secs(cfg.poll_xget_s.min(3600));
    let udp_attivo = cfg.udp.is_some();
    let mut lost_reason = String::new();
    loop {
        let hb_at = fra(ctx.last_activity, hb);
        let poll_at = fra(ctx.last_xget, poll);
        let udp_check_at = fra(ctx.last_udp_check, shared.bcast_period);
        tokio::select! {
            req = rx.recv() => match req {
                None => {
                    session.close().await;
                    return Exit::Stop(None);
                }
                Some(Request::Shutdown { reply }) => {
                    session.close().await;
                    return Exit::Stop(Some(reply));
                }
                Some(Request::Reconnect) => {
                    shared.log(LogLevel::Info, LogChannel::App, "Riconnessione richiesta", None);
                    session.close().await;
                    return Exit::Reconnect;
                }
                Some(Request::Cmd { line, reply }) => {
                    let out = exec(&mut session, &mut ctx, cfg, shared, &line).await;
                    if session.is_broken() {
                        lost_reason = out.error.clone().unwrap_or_else(|| "canale chiuso".into());
                    }
                    let _ = reply.send(out);
                }
            },
            _ = tokio::time::sleep_until(hb_at), if cfg.heartbeat_s > 0 => {
                let out = session.command(cmds::XLASTEVENT, None).await;
                ctx.last_activity = Instant::now();
                if out.ok() {
                    let v = out.data().unwrap_or("").to_string();
                    let changed = ctx.last_event.as_ref().is_some_and(|p| *p != v);
                    ctx.last_event = Some(v);
                    if changed {
                        shared.log(LogLevel::Info, LogChannel::App, "XLASTEVENT cambiato: rilettura dello stato", None);
                        internal_xget(&mut session, &mut ctx, cfg, shared).await;
                    }
                } else if session.is_broken() {
                    lost_reason = out.error.clone().unwrap_or_default();
                }
            },
            _ = tokio::time::sleep_until(poll_at), if cfg.poll_xget_s > 0 => {
                if shared.udp_fresh() {
                    ctx.last_xget = Instant::now();
                } else {
                    internal_xget(&mut session, &mut ctx, cfg, shared).await;
                }
            },
            // Sorveglianza del broadcast (spec §5.4): il passaggio al polling va detto,
            // altrimenti lo stato invecchia in silenzio.
            _ = tokio::time::sleep_until(udp_check_at), if udp_attivo => {
                ctx.last_udp_check = Instant::now();
                shared.udp_watchdog();
            },
        }
        if session.is_broken() {
            if lost_reason.is_empty() {
                lost_reason = "canale chiuso".into();
            }
            return Exit::Lost(lost_reason);
        }
    }
}

async fn internal_xget(
    session: &mut Session,
    ctx: &mut ConnCtx,
    cfg: &ClientConfig,
    shared: &Arc<Shared>,
) {
    let _ = exec(session, ctx, cfg, shared, cmds::XGET).await;
}

async fn exec(
    session: &mut Session,
    ctx: &mut ConnCtx,
    cfg: &ClientConfig,
    shared: &Arc<Shared>,
    line: &str,
) -> CmdOutcome {
    let out = session.command(line, None).await;
    ctx.last_activity = Instant::now();
    if out.response.is_none() {
        return out;
    }
    let max = session.max_ports();
    let parsed = parse_command(&out.command, max.max(1)).ok();
    let mut resync = false;
    if out.ok() {
        match &parsed {
            Some(ParsedCommand::Xget) => match Routing::from_xget(out.data().unwrap_or(""), max) {
                Ok(r) => {
                    shared.set_routing(r, RoutingSource::Xget);
                    shared.quiet();
                    ctx.last_xget = Instant::now();
                }
                Err(e) => shared.log(
                    LogLevel::Warn,
                    LogChannel::App,
                    format!("XGET non interpretabile: {e}"),
                    None,
                ),
            },
            Some(p) if p.is_switching() => {
                shared.quiet();
                let cached = shared.lock().routing.as_ref().map(|u| u.routing.clone());
                match cached {
                    Some(mut r) if r.max_ports == max => {
                        if r.apply_command(p) {
                            shared.set_routing(r, RoutingSource::Local);
                        } else {
                            resync = true;
                        }
                    }
                    _ => resync = true,
                }
            }
            Some(ParsedCommand::Xlastevent) => ctx.last_event = out.data().map(str::to_string),
            _ => {}
        }
    } else {
        let code = out.response.as_ref().and_then(|r| r.error_code_num()).unwrap_or(0);
        if PLATFORM_ERRORS.contains(&code) {
            // Guasti della matrice, non comandi sbagliati: senza questo l'operatore legge
            // «comando fallito», ripreme e la matrice è di fatto guasta senza che nessuno
            // lo dica (spec §3.7, descrizioni in protocol::response::error_description).
            shared.log(
                LogLevel::Error,
                LogChannel::App,
                format!(
                    "Guasto della matrice: ha risposto ER{code:04} ({}) a «{}». Non è un comando sbagliato \
                     e ripetere il comando non serve: la commutazione non è stata eseguita. Rileggo lo stato \
                     con XGET; se si ripete, la matrice va fatta vedere all'assistenza.",
                    error_description(code),
                    out.command
                ),
                None,
            );
            // Riallineamento: l'ultimo stato pubblicato potrebbe non essere più vero.
            resync = !matches!(parsed, Some(ParsedCommand::Xget));
        } else if matches!(
            parsed,
            Some(
                ParsedCommand::Xlos(_)
                    | ParsedCommand::Xgetlos(_)
                    | ParsedCommand::Xrxpwr(_)
                    | ParsedCommand::Xtxpwr(_)
            )
        ) {
            shared.mark_los_unsupported();
        }
    }
    if resync && !session.is_broken() {
        tokio::time::sleep(Duration::from_millis(cfg.delay_ms)).await;
        let x = session.command(cmds::XGET, None).await;
        if x.ok() {
            if let Ok(r) = Routing::from_xget(x.data().unwrap_or(""), max) {
                shared.set_routing(r, RoutingSource::Xget);
                shared.quiet();
                ctx.last_xget = Instant::now();
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Ricevitore UDP 17564
// ---------------------------------------------------------------------------

/// Mittenti legittimi dedotti da un host IPv4: l'indirizzo **virtuale** X del profilo, il
/// **primario** X+100 e il **backup** X+1 (spec §1.2, §5.4). `None` se l'host non è un
/// indirizzo IPv4 (nome DNS). Somme con `checked_add` su `u32`: nessun overflow silenzioso.
fn senders_from_host(host: &str) -> Option<Vec<Ipv4Addr>> {
    let x: Ipv4Addr = host.trim().parse().ok()?;
    let base = u32::from(x);
    let mut v = vec![x];
    for delta in [100u32, 1] {
        if let Some(somma) = base.checked_add(delta) {
            let ip = Ipv4Addr::from(somma);
            if !v.contains(&ip) {
                v.push(ip);
            }
        }
    }
    Some(v)
}

/// Completa `accept_from` quando il profilo lo lascia vuoto (vuoto = si accetta **qualunque**
/// mittente) e restituisce la riga di log da emettere. Due matrici dello stesso modello nella
/// stessa sala sono il caso normale: senza questo filtro la finestra mostra gli incroci
/// dell'altra e l'operatore commuta su uno stato falso.
fn derive_accept_from(udp: &mut UdpConfig, transport: &TransportConfig) -> (LogLevel, String) {
    let elenco = |v: &[Ipv4Addr]| v.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(", ");
    if !udp.accept_from.is_empty() {
        return (
            LogLevel::Info,
            format!(
                "Stato UDP accettato solo dai mittenti indicati nel profilo: {}",
                elenco(&udp.accept_from)
            ),
        );
    }
    match transport {
        TransportConfig::Tcp { host, .. } => match senders_from_host(host) {
            Some(v) => {
                let msg = format!(
                    "Stato UDP accettato solo dalla matrice {host}: mittenti {} (virtuale, primario, backup); \
                     i datagrammi di altre matrici vengono ignorati",
                    elenco(&v)
                );
                udp.accept_from = v;
                (LogLevel::Info, msg)
            }
            None => (
                LogLevel::Warn,
                format!(
                    "L'host «{host}» non è un indirizzo IPv4: non si può riconoscere la matrice e si accetta \
                     lo stato UDP da QUALUNQUE mittente. Un'altra matrice dello stesso modello sulla stessa \
                     rete farebbe mostrare incroci non suoi: indicare i mittenti ammessi nel profilo."
                ),
            ),
        },
        TransportConfig::Serial { port, .. } => (
            LogLevel::Warn,
            format!(
                "Canale seriale ({port}): l'indirizzo della matrice non è noto e si accetta lo stato UDP da \
                 QUALUNQUE mittente. Un'altra matrice dello stesso modello sulla stessa rete farebbe mostrare \
                 incroci non suoi: indicare i mittenti ammessi nel profilo."
            ),
        ),
    }
}

fn bind_udp(cfg: &UdpConfig) -> std::io::Result<(tokio::net::UdpSocket, Option<String>)> {
    use socket2::{Domain, Protocol, Socket, Type};
    let s = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    s.set_reuse_address(true)?;
    #[cfg(all(
        unix,
        not(any(target_os = "solaris", target_os = "illumos", target_os = "cygwin"))
    ))]
    s.set_reuse_port(true)?;
    s.set_broadcast(true)?;
    let addr = SocketAddr::from((Ipv4Addr::UNSPECIFIED, cfg.bind_port));
    s.bind(&addr.into())?;
    let mut warn = None;
    if let Some(g) = cfg.mcast_group {
        if let Err(e) = s.join_multicast_v4(&g, &Ipv4Addr::UNSPECIFIED) {
            warn = Some(format!("Iscrizione al gruppo multicast {g} fallita: {e}"));
        }
    }
    s.set_nonblocking(true)?;
    let std_sock: std::net::UdpSocket = s.into();
    Ok((tokio::net::UdpSocket::from_std(std_sock)?, warn))
}

async fn udp_listener(cfg: UdpConfig, shared: Arc<Shared>) {
    let sock = match bind_udp(&cfg) {
        Ok((s, warn)) => {
            if let Some(w) = warn {
                shared.log(LogLevel::Warn, LogChannel::Udp, w, None);
            }
            shared.log(
                LogLevel::Info,
                LogChannel::Udp,
                format!(
                    "In ascolto dello stato UDP sulla porta {}{}",
                    cfg.bind_port,
                    cfg.mcast_group.map(|g| format!(" (multicast {g})")).unwrap_or_default()
                ),
                None,
            );
            s
        }
        Err(e) => {
            shared.log(
                LogLevel::Warn,
                LogChannel::Udp,
                format!(
                    "Ascolto UDP sulla porta {} non disponibile ({e}): si prosegue senza broadcast",
                    cfg.bind_port
                ),
                None,
            );
            return;
        }
    };
    let mut asm = BcastAssembler::new(shared.bcast_period.as_millis() as u64);
    let mut buf = vec![0u8; 65_536];
    let mut bad_warned = false;
    loop {
        let (n, from) = match sock.recv_from(&mut buf).await {
            Ok(x) => x,
            Err(_) => {
                tokio::time::sleep(Duration::from_millis(200)).await;
                continue;
            }
        };
        let mut block = match parse_17564(&buf[..n]) {
            Ok(b) => b,
            Err(e) => {
                if !bad_warned {
                    bad_warned = true;
                    shared.log(
                        LogLevel::Warn,
                        LogChannel::Udp,
                        format!("Datagramma UDP da {from} scartato: {e}"),
                        None,
                    );
                }
                continue;
            }
        };
        if !cfg.accept_from.is_empty() {
            let src = match from.ip() {
                IpAddr::V4(ip) => ip,
                IpAddr::V6(ip) => ip.to_ipv4_mapped().unwrap_or(Ipv4Addr::UNSPECIFIED),
            };
            if !cfg.accept_from.contains(&src) && !cfg.accept_from.contains(&block.sender) {
                continue;
            }
            // Spec §5.4: `{X, X+100, X+1}` sono **una sola** identità. L'assemblatore
            // confronta il solo campo IP del datagramma, e dopo un failover quel campo può
            // cambiare (verifica V5 aperta): i due blocchi del TLX1280 sembrerebbero di due
            // matrici diverse e la fotografia non si comporrebbe più. Qui il datagramma è
            // già stato riconosciuto come nostro, quindi il campo si riporta all'identità
            // canonica (il primo mittente ammesso, cioè X).
            if let Some(canonico) = cfg.accept_from.first() {
                block.sender = *canonico;
            }
        }
        let expected = shared.lock().info.as_ref().map(|i| i.max_ports);
        // Confronto sul **blocco ricevuto**: il controllo sulla `Routing` ricomposta non
        // scatterebbe mai (l'assemblatore la costruisce con le nostre porte). Un blocco di
        // un'altra matrice entrerebbe comunque nell'assemblatore e rovinerebbe la
        // ricomposizione dei due blocchi del TLX1280.
        if let Some(exp) = expected {
            if block.last > exp || block.first > exp {
                shared.warn_udp_altra_matrice(from, block.last, exp);
                continue;
            }
        }
        // Datagramma accettato: da qui si conta la freschezza del broadcast. Non dalla
        // fotografia composta, che può mancare per un pezzo (handshake non finito, un solo
        // blocco dei due del TLX1280) mentre i datagrammi arrivano regolarmente.
        shared.note_udp();
        asm.set_expected_ports(expected);
        if let Some(r) = asm.push(block, now_ms()) {
            shared.on_udp_routing(r);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_e_serde() {
        let c = ClientConfig::default();
        assert_eq!(c.reconnect_backoff_s, vec![1, 2, 5, 10]);
        assert_eq!(backoff(&c, 0), Duration::from_secs(1));
        assert_eq!(backoff(&c, 99), Duration::from_secs(10));
        let j = serde_json::to_value(&c).unwrap();
        assert_eq!(j["transport"]["kind"], "tcp");
        assert_eq!(j["heartbeatS"], 15);
        let back: ClientConfig = serde_json::from_value(j).unwrap();
        assert_eq!(back, c);
        let partial: ClientConfig = serde_json::from_str(r#"{"pollXgetS":0}"#).unwrap();
        assert_eq!(partial.poll_xget_s, 0);
        assert_eq!(partial.cmd_timeout_ms, 2000);
    }

    #[test]
    fn sanitized_limita_i_valori_assurdi() {
        // Un config.json scritto a mano non deve poter chiudere il programma né
        // tempestare la matrice di tentativi.
        let c = ClientConfig {
            connect_timeout_ms: 0,
            cmd_timeout_ms: 0,
            status_timeout_base_ms: 1,
            heartbeat_s: u64::MAX,
            poll_xget_s: 10_000_000,
            reconnect_backoff_s: vec![0, 1, 99_999],
            delay_ms: 10_000_000,
            bcast_missing_periods: 0,
            ..ClientConfig::default()
        };
        let (s, note) = c.sanitized();
        assert_eq!(s.connect_timeout_ms, 3000);
        assert_eq!(s.cmd_timeout_ms, 200);
        assert_eq!(s.status_timeout_base_ms, 200);
        assert_eq!(s.heartbeat_s, 3600);
        assert_eq!(s.poll_xget_s, 3600);
        assert_eq!(s.reconnect_backoff_s, vec![1, 1, 600]);
        assert_eq!(s.delay_ms, 10_000);
        assert_eq!(s.bcast_missing_periods, 1);
        // ogni correzione è spiegata nel log
        assert!(note.len() >= 8, "note = {note:?}");
        assert!(note.iter().any(|n| n.contains("connectTimeoutMs")));
        assert!(note.iter().any(|n| n.contains("reconnectBackoffS[0]")));
        // niente panic con Instant + Duration dopo il taglio
        assert_eq!(
            fra(Instant::now(), Duration::from_secs(s.heartbeat_s)).elapsed(),
            Duration::ZERO
        );
        // una configurazione sana non viene toccata
        let (ok, note) = ClientConfig::default().sanitized();
        assert_eq!(ok, ClientConfig::default());
        assert!(note.is_empty(), "note = {note:?}");
        // 0 resta 0: heartbeat e polling disattivati sono legittimi
        let (s, _) =
            ClientConfig { heartbeat_s: 0, poll_xget_s: 0, ..ClientConfig::default() }.sanitized();
        assert_eq!((s.heartbeat_s, s.poll_xget_s), (0, 0));
    }

    #[test]
    fn backoff_mai_sotto_un_secondo() {
        let c = ClientConfig { reconnect_backoff_s: vec![0], ..ClientConfig::default() };
        assert_eq!(backoff(&c, 0), Duration::from_secs(1));
        assert_eq!(backoff(&c, 7), Duration::from_secs(1));
        let (s, _) = c.sanitized();
        assert_eq!(backoff(&s, 0), Duration::from_secs(1));
        let vuoto = ClientConfig { reconnect_backoff_s: vec![], ..ClientConfig::default() };
        assert_eq!(backoff(&vuoto, 0), Duration::from_secs(1));
        assert_eq!(backoff(&vuoto, 3), Duration::from_secs(10));
    }

    #[test]
    fn fra_non_va_in_panic_in_overflow() {
        let t = Instant::now();
        assert_eq!(fra(t, Duration::ZERO), t);
        let _ = fra(t, Duration::MAX); // senza `fra` sarebbe un panic
        let _ = fra(t, Duration::from_secs(u64::MAX / 2));
    }

    #[test]
    fn attesa_di_shutdown_copre_il_comando_piu_lungo() {
        // XGET su TLX1280: 2000 + 10 × 1280 = 14 800 ms, più il margine di chiusura.
        let w = ClientConfig::default().max_cmd_timeout();
        assert!(w >= Duration::from_millis(14_800), "attesa troppo breve: {w:?}");
        assert!(w <= Duration::from_secs(60));
        // Macro da 50 coppie con un delay lungo è ancora coperto.
        let lunga = ClientConfig { delay_ms: 1000, ..ClientConfig::default() }.max_cmd_timeout();
        assert!(lunga >= Duration::from_millis(52_000), "attesa troppo breve: {lunga:?}");
    }

    #[test]
    fn mittenti_dedotti_dall_host() {
        // spec §1.2: X virtuale, X+100 primario, X+1 backup
        assert_eq!(
            senders_from_host("192.168.13.15"),
            Some(vec![
                Ipv4Addr::new(192, 168, 13, 15),
                Ipv4Addr::new(192, 168, 13, 115),
                Ipv4Addr::new(192, 168, 13, 16),
            ])
        );
        assert_eq!(
            senders_from_host(" 10.0.0.45 "),
            Some(vec![
                Ipv4Addr::new(10, 0, 0, 45),
                Ipv4Addr::new(10, 0, 0, 145),
                Ipv4Addr::new(10, 0, 0, 46),
            ])
        );
        // nessun overflow silenzioso in cima all'intervallo
        assert_eq!(
            senders_from_host("255.255.255.255"),
            Some(vec![Ipv4Addr::new(255, 255, 255, 255)])
        );
        assert_eq!(
            senders_from_host("255.255.255.200"),
            Some(vec![Ipv4Addr::new(255, 255, 255, 200), Ipv4Addr::new(255, 255, 255, 201)])
        );
        // nome DNS: non si può dedurre nulla
        assert_eq!(senders_from_host("matrice-sala1.local"), None);
        assert_eq!(senders_from_host(""), None);
    }

    #[test]
    fn accept_from_riempito_solo_quando_si_puo() {
        let tcp = |host: &str| TransportConfig::Tcp {
            host: host.into(),
            port: 17567,
            connect_timeout_ms: 0,
        };
        // vuoto + host IPv4 → riempito, log informativo
        let mut u = UdpConfig::default();
        let (lv, msg) = derive_accept_from(&mut u, &tcp("192.168.13.15"));
        assert_eq!(lv, LogLevel::Info);
        assert_eq!(u.accept_from.len(), 3);
        assert!(u.accept_from.contains(&Ipv4Addr::new(192, 168, 13, 115)), "{:?}", u.accept_from);
        assert!(msg.contains("192.168.13.115"), "{msg}");
        // già indicati nel profilo → non si tocca niente
        let mut u = UdpConfig { accept_from: vec![Ipv4Addr::LOCALHOST], ..UdpConfig::default() };
        let (lv, _) = derive_accept_from(&mut u, &tcp("192.168.13.15"));
        assert_eq!(lv, LogLevel::Info);
        assert_eq!(u.accept_from, vec![Ipv4Addr::LOCALHOST]);
        // nome DNS → comportamento attuale (qualunque mittente) con avviso
        let mut u = UdpConfig::default();
        let (lv, msg) = derive_accept_from(&mut u, &tcp("matrice.sala1"));
        assert_eq!(lv, LogLevel::Warn);
        assert!(u.accept_from.is_empty());
        assert!(msg.contains("QUALUNQUE"), "{msg}");
        // seriale → idem
        let mut u = UdpConfig::default();
        let seriale = TransportConfig::Serial {
            port: "COM3".into(),
            baud: 9600,
            flow: crate::transport::FlowControl::XonXoff,
        };
        let (lv, msg) = derive_accept_from(&mut u, &seriale);
        assert_eq!(lv, LogLevel::Warn);
        assert!(u.accept_from.is_empty());
        assert!(msg.contains("COM3"), "{msg}");
    }

    #[test]
    fn errori_di_piattaforma_sono_guasti() {
        for code in [11u16, 13, 14, 15, 20] {
            assert!(PLATFORM_ERRORS.contains(&code));
            assert!(
                error_description(code).contains("costruttore"),
                "ER{code:04}: {}",
                error_description(code)
            );
        }
        // un comando sbagliato non è un guasto della matrice
        for code in [1u16, 6, 7, 8, 9, 21] {
            assert!(!PLATFORM_ERRORS.contains(&code));
        }
    }

    #[test]
    fn display_stati() {
        assert_eq!(ConnStatus::Reconnecting.to_string(), "reconnecting");
        assert_eq!(RoutingSource::Udp.to_string(), "udp");
        assert_eq!(LogLevel::Tx.to_string(), "tx");
        assert_eq!(LogChannel::Serial.to_string(), "serial");
    }
}
