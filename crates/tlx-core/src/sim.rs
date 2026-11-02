//! Simulatore di matrice TLX in-process: API ASCII su TCP (più client, stato condiviso) e
//! broadcast periodico UDP 17564. Serve per la modalità demo e per i test.
//!
//! Comportamento modellato sul manuale «TLX Matrix Switch ASCII API V5 Rev. M» e sulla
//! specifica TT (§3–§6): codici d'errore del costruttore, risposte verbose come negli
//! esempi (`R0015OK#CI0004O0007`, campo lunghezza = byte dopo le 4 cifre, `LF` compreso),
//! commenti verbose in inglese come il firmware reale, minuscole accettate.

use std::io;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket as StdUdpSocket};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::protocol::bcast::{blocks_for, encode_17564};
use crate::protocol::command::{decode_macro, parse_command, ParsedCommand, ALL_PORTS};
use crate::protocol::Routing;
use crate::util::ctime_utc;

/// Versione API dichiarata dal simulatore.
pub const SIM_VERSION: &str = "V5.07.07";

/// Configurazione del simulatore.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimConfig {
    /// Numero di porte (12/24/48/80/160/320/640/1280; altri valori accettati con
    /// schede da 16 porte).
    pub ports: u16,
    /// Indirizzo di ascolto TCP (porta 0 = effimera).
    pub bind: SocketAddr,
    /// Risposte verbose (`#commento`), come nel default di fabbrica.
    pub verbose: bool,
    /// Destinazione del broadcast UDP 17564 (`None` = disattivato). Può essere un
    /// broadcast, un gruppo multicast o un unicast (test).
    pub bcast_target: Option<SocketAddr>,
    /// Periodo del broadcast in ms (4000).
    pub bcast_ms: u64,
    /// Firmware Rev. M: supporta `XLOS`/`XGETLOS`/`XRXPWR`/`XTXPWR` (altrimenti `ER0001`).
    pub rev_m: bool,
    /// Ritardo artificiale prima di ogni risposta (ms).
    pub cmd_delay_ms: u64,
    /// Macro non atomico: applica le coppie valide fino a quella che fallisce, poi risponde
    /// `ERnnnn`. Spento per default (il simulatore è atomico). L'atomicità del Macro e la
    /// sorte delle coppie già eseguite non sono documentate (spec §3.3, verifica V8 aperta):
    /// entrambe le scelte sono arbitrarie, questo campo serve a esercitare la peggiore,
    /// perché un client non deve dedurre «ER ⇒ niente è cambiato».
    pub macro_partial_apply: bool,
}

impl Default for SimConfig {
    fn default() -> Self {
        SimConfig {
            ports: 24,
            bind: SocketAddr::from(([127, 0, 0, 1], 0)),
            verbose: true,
            bcast_target: None,
            bcast_ms: 4000,
            rev_m: true,
            cmd_delay_ms: 0,
            macro_partial_apply: false,
        }
    }
}

/// Numero di schede I/O per modello (spec §10); per N non standard schede da 16 porte.
pub fn cards_for(ports: u16) -> u16 {
    match ports {
        12 | 24 => 1,
        48 => 3,
        80 | 160 => 16,
        320 => 20,
        640 => 32,
        1280 => 40,
        n => n.div_ceil(16).max(1),
    }
}

/// Bitmap `XOPTIONS` del simulatore: bit 3–6 (0120), più il bit 1 su TLX320/640.
pub fn options_for(ports: u16) -> u16 {
    let base = 8 | 16 | 32 | 64;
    if ports == 320 || ports == 640 {
        base | 2
    } else {
        base
    }
}

/// Commento verbose di `XOPTIONS`, costruito **dai bit effettivi** della bitmap (spec §3.5:
/// bit 0 riservato, 1 `XTEST2`, 2 solo maiuscole, 3 `XMAXCARDS` reale, 4 P2P, 5 multicast
/// per lo stato, 6 `XCTIME`). I nomi sono quelli dell'esempio del manuale, in inglese come il
/// firmware; per `0120` la riga resta identica all'esempio
/// (`xoptions:Actual_Card_Count P2P Multicast Connect_Time`), mentre su TLX320/640 (`0122`)
/// compare anche `XTEST2`, che il commento scritto a mano ometteva.
pub fn options_comment(options: u16) -> String {
    const NAMES: [(u16, &str); 6] = [
        (1, "XTEST2"),
        (2, "Upper_Case_Only"),
        (3, "Actual_Card_Count"),
        (4, "P2P"),
        (5, "Multicast"),
        (6, "Connect_Time"),
    ];
    let names: Vec<&str> = NAMES
        .iter()
        .filter(|&&(bit, _)| options & (1 << bit) != 0)
        .map(|&(_, name)| name)
        .collect();
    format!("xoptions:{}", names.join(" "))
}

