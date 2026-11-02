//! Test di integrazione di `tlx-core` contro il simulatore in-process.
//!
//! Coprono il percorso completo trasporto → sessione → client: handshake, la sequenza
//! bidirezionale dello script Python, un errore `ER0007` del costruttore, la caduta della
//! connessione con riconnessione automatica e lo stato ricevuto dal broadcast UDP 17564.

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

use tlx_core::client::{ClientConfig, MatrixClient, UdpConfig, NOT_CONNECTED};
use tlx_core::events::{ClientEvent, RoutingSource};
use tlx_core::sim::{SimConfig, SimHandle, Simulator, SIM_VERSION};
use tlx_core::transport::TransportConfig;

/// Avvia un simulatore su una porta effimera di loopback.
async fn simulatore(cfg: SimConfig) -> SimHandle {
    Simulator::start(SimConfig { bind: SocketAddr::from(([127, 0, 0, 1], 0)), ..cfg })
        .await
        .expect("il simulatore deve partire sul loopback")
}

/// Configurazione del client verso un simulatore, senza heartbeat né polling: i test
/// controllano loro quando la matrice viene interrogata.
fn conf(addr: SocketAddr) -> ClientConfig {
    ClientConfig {
        transport: TransportConfig::Tcp {
            host: addr.ip().to_string(),
            port: addr.port(),
            connect_timeout_ms: 1000,
        },
        cmd_timeout_ms: 1500,
        status_timeout_base_ms: 1500,
        heartbeat_s: 0,
        poll_xget_s: 0,
        delay_ms: 0,
        ..ClientConfig::default()
    }
}

/// Client connesso e con handshake concluso.
async fn connesso(addr: SocketAddr, cfg: ClientConfig) -> MatrixClient {
    let c = MatrixClient::connect(ClientConfig { ..cfg });
    c.wait_connected(Duration::from_secs(10))
        .await
        .unwrap_or_else(|e| panic!("il client deve connettersi a {addr}: {e}"));
    c
}

#[tokio::test]
async fn handshake_legge_le_informazioni_della_matrice() {
    let sim = simulatore(SimConfig { ports: 48, ..SimConfig::default() }).await;
    let c = connesso(sim.local_addr, conf(sim.local_addr)).await;

    let info = c.info().expect("l'handshake deve aver popolato MatrixInfo");
    assert_eq!(info.version, SIM_VERSION);
    assert_eq!(info.xtype, "TLX48Switch");
    assert_eq!(info.max_ports, 48);
    assert_eq!(info.max_cards, 3);
    assert_eq!(info.installed, vec![true, true, true]);
    assert!(info.real_card_count, "bit 3 di XOPTIONS");
    assert!(info.p2p, "bit 4 di XOPTIONS");
    assert_eq!(info.los_supported, Some(true), "il simulatore è Rev. M");

    // L'attore esegue una XGET subito dopo l'handshake.
    let r = c.routing().expect("routing iniziale");
    assert_eq!(r.routing.max_ports, 48);
    assert_eq!(r.routing.connections(), Vec::new());
    assert_eq!(r.source, RoutingSource::Xget);

    c.shutdown().await;
}

#[tokio::test]
async fn handshake_senza_rev_m_disattiva_la_diagnostica_ottica() {
    let sim = simulatore(SimConfig { ports: 24, rev_m: false, ..SimConfig::default() }).await;
    let c = connesso(sim.local_addr, conf(sim.local_addr)).await;

    // Un ER su XLOS9999 non deve far fallire l'handshake (tollerante: §4 delle decisioni).
    assert_eq!(c.info().unwrap().los_supported, Some(false));
    let d = c.diagnostics().await.expect("XALARM resta disponibile");
    assert!(d.los.is_none() && d.rx_uw.is_none() && d.tx_uw.is_none());

    c.shutdown().await;
}

