//! Stato dell'applicazione: configurazione, connessione alla matrice, cache per la UI,
//! log, annulla. I comandi IPC (ipc.rs) chiamano i metodi di `App`.

use crate::config::{AppConfig, LinkMode, TransportKind};
use crate::ipc::*;
use crate::plan::{self, Outputs, Plan};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};
use tlx_core::client::{ClientConfig, ClientEvent, MatrixClient, UdpConfig};
use tlx_core::sim::{SimConfig, SimHandle, Simulator};
use tlx_core::transport::{FlowControl, TransportConfig};

const LOG_MAX: usize = 2000;

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

struct Conn {
    client: Option<MatrixClient>,
    sim: Option<SimHandle>,
    profile_id: Option<String>,
    forwarder: Option<tauri::async_runtime::JoinHandle<()>>,
}

#[derive(Clone)]
struct UndoRec {
    before: Outputs,
    after: Outputs,
}

#[derive(Default)]
struct Cache {
    conn: Option<ConnStateDto>,
    info: Option<MatrixInfoDto>,
    routing: Option<RoutingDto>,
    diag: Option<DiagnosticsDto>,
    undo: Option<UndoRec>,
}

struct LogBuf {
    entries: VecDeque<LogEntryDto>,
    next_id: u64,
}

pub struct App {
    handle: AppHandle,
    cfg_path: PathBuf,
    cfg: Mutex<AppConfig>,
    conn: tokio::sync::Mutex<Conn>,
    cache: Mutex<Cache>,
    log: Mutex<LogBuf>,
    /// Una sola azione di commutazione alla volta (le letture passano comunque dalla coda del client).
    op: tokio::sync::Mutex<()>,
}

impl App {
    pub fn new(handle: AppHandle, cfg_path: PathBuf) -> Self {
        let (cfg, warn) = AppConfig::load(&cfg_path);
        let app = Self {
            handle,
            cfg_path,
            cfg: Mutex::new(cfg),
            conn: tokio::sync::Mutex::new(Conn {
                client: None,
                sim: None,
                profile_id: None,
                forwarder: None,
            }),
            cache: Mutex::new(Cache::default()),
            log: Mutex::new(LogBuf { entries: VecDeque::new(), next_id: 1 }),
            op: tokio::sync::Mutex::new(()),
        };
        if let Some(w) = warn {
            app.log("warn", "app", &w, None);
        }
        // prima scrittura: il file esiste subito con i default
        let _ = app.cfg.lock().unwrap().save(&app.cfg_path);
        app
    }

    // ───────────────────────── log ─────────────────────────

    pub fn log(&self, level: &str, channel: &str, message: &str, latency_ms: Option<u64>) {
        let entry = {
            let mut l = self.log.lock().unwrap();
            let e = LogEntryDto {
                id: l.next_id,
                ts: now_ms(),
                level: level.into(),
                channel: channel.into(),
                message: message.into(),
                latency_ms,
            };
            l.next_id += 1;
            l.entries.push_back(e.clone());
            while l.entries.len() > LOG_MAX {
                l.entries.pop_front();
            }
            e
        };
        let _ = self.handle.emit("tlx://log", entry);
    }

    pub fn get_log(&self) -> Vec<LogEntryDto> {
        self.log.lock().unwrap().entries.iter().cloned().collect()
    }

    pub fn clear_log(&self) {
        self.log.lock().unwrap().entries.clear();
        self.log("info", "app", "Log svuotato", None);
    }

    pub fn log_csv(&self) -> String {
        let mut s = String::from("id;ora;livello;canale;messaggio;latenza_ms\n");
        for e in self.log.lock().unwrap().entries.iter() {
            s.push_str(&format!(
                "{};{};{};{};\"{}\";{}\n",
                e.id,
                fmt_ts(e.ts),
                e.level,
                e.channel,
                e.message.replace('"', "\"\""),
                e.latency_ms.map(|x| x.to_string()).unwrap_or_default()
            ));
        }
        s
    }

    // ───────────────────────── config ─────────────────────────

    pub fn config(&self) -> AppConfig {
        self.cfg.lock().unwrap().clone()
    }

    pub fn save_config(&self, mut c: AppConfig) -> Result<(), String> {
        c.sanitize();
        c.save(&self.cfg_path)?;
        *self.cfg.lock().unwrap() = c;
        Ok(())
    }

    fn settings(&self) -> crate::config::Settings {
        self.cfg.lock().unwrap().settings.clone()
    }