struct SimState {
    ports: u16,
    /// Valore corrente di `XALARM` (bitmap decimale, spec §7.1): 0 all'avvio, modificabile
    /// con [`SimHandle::set_alarm`] per esercitare il percorso «allarme di sistema».
    alarm: u16,
    routing: Routing,
    last_event: String,
    in_secs: Vec<f64>,
    out_secs: Vec<f64>,
    last_account: Instant,
}

struct Reply {
    lines: Vec<String>,
    quit: bool,
    cr: Option<bool>,
}

fn frame(body: String) -> String {
    let len = body.len() + 1;
    let len = if len > 9999 { 0 } else { len };
    format!("R{len:04}{body}")
}

impl SimState {
    fn new(ports: u16) -> Self {
        SimState {
            ports,
            alarm: 0,
            routing: Routing::new(ports),
            last_event: ctime_now(),
            in_secs: vec![0.0; ports as usize + 1],
            out_secs: vec![0.0; ports as usize + 1],
            last_account: Instant::now(),
        }
    }

    fn account(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_account).as_secs_f64();
        self.last_account = now;
        let mut fed = vec![false; self.ports as usize + 1];
        for (k, &i) in self.routing.outputs.iter().enumerate() {
            if i != 0 {
                self.out_secs[k + 1] += dt;
                if let Some(f) = fed.get_mut(i as usize) {
                    *f = true;
                }
            }
        }
        for (p, f) in fed.iter().enumerate() {
            if *f {
                self.in_secs[p] += dt;
            }
        }
    }

    fn switch(&mut self, f: impl FnOnce(&mut Routing)) {
        self.account();
        let before = self.routing.clone();
        f(&mut self.routing);
        if before != self.routing {
            self.last_event = ctime_now();
        }
    }

    /// LOS deterministico: luce sulle porte usate come ingresso e su una porta su quattro.
    fn los(&self, p: u16) -> bool {
        let used_as_input = self.routing.outputs.contains(&p);
        !(used_as_input || p % 4 == 1)
    }

    /// Potenza ricevuta in µW. Alcune porte sono riservate in modo deterministico alle due
    /// condizioni diagnostiche peggiori di §11.2/§14.2, che altrimenti sarebbero
    /// irraggiungibili (il simulatore è anche oracolo dei test):
    /// - `p % 20 == 5` (5, 25, 45, …) → `0000` con LOS = 0: SFP senza diagnostica ottica
    ///   (`SignalUnknown`);
    /// - `p % 20 == 9` (9, 29, 49, …) → `0005` µW con LOS = 0: sotto il segnaposto di
    ///   10 µW di §11.2, fibra sporca o troppo lunga (`SignalWeak`).
    ///
    /// Entrambe le famiglie hanno `p % 4 == 1`, quindi [`SimState::los`] vi restituisce
    /// sempre `false`: la condizione «LOS = 0» della specifica è garantita per costruzione.
    /// Le altre porte restano come prima: `0000` con LOS = 1 (`PortLOS`), altrimenti
    /// 250–549 µW, cioè un margine sano.
    fn rx_uw(&self, p: u16) -> u16 {
        match (self.los(p), p % 20) {
            (true, _) => 0,
            (false, 5) => 0,
            (false, 9) => 5,
            (false, _) => 250 + (p as u32 * 37 % 300) as u16,
        }
    }

    fn tx_uw(&self, p: u16) -> u16 {
        400 + (p as u32 * 13 % 120) as u16
    }

    fn ports_of(&self, p: u16) -> Vec<u16> {
        if p == ALL_PORTS {
            (1..=self.ports).collect()
        } else {
            vec![p]
        }
    }

    /// Applica le coppie di un Macro fino alla prima non valida (vedi
    /// [`SimConfig::macro_partial_apply`]). Ripete i controlli d'intervallo di
    /// `parse_command` (§3.3: `iiii = 9999` vietato, porte fuori intervallo), così la coppia
    /// su cui ci si ferma è la stessa che genera l'`ERnnnn`. Scelta arbitraria finché V8 è
    /// aperta: il firmware reale potrebbe essere atomico, applicare un prefisso o fermarsi
    /// altrove.
    fn apply_macro_prefix(&mut self, original: &str, n: u16) {
        let Ok(dec) = decode_macro(original) else {
            return;
        };
        let valid: Vec<(u16, u16)> = dec
            .pairs
            .iter()
            .take_while(|&&(i, o)| i != ALL_PORTS && i <= n && (o == ALL_PORTS || o <= n))
            .copied()
            .collect();
        if !valid.is_empty() {
            self.switch(|r| r.apply_macro(&valid));
        }
    }

    fn execute(&mut self, original: &str, cfg: &SimConfig) -> Reply {
        let verbose = cfg.verbose;
        let echo = || if verbose { Some(original.to_string()) } else { None };
        let ok = |data: String, comment: Option<String>| -> Reply {
            let mut body = format!("OK{data}");
            if let (true, Some(c)) = (verbose, comment) {
                body.push('#');
                body.push_str(&c);
            }
            Reply { lines: vec![frame(body)], quit: false, cr: None }
        };
        let err = |code: u16, detail: String| -> Reply {
            let mut body = format!("ER{code:04}");
            if verbose {
                body.push_str(&format!("#{detail}: '{original}'"));
            }
            Reply { lines: vec![frame(body)], quit: false, cr: None }
        };
        let n = self.ports;
        let cmd = match parse_command(original, n) {
            Ok(c) => c,
            Err(e) => {
                if cfg.macro_partial_apply {
                    self.apply_macro_prefix(original, n);
                }
                return err(e.code, e.detail);
            }
        };
        let rev_m_only = matches!(
            cmd,
            ParsedCommand::Xlos(_)
                | ParsedCommand::Xgetlos(_)
                | ParsedCommand::Xrxpwr(_)
                | ParsedCommand::Xtxpwr(_)
        );
        if rev_m_only && !cfg.rev_m {
            return err(1, "Unknown command".into());
        }
        match cmd {
            ParsedCommand::Connect { input, outputs } => {
                self.switch(|r| r.apply_ci(input, &outputs));
                ok(String::new(), echo())
            }
            ParsedCommand::DisconnectInputs(v) => {
                self.switch(|r| r.apply_di(&v));
                ok(String::new(), echo())
            }
            ParsedCommand::DisconnectOutputs(v) => {
                self.switch(|r| r.apply_do(&v));
                ok(String::new(), echo())
            }
            ParsedCommand::Macro { pairs, .. } => {
                // Macro atomico: `parse_command` valida tutte le coppie prima di accettare il
                // comando, quindi qui si applica tutto o niente. È la scelta più favorevole
                // fra quelle possibili ed è arbitraria: l'atomicità del Macro e la sorte
                // delle coppie già eseguite non sono documentate (§3.3, V8 aperta). Con
                // `SimConfig::macro_partial_apply` il simulatore applica invece il prefisso
                // valido, per non lasciare credere al client che «ER ⇒ niente è cambiato».
                self.switch(|r| r.apply_macro(&pairs));
                ok(String::new(), echo())
            }
            ParsedCommand::Put(target) => {
                self.switch(|r| r.apply_put(&target));
                ok(String::new(), echo())
            }
            // ATTENZIONE: l'effetto di `XRESET` e di `XTEST1/2/3` sugli incroci **non è
            // documentato** (spec §3.3 per `XRESET`, §3.1/§3.7: sola manutenzione; verifica
            // V20 aperta). «Azzera tutto» e le tre permutazioni qui sotto sono invenzioni
            // arbitrarie del simulatore, scelte solo per avere un effetto osservabile: non
            // sono un requisito e nessun test deve asserirle come tale. La risposta corretta
            // a un effetto non modellabile è quella che il client usa già:
            // `Routing::apply_command` restituisce `false` per questi comandi e forza la
            // rilettura dello stato.
            ParsedCommand::Xreset => {
                self.switch(|r| r.clear());
                ok(String::new(), echo())
            }
            ParsedCommand::Xtest(k) => {
                if k == 2 && !(n == 320 || n == 640) {
                    return err(1, "XTEST2 is only available on the TLX320 and TLX640".into());
                }
                self.switch(|r| {
                    r.clear();
                    for i in 1..=n {
                        let o = match k {
                            1 => i,
                            2 if n == 320 => {
                                if i <= 160 {
                                    i + 160
                                } else {
                                    i - 160
                                }
                            }
                            2 => n + 1 - i,
                            _ => {
                                if i % 2 == 1 {
                                    (i + 1).min(n)
                                } else {
                                    i - 1
                                }
                            }
                        };
                        r.set(o, i);
                    }
                });
                ok(String::new(), echo())
            }
            ParsedCommand::StatusIn(p) => {
                let outs = self.routing.outputs_of(p);
                let mut s = format!("I{p:04}");
                if outs.is_empty() {
                    s.push_str("O0000");
                }
                for o in outs {
                    s.push_str(&format!("O{o:04}"));
                }
                ok(s, echo())
            }
            ParsedCommand::StatusOut(p) => {
                ok(format!("I{:04}O{p:04}", self.routing.input_of(p)), echo())
            }
            ParsedCommand::Xget => ok(self.routing.to_xget(), None),
            ParsedCommand::XstatusIo => ok(self.routing.to_statusio(), None),
            ParsedCommand::XstatusOi => ok(self.routing.to_statusoi(), None),
            ParsedCommand::Xversion => ok(SIM_VERSION.into(), echo()),
            ParsedCommand::Xtype => ok(format!("TLX{n}Switch"), echo()),
            ParsedCommand::Xmaxports => ok(format!("{n:04}"), echo()),
            ParsedCommand::Xmaxcards => ok(format!("{:04}", cards_for(n)), echo()),
            ParsedCommand::Xinstalled => ok("1".repeat(cards_for(n) as usize), echo()),
            ParsedCommand::Xportconfig => ok(format!("{n:04},0000,0000"), echo()),
            ParsedCommand::Xoptions => {
                let opts = options_for(n);
                ok(format!("{opts:04}"), Some(options_comment(opts)))
            }
            ParsedCommand::Xalarm => ok(format!("{:04}", self.alarm), echo()),
            ParsedCommand::Xlastevent => ok(self.last_event.clone(), None),
            ParsedCommand::Xctime(p) => {
                self.account();
                ok(
                    format!(
                        "{p:04},{},{}",
                        self.in_secs[p as usize] as u64, self.out_secs[p as usize] as u64
                    ),
                    echo(),
                )
            }
            ParsedCommand::Xlos(p) => {
                let s: String = self
                    .ports_of(p)
                    .into_iter()
                    .map(|q| if self.los(q) { '1' } else { '0' })
                    .collect();
                ok(format!("L{s}"), None)
            }
            // Formato ARBITRARIO del simulatore, non documentato: il manuale dichiara
            // «port: nnnn LOS=TRUE/FALSE» ma stampa un esempio incoerente
            // (`R0005OKL0 port 14 LOS = FALSE`, §13.1 punto 6), la specifica prescrive
            // `XLOS` e lascia `XGETLOS` alla verifica V36. Nessun parser del progetto
            // consuma questa risposta: non prenderla per documentata.
            ParsedCommand::Xgetlos(p) => {
                let list: Vec<u16> = if p == 0 {
                    (1..=n).filter(|&q| self.los(q)).collect()
                } else {
                    self.ports_of(p)
                };
                let s = list
                    .into_iter()
                    .map(|q| {
                        format!("port: {q:04} LOS={}", if self.los(q) { "TRUE" } else { "FALSE" })
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                ok(s, None)
            }
            ParsedCommand::Xrxpwr(p) => {
                let s: String =
                    self.ports_of(p).into_iter().map(|q| format!("{:04}", self.rx_uw(q))).collect();
                ok(format!("R{s}"), None)
            }
            ParsedCommand::Xtxpwr(p) => {
                let s: String =
                    self.ports_of(p).into_iter().map(|q| format!("{:04}", self.tx_uw(q))).collect();
                ok(format!("T{s}"), None)
            }
            ParsedCommand::Xcron => {
                let mut r = ok(String::new(), echo());
                r.cr = Some(true);
                r
            }
            ParsedCommand::Xcroff => {
                let mut r = ok(String::new(), echo());
                r.cr = Some(false);
                r
            }
            ParsedCommand::Xquit => {
                let mut r = ok(String::new(), echo());
                r.quit = true;
                r
            }
            ParsedCommand::Xhelp => Reply {
                lines: vec![
                    format!("Version: {SIM_VERSION} (build: sim)"),
                    "Usage: tlxsimapi [options]".into(),
                    "   (simulatore TT Group: elenco opzioni non disponibile)".into(),
                ],
                quit: false,
                cr: None,
            },
        }
    }
}

fn ctime_now() -> String {
    ctime_utc(crate::util::now_ms() / 1000)
}

/// Avvio del simulatore.
pub struct Simulator;

/// Handle di un simulatore in esecuzione. Alla distruzione (o con [`SimHandle::shutdown`])
/// chiude l'ascolto, le connessioni e il broadcast.
pub struct SimHandle {
    /// Indirizzo TCP effettivo di ascolto.
    pub local_addr: SocketAddr,
    state: Arc<Mutex<SimState>>,
    cfg: SimConfig,
    stop_tx: watch::Sender<bool>,
    kick_tx: watch::Sender<u64>,
    tasks: Vec<JoinHandle<()>>,
}

impl std::fmt::Debug for SimHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SimHandle")
            .field("local_addr", &self.local_addr)
            .field("ports", &self.cfg.ports)
            .finish()
    }
}