#[tokio::test]
async fn bidir_dello_script_e_esclusivo_su_entrambe_le_porte() {
    let sim = simulatore(SimConfig { ports: 24, ..SimConfig::default() }).await;
    // Stato di partenza: il codec (1) alimenta anche il monitor 8, il tablet (5) il monitor 9.
    sim.execute("CI0001O0008");
    sim.execute("CI0005O0009");

    let c = connesso(sim.local_addr, conf(sim.local_addr)).await;
    let (a, b) = (1u16, 5u16);

    // La sequenza dello script Python: DI a, DI b, CI a O b, CI b O a.
    for o in [c.di(&[a]).await, c.di(&[b]).await, c.ci(a, &[b]).await, c.ci(b, &[a]).await] {
        let out = o.expect("comando inviato");
        assert!(out.ok(), "comando {} fallito: {:?}", out.command, out);
    }

    let r = c.refresh().await.expect("XGET dopo la commutazione");
    assert_eq!(r.input_of(b), a, "video del codec sul tablet");
    assert_eq!(r.input_of(a), b, "ritorno del tablet sul codec");
    assert_eq!(r.input_of(8), 0, "il DI ha spento il monitor 8 (modalità esclusiva)");
    assert_eq!(r.input_of(9), 0, "il DI ha spento il monitor 9 (modalità esclusiva)");
    assert_eq!(r.connections(), vec![(a, b), (b, a)]);

    // Il simulatore vede lo stesso stato del client.
    assert_eq!(sim.routing(), r);

    c.shutdown().await;
}

#[tokio::test]
async fn errore_del_costruttore_non_rompe_la_sessione() {
    let sim = simulatore(SimConfig { ports: 24, ..SimConfig::default() }).await;
    let c = connesso(sim.local_addr, conf(sim.local_addr)).await;

    let out = c.raw("CI0004O0099").await;
    assert!(!out.ok(), "porta 99 su una matrice da 24: deve essere un errore");
    assert_eq!(out.error_code(), Some("0007"));
    assert!(out.error.is_none(), "ER non è un errore di trasporto");
    let msg = out.error_message().expect("messaggio d'errore");
    assert!(msg.contains("0007"), "messaggio inatteso: {msg}");

    // La sessione resta valida: il comando successivo passa.
    let ok = c.ci(4, &[7]).await.expect("comando inviato");
    assert!(ok.ok(), "{ok:?}");
    assert_eq!(c.refresh().await.unwrap().input_of(7), 4);
    assert!(c.is_connected());

    c.shutdown().await;
}

#[tokio::test]
async fn riconnessione_automatica_dopo_la_caduta_del_collegamento() {
    let sim = simulatore(SimConfig { ports: 24, ..SimConfig::default() }).await;
    let cfg = ClientConfig { reconnect_backoff_s: vec![1], ..conf(sim.local_addr) };
    let c = connesso(sim.local_addr, cfg).await;
    let mut ev = c.subscribe();

    sim.execute("CI0002O0006");
    sim.drop_clients(); // failover del controller: tutte le connessioni cadono

    // Il comando dopo la caduta fallisce; `drop_clients()` però chiude le connessioni al
    // prossimo risveglio del simulatore, che in quel momento potrebbe servire comunque il
    // comando: l'esito del singolo comando non è deterministico, la caduta sì. Si insiste
    // finché la connessione non cade davvero.
    for _ in 0..20 {
        if !c.raw("XGET").await.ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // L'attore deve annunciare uno stato diverso da «connected» e poi tornare connesso.
    let visto_caduta = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(ClientEvent::Conn(s)) = ev.recv().await {
                if s.status.to_string() != "connected" {
                    return s.status.to_string();
                }
            }
        }
    })
    .await
    .expect("il client deve segnalare la caduta");
    assert!(
        ["connecting", "reconnecting", "handshaking", "error", "disconnected"]
            .contains(&visto_caduta.as_str()),
        "stato inatteso: {visto_caduta}"
    );

    c.wait_connected(Duration::from_secs(20)).await.expect("il client riprova per sempre");
    let r = c.refresh().await.expect("XGET dopo la riconnessione");
    assert_eq!(r.input_of(6), 2, "lo stato della matrice è rimasto");

    c.shutdown().await;
}