    fn active_profile_id(&self) -> Option<String> {
        self.cfg.lock().unwrap().active_profile_id.clone()
    }

    fn connected_profile(&self) -> Option<String> {
        // usato solo per le etichette/blocchi; la connessione vera è in `conn`
        self.cache.lock().unwrap().conn.as_ref().and_then(|c| c.profile_id.clone())
    }

    // ───────────────────────── connessione ─────────────────────────

    pub async fn autoconnect(&self) {
        if let Some(id) = self.active_profile_id() {
            if let Err(e) = self.connect(&id).await {
                self.log("error", "app", &e, None);
            }
        }
    }

    pub async fn connect(&self, profile_id: &str) -> Result<(), String> {
        let (profile, settings) = {
            let c = self.cfg.lock().unwrap();
            let p = c.profile(profile_id).cloned().ok_or("Profilo inesistente")?;
            (p, c.settings.clone())
        };
        self.disconnect().await;
        {
            // il profilo scelto diventa quello attivo (riapertura automatica al prossimo avvio)
            let mut c = self.cfg.lock().unwrap();
            if c.active_profile_id.as_deref() != Some(profile_id) {
                c.active_profile_id = Some(profile_id.to_string());
                let _ = c.save(&self.cfg_path);
            }
        }
        let mut conn = self.conn.lock().await;
        let mut sim = None;
        let transport = match profile.transport {
            TransportKind::Tcp => TransportConfig::Tcp {
                host: profile.host.trim().to_string(),
                port: profile.port,
                connect_timeout_ms: settings.timeout_connect_ms,
            },
            TransportKind::Serial => {
                if profile.serial_port.trim().is_empty() {
                    return Err("Nessuna porta seriale scelta nel profilo (Impostazioni)".into());
                }
                TransportConfig::Serial {
                    port: profile.serial_port.trim().to_string(),
                    baud: profile.baud,
                    flow: match profile.flow_control {
                        crate::config::FlowControlCfg::Xonxoff => FlowControl::XonXoff,
                        crate::config::FlowControlCfg::None => FlowControl::None,
                        crate::config::FlowControlCfg::Hardware => FlowControl::Hardware,
                    },
                }
            }
            TransportKind::Demo => {
                let h = Simulator::start(SimConfig {
                    ports: profile.demo_ports.max(2),
                    bind: "127.0.0.1:0".parse().unwrap(),
                    verbose: true,
                    bcast_target: None,
                    bcast_ms: 4000,
                    rev_m: true,
                    cmd_delay_ms: 15,
                    ..SimConfig::default()
                })
                .await
                .map_err(|e| format!("Avvio del simulatore fallito: {e}"))?;
                let addr = h.local_addr;
                sim = Some(h);
                self.log(
                    "info",
                    "demo",
                    &format!("Simulatore TLX{} avviato su {addr}", profile.demo_ports),
                    None,
                );
                TransportConfig::Tcp {
                    host: addr.ip().to_string(),
                    port: addr.port(),
                    connect_timeout_ms: 2000,
                }
            }
        };
        let udp = if profile.transport == TransportKind::Tcp && profile.udp_listen {
            let mut accept = vec![];
            if let Ok(ip) = profile.host.trim().parse::<std::net::Ipv4Addr>() {
                let o = ip.octets();
                accept.push(ip);
                accept.push(std::net::Ipv4Addr::new(o[0], o[1], o[2], o[3].wrapping_add(100)));
                accept.push(std::net::Ipv4Addr::new(o[0], o[1], o[2], o[3].wrapping_add(1)));
            }
            Some(UdpConfig {
                bind_port: 17564,
                mcast_group: profile.udp_mcast_group.trim().parse().ok(),
                accept_from: accept,
            })
        } else {
            None
        };
        let cc = ClientConfig {
            transport,
            connect_timeout_ms: settings.timeout_connect_ms,
            cmd_timeout_ms: settings.timeout_cmd_ms,
            status_timeout_base_ms: settings.timeout_cmd_ms,
            heartbeat_s: settings.heartbeat_s,
            poll_xget_s: settings.poll_xget_s,
            delay_ms: settings.delay_ms,
            udp,
            ..Default::default()
        };
        let client = MatrixClient::connect(cc);
        let mut rx = client.subscribe();
        // L'attore del client può aver già emesso «connecting» prima della nostra iscrizione:
        // quell'evento è perduto. Scriviamo subito lo stato corrente in cache (e lo emettiamo),
        // altrimenti la UI resterebbe su «disconnected» e il profilo connesso (usato per
        // etichette, blocchi e preset) resterebbe quello vecchio fino al primo evento utile.
        {
            let dto = ConnStateDto::from_core(&client.conn_state(), &profile.id);
            self.cache.lock().unwrap().conn = Some(dto.clone());
            let _ = self.handle.emit("tlx://conn", dto);
        }
        let me = self.handle.clone();
        let pid = profile.id.clone();
        let fwd = tauri::async_runtime::spawn(async move {
            use tauri::Manager;
            loop {
                match rx.recv().await {
                    Ok(ev) => {
                        if let Some(st) = me.try_state::<std::sync::Arc<App>>() {
                            st.on_event(&pid, ev);
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
        });
        conn.client = Some(client);
        conn.sim = sim;
        conn.profile_id = Some(profile.id.clone());
        conn.forwarder = Some(fwd);
        Ok(())
    }

    pub async fn disconnect(&self) {
        let mut conn = self.conn.lock().await;
        if let Some(c) = conn.client.take() {
            c.shutdown().await;
        }
        if let Some(f) = conn.forwarder.take() {
            f.abort();
        }
        if let Some(s) = conn.sim.take() {
            s.shutdown();
        }
        let pid = conn.profile_id.take();
        drop(conn);
        let st = ConnStateDto {
            status: "disconnected".into(),
            profile_id: pid,
            endpoint: String::new(),
            error: None,
            since: now_ms(),
        };
        {
            let mut c = self.cache.lock().unwrap();
            c.conn = Some(st.clone());
            c.info = None;
            c.routing = None;
            c.diag = None;
            c.undo = None;
        }
        let _ = self.handle.emit("tlx://conn", st);
    }

    pub async fn shutdown(&self) {
        self.disconnect().await;
    }

    fn on_event(&self, profile_id: &str, ev: ClientEvent) {
        match ev {
            ClientEvent::Conn(s) => {
                let dto = ConnStateDto::from_core(&s, profile_id);
                self.cache.lock().unwrap().conn = Some(dto.clone());
                let _ = self.handle.emit("tlx://conn", dto);
            }
            ClientEvent::Info(i) => {
                let dto = MatrixInfoDto::from(i);
                self.cache.lock().unwrap().info = Some(dto.clone());
                let _ = self.handle.emit("tlx://info", dto);
            }
            ClientEvent::Routing(r) => {
                let dto = RoutingDto::from_core(&r);
                self.cache.lock().unwrap().routing = Some(dto.clone());
                let _ = self.handle.emit("tlx://routing", dto);
            }
            ClientEvent::Log(l) => {
                self.log(&l.level.to_string(), &l.channel.to_string(), &l.message, l.latency_ms);
            }
        }
    }

    async fn client(&self) -> Result<MatrixClient, String> {
        self.conn.lock().await.client.clone().ok_or_else(|| "Matrice non connessa".to_string())
    }

    pub fn snapshot(&self) -> SnapshotDto {
        let c = self.cache.lock().unwrap();
        SnapshotDto {
            conn: c.conn.clone().unwrap_or(ConnStateDto {
                status: "disconnected".into(),
                profile_id: None,
                endpoint: String::new(),
                error: None,
                since: now_ms(),
            }),
            info: c.info.clone(),
            routing: c.routing.clone(),
            diagnostics: c.diag.clone(),
            can_undo: c.undo.is_some(),
        }
    }

    // ───────────────────────── letture ─────────────────────────

    pub async fn refresh(&self) -> Result<RoutingDto, String> {
        let client = self.client().await?;
        let r = client.refresh().await.map_err(|e| e.to_string())?;
        let dto = RoutingDto {
            max_ports: r.max_ports,
            outputs: r.outputs.clone(),
            source: "xget".into(),
            updated_at: now_ms(),
        };
        self.cache.lock().unwrap().routing = Some(dto.clone());
        let _ = self.handle.emit("tlx://routing", dto.clone());
        Ok(dto)
    }

    pub async fn diagnostics(&self) -> Result<DiagnosticsDto, String> {
        let client = self.client().await?;
        let d = client.diagnostics().await.map_err(|e| e.to_string())?;
        let dto = DiagnosticsDto::from_core(d, now_ms());
        self.cache.lock().unwrap().diag = Some(dto.clone());
        let _ = self.handle.emit("tlx://diag", dto.clone());
        Ok(dto)
    }

    pub async fn raw(&self, command: &str, force: bool) -> Result<CmdResultDto, String> {
        let cmd = command.trim().to_ascii_uppercase();
        if cmd.is_empty() {
            return Err("Comando vuoto".into());
        }
        if cmd.chars().any(|c| !c.is_ascii_alphanumeric() && c != ';') {
            return Err("Il comando può contenere solo lettere, cifre e «;»".into());
        }
        if !force {
            if let Some(why) = tlx_core::protocol::is_dangerous(&cmd) {
                return Err(format!("PERICOLOSO: {why}"));
            }
        }
        if cmd.starts_with("XHELP") {
            return Err("XHELP risponde su più righe e non è gestito dalla console".into());
        }
        let client = self.client().await?;
        let _g = self.op.lock().await;
        let out = client.raw(&cmd).await;
        // un comando di commutazione dalla console rende vecchi stato e annulla
        if ["CI", "DI", "DO", "M", "XPUT", "XRESET", "XTEST"].iter().any(|p| cmd.starts_with(p)) {
            self.cache.lock().unwrap().undo = None;
            let _ = self.read_outputs(&client).await;
        }
        Ok(CmdResultDto::from_core(out))
    }

    // ───────────────────────── commutazioni ─────────────────────────

    fn set_routing(&self, outputs: Outputs, source: &str) {
        let dto = RoutingDto {
            max_ports: outputs.len() as u16,
            outputs,
            source: source.into(),
            updated_at: now_ms(),
        };
        self.cache.lock().unwrap().routing = Some(dto.clone());
        let _ = self.handle.emit("tlx://routing", dto);
    }

    async fn read_outputs(&self, client: &MatrixClient) -> Result<Outputs, String> {
        let r = client
            .refresh()
            .await
            .map_err(|e| format!("Lettura dello stato (XGET) fallita: {e}"))?;
        self.set_routing(r.outputs.clone(), "xget");
        Ok(r.outputs)
    }

    fn names(&self, ports: &[u16]) -> String {
        ports.iter().map(|p| self.name(*p)).collect::<Vec<_>>().join(", ")
    }

    /// Esegue un piano sullo stato `before` appena letto: stop al primo errore, attesa
    /// `delay_ms`, rilettura dell'intera matrice (una seconda volta se non corrisponde),
    /// confronto, avvisi di sicurezza, record per l'annulla.
    async fn execute(
        &self,
        client: &MatrixClient,
        before: Outputs,
        pl: Plan,
        what: &str,
        record_undo: bool,
    ) -> ActionResultDto {
        let settings = self.settings();
        let mut warnings = pl.warnings.clone();
        if pl.commands.is_empty() {
            return ActionResultDto {
                ok: true,
                steps: vec![],
                verified: Some(true),
                mismatches: vec![],
                message: format!("{what}: niente da fare"),
                warnings,
            };
        }
        self.log("info", "app", &format!("— {what} —"), None);
        let mut steps = vec![];
        let mut sent_ok = vec![];
        let mut failure: Option<(String, bool)> = None; // (descrizione, esito ignoto)
        for cmd in &pl.commands {
            let out = CmdResultDto::from_core(client.raw(cmd).await);
            let good = out.ok;
            if good {
                sent_ok.push(cmd.clone());
            } else if let Some(e) = &out.error {
                failure = Some((
                    format!("nessuna risposta a «{cmd}» ({e}): non si sa se è stato eseguito"),
                    true,
                ));
            } else {
                let why = out
                    .comment
                    .clone()
                    .or_else(|| out.error_code.clone().map(|c| format!("errore {c}")))
                    .unwrap_or_default();
                failure = Some((format!("la matrice ha rifiutato «{cmd}» ({why})"), false));
            }
            steps.push(out);
            if !good {
                break;
            }
        }
        let expected = plan::simulate(&before, &sent_ok);
        // rilettura: dopo il ritardo di spegnimento; se non torna, un secondo tentativo
        tokio::time::sleep(Duration::from_millis(settings.delay_ms.clamp(50, 1000))).await;
        let mut read = self.read_outputs(client).await;
        if matches!(&read, Ok(r) if *r != expected) {
            tokio::time::sleep(Duration::from_millis(700)).await;
            read = self.read_outputs(client).await;
        }
        let (verified, mismatches, after) = match read {
            Ok(after) => {
                let m = plan::verify(&expected, &after);
                (Some(m.is_empty()), m, Some(after))
            }
            Err(e) => (None, vec![e], None),
        };
        if let Some(after) = &after {
            // effetti collaterali (es. bidir esclusivo che spegne altri monitor)
            let declared: Vec<u16> = pl.intent.iter().map(|(_, o)| *o).collect();
            let off: Vec<u16> = plan::changed(&before, after)
                .into_iter()
                .filter(|o| !declared.contains(o) && after[*o as usize - 1] == 0)
                .collect();
            if !off.is_empty() && failure.is_none() {
                warnings.push(format!("Spente anche le uscite: {}", self.names(&off)));
            }
            let roles = {
                let pid = self.connected_profile().unwrap_or_default();
                self.cfg.lock().unwrap().roles(&pid)
            };
            warnings.extend(plan::safety_warnings(after, &roles));
            if record_undo && before != *after {
                self.cache.lock().unwrap().undo =
                    Some(UndoRec { before: before.clone(), after: after.clone() });
            }
        }
        let ok = failure.is_none() && verified == Some(true);
        let message = match (&failure, verified) {
            (Some((f, _)), _) => format!("{what}: {f}. Comandi successivi non inviati."),
            (None, Some(true)) => format!("{what}: fatto e verificato"),
            (None, Some(false)) => {
                format!("{what}: comandi accettati ma la matrice non è nello stato atteso")
            }
            (None, None) => format!("{what}: esito non verificato, controlla la matrice"),
        };
        self.log(if ok { "info" } else { "error" }, "app", &message, None);
        for m in &mismatches {
            self.log("warn", "app", m, None);
        }
        for w in &warnings {
            self.log("warn", "app", w, None);
        }
        ActionResultDto { ok, steps, verified, mismatches, message, warnings }
    }

    /// Commutazione: lettura fresca (XGET) sotto il lock delle operazioni, pianificazione,
    /// controllo dei blocchi sullo stato simulato (anche gli effetti collaterali dei DI),
    /// conflitti interni al piano, esecuzione.
    async fn run_plan<F>(&self, what: String, build: F) -> Result<ActionResultDto, String>
    where
        F: FnOnce(&Outputs, &plan::Ctx, &[u16]) -> Result<Plan, String>,
    {
        let client = self.client().await?;
        let _g = self.op.lock().await;
        self.run_plan_locked(&client, what, build, true).await
    }

    async fn run_plan_locked<F>(
        &self,
        client: &MatrixClient,
        what: String,
        build: F,
        record_undo: bool,
    ) -> Result<ActionResultDto, String>
    where
        F: FnOnce(&Outputs, &plan::Ctx, &[u16]) -> Result<Plan, String>,
    {
        let before = self.read_outputs(client).await?;
        let pid = self.connected_profile().unwrap_or_default();
        let (locked, roles) = {
            let c = self.cfg.lock().unwrap();
            (c.locked_ports(&pid), c.roles(&pid))
        };
        let p2p = self.cache.lock().unwrap().info.as_ref().map(|i| i.p2p).unwrap_or(false);
        let ctx = plan::Ctx { max: before.len() as u16, roles: &roles, p2p };
        let mut pl = build(&before, &ctx, &locked)?;
        let expected = plan::simulate(&before, &pl.commands);
        let hits = plan::locked_hits(&before, &expected, &locked);
        if !hits.is_empty() {
            return Err(format!(
                "Operazione annullata: cambierebbe le uscite bloccate {} (sbloccale in «Porte» o scegli altre porte)",
                self.names(&hits)
            ));
        }
        for (i, o) in plan::broken_intent(&pl.intent, &expected) {
            pl.warnings.push(format!(
                "Il collegamento {} → {} viene sostituito da un collegamento successivo dello stesso preset",
                self.name(i),
                self.name(o)
            ));
        }
        Ok(self.execute(client, before, pl, &what, record_undo).await)
    }

    fn name(&self, p: u16) -> String {
        let pid = self.connected_profile().unwrap_or_default();
        self.cfg.lock().unwrap().label_name(&pid, p)
    }

    pub async fn link(&self, a: u16, b: u16, mode: LinkMode) -> Result<ActionResultDto, String> {
        let strat = self.settings().bidir_strategy;
        let what = match mode {
            LinkMode::Bidir => format!("Collegamento {} ↔ {}", self.name(a), self.name(b)),
            LinkMode::Kvm => format!("Postazione {} ← PC {}", self.name(b), self.name(a)),
            LinkMode::Oneway => format!("Video {} → {}", self.name(a), self.name(b)),
        };
        self.run_plan(what, |_, ctx, _| plan::plan_link(a, b, mode, strat, ctx)).await
    }

    pub async fn apply_routes(&self, changes: Vec<(u16, u16)>) -> Result<ActionResultDto, String> {
        let what = format!("Applicazione di {} modifiche", changes.len());
        self.run_plan(what, |_, ctx, _| plan::plan_routes(&changes, ctx)).await
    }

    pub async fn disconnect_port(
        &self,
        port: u16,
        mode: plan::DiscMode,
    ) -> Result<ActionResultDto, String> {
        let what = format!("Scollegamento {}", self.name(port));
        self.run_plan(what, |_, ctx, _| plan::plan_disconnect(port, mode, ctx)).await
    }

    pub async fn disconnect_all(&self) -> Result<ActionResultDto, String> {
        self.run_plan("Scollegamento di tutte le uscite".into(), |cur, _, locked| {
            plan::plan_disconnect_all(cur, locked)
        })
        .await
    }

    pub async fn execute_preset(
        &self,
        profile_id: &str,
        preset_id: &str,
    ) -> Result<ActionResultDto, String> {
        if self.connected_profile().as_deref() != Some(profile_id) {
            return Err(
                "Il preset appartiene a un'altra matrice: seleziona prima quella matrice".into()
            );
        }
        let preset = {
            let c = self.cfg.lock().unwrap();
            c.presets
                .get(profile_id)
                .and_then(|v| v.iter().find(|p| p.id == preset_id))
                .cloned()
                .ok_or("Preset inesistente")?
        };
        if preset.links.is_empty() && !preset.scene {
            return Err(format!("Il preset «{}» non contiene collegamenti", preset.name));
        }
        let strat = self.settings().bidir_strategy;
        self.run_plan(format!("Preset «{}»", preset.name), |cur, ctx, locked| {
            if preset.scene {
                let links: Vec<_> = preset.links.iter().map(|l| (l.a, l.b, l.mode)).collect();
                let target = plan::scene_target(&links, ctx.max)?;
                plan::plan_scene(&target, cur, locked, ctx)
            } else {
                let plans = preset
                    .links
                    .iter()
                    .map(|l| plan::plan_link(l.a, l.b, l.mode, strat, ctx))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(plan::concat(plans))
            }
        })
        .await
    }

    /// Annulla: solo se le uscite toccate sono ancora come le ha lasciate l'operazione.
    pub async fn undo(&self) -> Result<ActionResultDto, String> {
        let client = self.client().await?;
        let _g = self.op.lock().await;
        let rec = self.cache.lock().unwrap().undo.clone().ok_or("Niente da annullare")?;
        let now = self.read_outputs(&client).await?;
        let touched = plan::changed(&rec.before, &rec.after);
        if touched.iter().any(|o| now.get(*o as usize - 1) != rec.after.get(*o as usize - 1)) {
            self.cache.lock().unwrap().undo = None;
            return Err("La matrice è cambiata dopo l'ultima operazione: annulla non sicuro, operazione non eseguita".into());
        }
        let r = self
            .run_plan_locked(
                &client,
                "Annulla ultima operazione".into(),
                |_, ctx, _| plan::plan_restore(&rec.before, &touched, ctx),
                false,
            )
            .await?;
        self.cache.lock().unwrap().undo = None;
        Ok(r)
    }

    /// Situazione attuale → link di una scena (rilegge prima la matrice).
    pub async fn snapshot_links(&self) -> Result<Vec<(u16, u16, LinkMode)>, String> {
        let client = self.client().await?;
        let _g = self.op.lock().await;
        let cur = self.read_outputs(&client).await?;
        Ok(plan::routing_to_links(&cur))
    }
}

pub fn fmt_ts(ms: u64) -> String {
    // ora locale non disponibile senza dipendenze: UTC ISO, sufficiente per il CSV
    let secs = ms / 1000;
    let (d, s) = (secs / 86400, secs % 86400);
    let (h, m, sec) = (s / 3600, (s % 3600) / 60, s % 60);
    // giorni → data civile (algoritmo di Howard Hinnant)
    let z = d as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    format!("{year:04}-{month:02}-{day:02} {h:02}:{m:02}:{sec:02} UTC")
}