impl Simulator {
    /// Avvia il simulatore sul runtime tokio corrente.
    pub async fn start(cfg: SimConfig) -> io::Result<SimHandle> {
        if cfg.ports == 0 || cfg.ports >= 9999 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "numero di porte non valido"));
        }
        let listener = TcpListener::bind(cfg.bind).await?;
        let local_addr = listener.local_addr()?;
        let state = Arc::new(Mutex::new(SimState::new(cfg.ports)));
        let (stop_tx, stop_rx) = watch::channel(false);
        let (kick_tx, kick_rx) = watch::channel(0u64);
        let mut tasks = Vec::new();

        {
            let state = state.clone();
            let cfg = cfg.clone();
            let stop_rx = stop_rx.clone();
            tasks.push(tokio::spawn(async move {
                let mut stop = stop_rx.clone();
                loop {
                    tokio::select! {
                        r = listener.accept() => match r {
                            Ok((stream, _)) => {
                                let _ = stream.set_nodelay(true);
                                tokio::spawn(handle_client(
                                    stream, state.clone(), cfg.clone(), stop_rx.clone(), kick_rx.clone(),
                                ));
                            }
                            Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
                        },
                        _ = stop.changed() => break,
                    }
                }
            }));
        }

        if let Some(target) = cfg.bcast_target {
            let sock = bcast_socket(target)?;
            let sender = match target.ip() {
                std::net::IpAddr::V4(ip) if ip.is_loopback() => Ipv4Addr::LOCALHOST,
                _ => Ipv4Addr::new(192, 168, 13, 15),
            };
            let period = Duration::from_millis(cfg.bcast_ms.max(10));
            let state = state.clone();
            let mut stop = stop_rx.clone();
            tasks.push(tokio::spawn(async move {
                let mut tick = tokio::time::interval(period);
                tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                loop {
                    tokio::select! {
                        _ = tick.tick() => {
                            let routing = state.lock().map(|s| s.routing.clone()).ok();
                            if let Some(r) = routing {
                                for b in blocks_for(&r, sender) {
                                    let _ = sock.send_to(&encode_17564(&b), target).await;
                                }
                            }
                        }
                        _ = stop.changed() => break,
                    }
                }
            }));
        }

        Ok(SimHandle { local_addr, state, cfg, stop_tx, kick_tx, tasks })
    }
}