#[tokio::test]
async fn comandi_rifiutati_mentre_la_matrice_non_risponde() {
    // Porta libera, nessun simulatore in ascolto: il client resta in riconnessione.
    let porta = porta_libera();
    let c = MatrixClient::connect(conf(SocketAddr::from(([127, 0, 0, 1], porta))));
    assert!(c.wait_connected(Duration::from_millis(1500)).await.is_err());

    let out = c.raw("XGET").await;
    assert!(!out.ok());
    assert_eq!(out.error.as_deref(), Some(NOT_CONNECTED));
    assert!(c.refresh().await.is_err());

    c.shutdown().await;
}

#[tokio::test]
async fn stato_aggiornato_dal_broadcast_udp_17564() {
    // La porta d'ascolto si sceglie sondandola, e fra il sondaggio e il `bind` del client un
    // altro processo può prenderla; con `SO_REUSEADDR` la collisione non dà errore, i due
    // socket si spartiscono i datagrammi e l'attesa scade. Si riprova su una porta nuova.
    let mut ultimo = String::new();
    for tentativo in 1..=3 {
        match prova_broadcast(porta_libera()).await {
            Ok(()) => return,
            Err(e) => ultimo = format!("tentativo {tentativo}: {e}"),
        }
    }
    panic!("nessuno stato dal broadcast UDP in 3 tentativi ({ultimo})");
}

/// Un giro di prova del broadcast su una porta data. `Err` = nessun datagramma utile in
/// tempo (porta probabilmente contesa): il chiamante riprova. Un valore sbagliato è invece
/// un difetto e fa fallire subito il test.
async fn prova_broadcast(porta_udp: u16) -> Result<(), String> {
    let sim = simulatore(SimConfig {
        ports: 24,
        bcast_target: Some(SocketAddr::from(([127, 0, 0, 1], porta_udp))),
        bcast_ms: 100,
        ..SimConfig::default()
    })
    .await;

    let cfg = ClientConfig {
        bcast_period_ms: 150,
        udp: Some(UdpConfig {
            bind_port: porta_udp,
            mcast_group: None,
            accept_from: vec![Ipv4Addr::LOCALHOST],
        }),
        ..conf(sim.local_addr)
    };
    let c = connesso(sim.local_addr, cfg).await;
    let mut ev = c.subscribe();

    // Commutazione fatta sulla matrice, non dal client: solo il broadcast può rivelarla.
    sim.execute("CI0003O0010");

    let atteso = tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            if let Ok(ClientEvent::Routing(u)) = ev.recv().await {
                if u.source == RoutingSource::Udp {
                    return u;
                }
            }
        }
    })
    .await;

    let Ok(upd) = atteso else {
        c.shutdown().await;
        return Err(format!("nessun datagramma utile sulla porta {porta_udp} entro 8 s"));
    };

    assert_eq!(upd.routing.max_ports, 24);
    assert_eq!(upd.routing.input_of(10), 3, "lo stato arrivato dal broadcast è sbagliato");
    assert_eq!(c.routing().unwrap().source, RoutingSource::Udp);

    c.shutdown().await;
    Ok(())
}

/// Una porta UDP libera sul loopback (il socket viene chiuso subito). Fra la chiusura e il
/// `bind` del client c'è una finestra in cui un altro processo può prendere la porta, e il
/// listener usa `SO_REUSEADDR`, quindi una collisione non darebbe errore: chi la usa per
/// l'ascolto deve riprovare su una porta nuova (vedi `stato_aggiornato_dal_broadcast_udp_17564`).
fn porta_libera() -> u16 {
    let s = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).expect("porta UDP libera");
    s.local_addr().expect("indirizzo locale").port()
}

// ---------------------------------------------------------------------------
// Matrice finta: risponde all'handshake e tace sui comandi indicati. Serve per provare il
// timeout di un comando, che il simulatore non può produrre selettivamente (`cmd_delay_ms`
// ritarda ogni risposta, handshake compreso).
// ---------------------------------------------------------------------------