fn bcast_socket(target: SocketAddr) -> io::Result<UdpSocket> {
    let bind: SocketAddr = match target.ip() {
        std::net::IpAddr::V4(ip) if ip.is_loopback() => SocketAddr::from(([127, 0, 0, 1], 0)),
        _ => SocketAddr::from(([0, 0, 0, 0], 0)),
    };
    let s = StdUdpSocket::bind(bind)?;
    s.set_broadcast(true)?;
    let _ = s.set_multicast_loop_v4(true);
    s.set_nonblocking(true)?;
    UdpSocket::from_std(s)
}

async fn handle_client(
    stream: TcpStream,
    state: Arc<Mutex<SimState>>,
    cfg: SimConfig,
    mut stop: watch::Receiver<bool>,
    mut kick: watch::Receiver<u64>,
) {
    // Il ricevitore arriva clonato dal ciclo di accept, che non ha mai consumato i
    // `drop_clients()` precedenti: senza questo, ogni nuova connessione verrebbe chiusa
    // subito da un `changed()` già pendente e la riconnessione non riuscirebbe mai.
    kick.mark_unchanged();
    let (rd, mut wr) = stream.into_split();
    let mut reader = BufReader::new(rd);
    let mut buf = Vec::new();
    let mut cr = false;
    loop {
        buf.clear();
        tokio::select! {
            r = reader.read_until(b'\n', &mut buf) => match r {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            },
            _ = stop.changed() => break,
            _ = kick.changed() => break,
        }
        let text = String::from_utf8_lossy(&buf).trim_end_matches(['\r', '\n']).to_string();
        if text.trim().is_empty() {
            continue;
        }
        if cfg.cmd_delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(cfg.cmd_delay_ms)).await;
        }
        let reply = match state.lock() {
            Ok(mut st) => st.execute(text.trim(), &cfg),
            Err(_) => break,
        };
        if let Some(c) = reply.cr {
            cr = c;
        }
        let term = if cr { "\r\n" } else { "\n" };
        let mut out = String::new();
        for l in &reply.lines {
            out.push_str(l);
            out.push_str(term);
        }
        if wr.write_all(out.as_bytes()).await.is_err() {
            break;
        }
        if reply.quit {
            break;
        }
    }
    let _ = wr.shutdown().await;
}

impl SimHandle {
    /// Configurazione con cui è stato avviato.
    pub fn config(&self) -> &SimConfig {
        &self.cfg
    }

    /// Stato corrente degli incroci.
    pub fn routing(&self) -> Routing {
        self.state
            .lock()
            .map(|s| s.routing.clone())
            .unwrap_or_else(|_| Routing::new(self.cfg.ports))
    }

    /// Esegue un comando come se arrivasse da un altro controllore (es. un operatore SMP);
    /// restituisce la prima riga di risposta senza terminatore.
    pub fn execute(&self, line: &str) -> String {
        match self.state.lock() {
            Ok(mut st) => {
                st.execute(line.trim(), &self.cfg).lines.into_iter().next().unwrap_or_default()
            }
            Err(_) => String::new(),
        }
    }

    /// Forza il valore di `XALARM` (spec §7.1: bitmap decimale, `≠ 0` = allarme di sistema,
    /// decodifica per bit dipendente dal modello). All'avvio vale 0; serve ai test e alla
    /// modalità Demo per esercitare il percorso «allarme di sistema» (`SystemAlarm`, §14.2),
    /// altrimenti irraggiungibile. Il valore è limitato a 9999, le cifre rappresentabili.
    pub fn set_alarm(&self, value: u16) {
        if let Ok(mut st) = self.state.lock() {
            st.alarm = value.min(9999);
        }
    }