/// Risposta canonica a un comando, con il campo lunghezza a `0000` come nei log del
/// costruttore: il client deve ignorarlo (spec §3.2 regola 1).
fn risposta_finta(cmd: &str) -> Option<String> {
    let dati = match cmd {
        "XVERSION" => "V5.07.07",
        "XOPTIONS" => "0120",
        "XTYPE" => "TLX24Switch",
        "XMAXPORTS" => "0024",
        "XMAXCARDS" => "0001",
        "XINSTALLED" => "1",
        "XPORTCONFIG" => "0024,0000,0000",
        "XLOS9999" => "L000000000000000000000000",
        "XLASTEVENT" => "Thu Jul  1 11:23:52 2010",
        "XGET" | "XQUIT" => "",
        _ => return None,
    };
    Some(format!("R0000OK{dati}"))
}

struct MatriceFinta {
    addr: SocketAddr,
    conteggio: Arc<Mutex<HashMap<String, usize>>>,
    _task: tokio::task::JoinHandle<()>,
}

impl MatriceFinta {
    /// Quante volte la matrice ha ricevuto quel comando, su tutte le connessioni.
    fn quante(&self, cmd: &str) -> usize {
        self.conteggio.lock().expect("contatore").get(cmd).copied().unwrap_or(0)
    }
}

/// Avvia una matrice finta sul loopback che risponde all'handshake e **non risponde mai** ai
/// comandi elencati in `mute`, lasciando il client in timeout.
async fn matrice_finta(mute: &'static [&'static str]) -> MatriceFinta {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.expect("ascolto sul loopback");
    let addr = listener.local_addr().expect("indirizzo locale");
    let conteggio: Arc<Mutex<HashMap<String, usize>>> = Arc::new(Mutex::new(HashMap::new()));
    let c2 = conteggio.clone();
    let task = tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let c3 = c2.clone();
            tokio::spawn(async move {
                let (rd, mut wr) = stream.into_split();
                let mut reader = BufReader::new(rd);
                let mut riga = String::new();
                loop {
                    riga.clear();
                    match reader.read_line(&mut riga).await {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                    let cmd = riga.trim().to_ascii_uppercase();
                    if cmd.is_empty() {
                        continue;
                    }
                    *c3.lock().expect("contatore").entry(cmd.clone()).or_insert(0) += 1;
                    if mute.contains(&cmd.as_str()) {
                        continue; // nessuna risposta: il client deve andare in timeout
                    }
                    let r = risposta_finta(&cmd).unwrap_or_else(|| "R0000ER0001".to_string());
                    if wr.write_all(format!("{r}\n").as_bytes()).await.is_err() {
                        break;
                    }
                }
            });
        }
    });
    MatriceFinta { addr, conteggio, _task: task }
}

#[tokio::test]
async fn un_comando_in_timeout_non_viene_mai_ripetuto() {
    let m = matrice_finta(&["CI0003O0010"]).await;
    let cfg = ClientConfig {
        cmd_timeout_ms: 400,
        status_timeout_base_ms: 1500,
        reconnect_backoff_s: vec![1],
        ..conf(m.addr)
    };
    let c = MatrixClient::connect(cfg);
    c.wait_connected(Duration::from_secs(10)).await.expect("handshake sulla matrice finta");

    let out = c.raw("CI0003O0010").await;
    assert!(!out.ok(), "un comando senza risposta non può risultare riuscito: {out:?}");
    assert!(out.error.is_some(), "deve essere un errore di trasporto/timeout, non un ERnnnn");
    assert!(out.error_code().is_none());
    assert!(out.response.is_none(), "nessuna riga ricevuta");

    // Dopo il timeout la sessione è compromessa: si chiude, si riconnette, si rifà l'handshake.
    c.wait_connected(Duration::from_secs(25)).await.expect("il client riprova per sempre");
    assert!(c.refresh().await.is_ok(), "la sessione nuova funziona");

    // Spec §5.1: i comandi di commutazione andati in timeout non si ripetono mai.
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(
        m.quante("CI0003O0010"),
        1,
        "il comando andato in timeout è stato inviato più di una volta"
    );
    assert!(m.quante("XVERSION") >= 2, "l'handshake deve essere stato rifatto");

    c.shutdown().await;
}