    /// Valore corrente di `XALARM`.
    pub fn alarm(&self) -> u16 {
        self.state.lock().map(|s| s.alarm).unwrap_or(0)
    }

    /// Chiude tutte le connessioni TCP aperte (simula un riavvio del servizio o un failover);
    /// l'ascolto resta attivo.
    pub fn drop_clients(&self) {
        self.kick_tx.send_modify(|k| *k += 1);
    }

    /// Ferma il simulatore (sincrono).
    pub fn shutdown(self) {
        drop(self);
    }
}

impl Drop for SimHandle {
    fn drop(&mut self) {
        let _ = self.stop_tx.send(true);
        for t in &self.tasks {
            t.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(n: u16) -> (SimState, SimConfig) {
        (SimState::new(n), SimConfig { ports: n, ..SimConfig::default() })
    }

    fn one(s: &mut SimState, c: &SimConfig, line: &str) -> String {
        s.execute(line, c).lines.remove(0)
    }

    #[test]
    fn risposte_verbose_come_il_manuale() {
        let (mut s, c) = st(80);
        assert_eq!(one(&mut s, &c, "CI0004O0007"), "R0015OK#CI0004O0007");
        assert_eq!(
            one(&mut s, &c, "CI0004O0087"),
            "R0072ER0007#Output port number 87 is out of range (1 thru 80): 'CI0004O0087'"
        );
        assert_eq!(one(&mut s, &c, "XGET"), "R0012OKI00040007");
        assert_eq!(one(&mut s, &c, "XMAXPORTS"), "R0017OK0080#XMAXPORTS");
        // Il manuale stampa `R0062` per questa riga, ma il corpo è di 60 byte: il campo
        // lunghezza è incoerente in 6 esempi su 34 (spec §3.2, regola 1) e il client lo
        // ignora. Il simulatore usa la regola documentata: byte dopo le 4 cifre, LF compreso.
        assert_eq!(
            one(&mut s, &c, "XOPTIONS"),
            "R0061OK0120#xoptions:Actual_Card_Count P2P Multicast Connect_Time"
        );
        assert_eq!(one(&mut s, &c, "Si0004"), "R0020OKI0004O0007#Si0004");
        assert_eq!(one(&mut s, &c, "So0008"), "R0020OKI0000O0008#So0008");
        assert_eq!(one(&mut s, &c, "xquit"), "R0009OK#xquit");
    }

    #[test]
    fn risposte_non_verbose() {
        let mut s = SimState::new(48);
        let c = SimConfig { ports: 48, verbose: false, ..SimConfig::default() };
        assert_eq!(one(&mut s, &c, "CI0004O0007"), "R0003OK");
        assert_eq!(one(&mut s, &c, "CI0004O0087"), "R0007ER0007");
        assert_eq!(one(&mut s, &c, "XMAXCARDS"), "R0007OK0003");
        assert_eq!(one(&mut s, &c, "XINSTALLED"), "R0006OK111");
        assert_eq!(one(&mut s, &c, "XTYPE"), "R0014OKTLX48Switch");
        assert_eq!(one(&mut s, &c, "XPORTCONFIG"), "R0017OK0048,0000,0000");
        assert_eq!(one(&mut s, &c, "XSTATUSOI"), "R0013OKO00070004;");
        assert_eq!(one(&mut s, &c, "XRESET"), "R0003OK");
        assert_eq!(one(&mut s, &c, "XSTATUSIO"), "R0005OKI;");
        assert_eq!(one(&mut s, &c, "XGET"), "R0003OK");
        assert_eq!(one(&mut s, &c, "FOO"), "R0007ER0001");
    }

    #[test]
    fn macro_e_punto_e_virgola() {
        let (mut s, c) = st(24);
        assert!(one(&mut s, &c, "M00010002;").starts_with("R0014OK"));
        assert_eq!(s.routing.input_of(2), 1);
        assert!(one(&mut s, &c, "M00010002;00030004").contains("ER0017"));
        assert!(one(&mut s, &c, "M").contains("OK"));
        assert!(one(&mut s, &c, "M00009999").contains("OK"));
        assert!(s.routing.connections().is_empty());
    }

    #[test]
    fn rev_m_e_diagnostica() {
        let (mut s, c) = st(24);
        one(&mut s, &c, "CI0003O0010");
        let l = one(&mut s, &c, "XLOS9999");
        assert!(l.starts_with("R0028OKL"));
        assert_eq!(&l[7 + 3..7 + 4], "0"); // porta 3 usata come ingresso: nessun LOS
        assert!(one(&mut s, &c, "XRXPWR9999").len() == 7 + 1 + 96);
        assert!(one(&mut s, &c, "XTXPWR0001").starts_with("R0008OKT0"));
        let old = SimConfig { rev_m: false, ..c.clone() };
        assert!(one(&mut s, &old, "XLOS9999").contains("ER0001"));
        assert!(one(&mut s, &c, "XTEST2").contains("ER0001"));
        assert_eq!(one(&mut s, &c, "XTEST1"), "R0010OK#XTEST1");
        // Niente assert sugli incroci dopo `XTEST1`: l'effetto di `XTEST*` e `XRESET` non è
        // documentato (V20 aperta) e la permutazione del simulatore è un'invenzione, non un
        // requisito. Il client non la modella: `Routing::apply_command` restituisce `false`
        // per questi comandi e rilegge lo stato.
    }

    #[test]
    fn cr_e_help() {
        let (mut s, c) = st(24);
        let r = s.execute("XCRON", &c);
        assert_eq!(r.cr, Some(true));
        let r = s.execute("XHELP", &c);
        assert!(r.lines.len() > 1 && !r.lines[0].starts_with('R'));
        assert_eq!(cards_for(1280), 40);
        assert_eq!(options_for(320), 122);
    }

    /// Punto 1: le due condizioni diagnostiche peggiori di §11.2/§14.2 sono raggiungibili.
    #[test]
    fn segnale_debole_e_sconosciuto_su_porte_riservate() {
        let (mut s, c) = st(48);
        // Le porte riservate hanno sempre LOS = 0, come esige la specifica.
        for p in [5, 9, 25, 29, 45] {
            assert!(!s.los(p), "la porta {p} non deve essere in LOS");
        }
        // `XRXPWR = 0000` con LOS = 0 → SignalUnknown (SFP senza diagnostica).
        assert_eq!(s.rx_uw(5), 0);
        assert_eq!(s.rx_uw(25), 0);
        assert_eq!(s.rx_uw(45), 0);
        // `0 < XRXPWR < 10 µW` con LOS = 0 → SignalWeak.
        assert_eq!(s.rx_uw(9), 5);
        assert_eq!(s.rx_uw(29), 5);
        // Le altre porte non cambiano: margine sano con luce, 0 con LOS (PortLOS).
        assert!(s.rx_uw(1) >= 250 && s.rx_uw(1) <= 549);
        assert!(s.los(2) && s.rx_uw(2) == 0);
        // Verifica anche attraverso la risposta a larghezza fissa (`R` + 4 cifre per porta).
        let r = one(&mut s, &c, "XRXPWR9999");
        let data = &r[8..];
        assert_eq!(&data[4 * 4..5 * 4], "0000", "porta 5");
        assert_eq!(&data[8 * 4..9 * 4], "0005", "porta 9");
        assert_eq!(&data[0..4], format!("{:04}", s.rx_uw(1)));
    }

    /// Punto 2: `XALARM` è forzabile, quindi il percorso «allarme di sistema» ha un oracolo.
    #[tokio::test]
    async fn xalarm_forzabile_dallo_handle() {
        let h = Simulator::start(SimConfig::default()).await.expect("avvio");
        assert_eq!(h.alarm(), 0);
        assert_eq!(h.execute("XALARM"), "R0014OK0000#XALARM");
        h.set_alarm(34); // bit 1 (ventola) + bit 5 sulla mappa TLX640 (§7.1)
        assert_eq!(h.alarm(), 34);
        assert_eq!(h.execute("XALARM"), "R0014OK0034#XALARM");
        h.set_alarm(70000u32 as u16); // valore grande: limitato a 4 cifre
        assert_eq!(h.execute("XALARM").len(), "R0014OK0000#XALARM".len());
        h.set_alarm(0);
        assert_eq!(h.execute("XALARM"), "R0014OK0000#XALARM");
    }

    /// Punto 3: il commento verbose di `XOPTIONS` viene dai bit, non da una stringa fissa.
    #[test]
    fn commento_xoptions_dai_bit() {
        // 80 porte (0120): riga identica all'esempio del manuale, lunghezza compresa.
        assert_eq!(options_comment(120), "xoptions:Actual_Card_Count P2P Multicast Connect_Time");
        // TLX320/640 (0122): compare `XTEST2`, che il commento scritto a mano ometteva.
        assert_eq!(
            options_comment(options_for(320)),
            "xoptions:XTEST2 Actual_Card_Count P2P Multicast Connect_Time"
        );
        assert_eq!(options_comment(0), "xoptions:");
        assert_eq!(options_comment(4), "xoptions:Upper_Case_Only");
        // Il bit 0 è riservato: nessun nome.
        assert_eq!(options_comment(1), "xoptions:");
        let (mut s, c) = st(320);
        assert_eq!(
            one(&mut s, &c, "XOPTIONS"),
            "R0068OK0122#xoptions:XTEST2 Actual_Card_Count P2P Multicast Connect_Time"
        );
    }

    /// Punto 4: l'effetto di `XRESET`/`XTEST*` sugli incroci non è documentato (V20); il
    /// contratto verso il client non è la permutazione del simulatore ma la rilettura.
    #[test]
    fn xreset_e_xtest_non_sono_modellabili_dal_client() {
        let mut r = Routing::new(24);
        r.apply_ci(3, &[10]);
        assert!(!r.apply_command(&ParsedCommand::Xreset), "XRESET richiede la rilettura");
        for k in 1..=3 {
            assert!(!r.apply_command(&ParsedCommand::Xtest(k)), "XTEST{k} richiede la rilettura");
        }
        // `apply_command` non ha toccato lo stato del client: nessun effetto inventato.
        assert_eq!(r.input_of(10), 3);
    }

    /// Punto 5: formato arbitrario di `XGETLOS` (V36) e Macro non atomico opzionale (V8).
    #[test]
    fn xgetlos_e_macro_non_atomico() {
        let (mut s, c) = st(24);
        // Formato arbitrario del simulatore, dichiarato nel commento: nessun parser lo usa.
        assert_eq!(one(&mut s, &c, "XGETLOS0001"), "R0023OKport: 0001 LOS=FALSE");
        assert_eq!(one(&mut s, &c, "XGETLOS0002"), "R0022OKport: 0002 LOS=TRUE");
        // Default (atomico): la coppia 2 è fuori intervallo, niente viene applicato.
        let bad = "M000100020003009900050006";
        assert!(one(&mut s, &c, bad).contains("ER0007"));
        assert!(s.routing.connections().is_empty());
        // Con `macro_partial_apply`: le coppie valide prima dell'errore restano applicate.
        let partial = SimConfig { macro_partial_apply: true, ..c.clone() };
        assert!(one(&mut s, &partial, bad).contains("ER0007"));
        assert_eq!(s.routing.input_of(2), 1);
        assert_eq!(s.routing.input_of(6), 0, "le coppie dopo l'errore non sono applicate");
        // Un errore che non è di coppia non applica nulla nemmeno con il campo attivo.
        s.routing.clear();
        assert!(one(&mut s, &partial, "M00010002;0003").contains("ER0017"));
        assert!(s.routing.connections().is_empty());
    }
}
