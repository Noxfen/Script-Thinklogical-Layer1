//! `tlx-sim` — simulatore di matrice Thinklogical TLX a riga di comando.
//!
//! Espone l'API ASCII su TCP e, se richiesto, il broadcast di stato UDP 17564.
//! Serve per provare TLX Control senza una matrice reale e per i test di integrazione.

use std::net::SocketAddr;
use std::process::ExitCode;

use tlx_core::sim::{cards_for, options_for, SimConfig, Simulator, SIM_VERSION};

const USO: &str = "\
tlx-sim — simulatore di matrice Thinklogical TLX

Uso:
  tlx-sim [opzioni]

Opzioni:
  --ports <N>          numero di porte (default 24)
  --listen <IP:PORTA>  ascolto dell'API ASCII (default 0.0.0.0:17567)
  --bcast <IP:PORTA>   destinazione del broadcast di stato UDP (es. 192.168.13.255:17564
                       oppure 239.255.13.9:17564); assente = broadcast disattivato
  --bcast-ms <MS>      periodo del broadcast (default 4000)
  --delay-ms <MS>      ritardo artificiale prima di ogni risposta (default 0)
  --no-verbose         risposte senza il commento '#...'
  --no-rev-m           firmware senza XLOS/XRXPWR/XTXPWR (rispondono ER0001)
  --macro-partial      Macro non atomico: applica le coppie fino alla prima non valida,
                       invece di rifiutare tutto il comando (verifica V8 aperta)
  -h, --help           questo aiuto
";

struct Opzioni {
    ports: u16,
    listen: SocketAddr,
    bcast: Option<SocketAddr>,
    bcast_ms: u64,
    delay_ms: u64,
    verbose: bool,
    rev_m: bool,
    macro_partial: bool,
}

impl Default for Opzioni {
    fn default() -> Self {
        Opzioni {
            ports: 24,
            listen: "0.0.0.0:17567".parse().expect("indirizzo di default valido"),
            bcast: None,
            bcast_ms: 4000,
            delay_ms: 0,
            verbose: true,
            rev_m: true,
            macro_partial: false,
        }
    }
}

/// Legge il valore che segue un'opzione, o fallisce con un messaggio leggibile.
fn valore(it: &mut impl Iterator<Item = String>, opt: &str) -> Result<String, String> {
    it.next().ok_or_else(|| format!("l'opzione {opt} richiede un valore"))
}

/// Risolve un `IP:PORTA`; accetta anche un nome host.
fn indirizzo(s: &str, opt: &str) -> Result<SocketAddr, String> {
    use std::net::ToSocketAddrs;
    if let Ok(a) = s.parse::<SocketAddr>() {
        return Ok(a);
    }
    s.to_socket_addrs()
        .map_err(|e| format!("{opt}: indirizzo «{s}» non valido ({e})"))?
        .next()
        .ok_or_else(|| format!("{opt}: nessun indirizzo per «{s}»"))
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Option<Opzioni>, String> {
    let mut o = Opzioni::default();
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-h" | "--help" => return Ok(None),
            "--ports" => {
                let v = valore(&mut it, "--ports")?;
                o.ports = v
                    .parse::<u16>()
                    .map_err(|_| format!("--ports: «{v}» non è un numero di porte valido"))?;
                if o.ports == 0 {
                    return Err("--ports: deve essere almeno 1".into());
                }
            }
            "--listen" => {
                let v = valore(&mut it, "--listen")?;
                o.listen = indirizzo(&v, "--listen")?;
            }
            "--bcast" => {
                let v = valore(&mut it, "--bcast")?;
                o.bcast = Some(indirizzo(&v, "--bcast")?);
            }
            "--bcast-ms" => {
                let v = valore(&mut it, "--bcast-ms")?;
                o.bcast_ms =
                    v.parse::<u64>().map_err(|_| format!("--bcast-ms: «{v}» non è un numero"))?;
                if o.bcast_ms == 0 {
                    return Err("--bcast-ms: deve essere maggiore di 0".into());
                }
            }
            "--delay-ms" => {
                let v = valore(&mut it, "--delay-ms")?;
                o.delay_ms =
                    v.parse::<u64>().map_err(|_| format!("--delay-ms: «{v}» non è un numero"))?;
            }
            "--no-verbose" => o.verbose = false,
            "--no-rev-m" => o.rev_m = false,
            "--macro-partial" => o.macro_partial = true,
            altro => return Err(format!("opzione sconosciuta: «{altro}»")),
        }
    }
    Ok(Some(o))
}

#[tokio::main]
async fn main() -> ExitCode {
    let o = match parse_args(std::env::args().skip(1)) {
        Ok(Some(o)) => o,
        Ok(None) => {
            print!("{USO}");
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("tlx-sim: {e}\n\n{USO}");
            return ExitCode::FAILURE;
        }
    };

    let cfg = SimConfig {
        ports: o.ports,
        bind: o.listen,
        verbose: o.verbose,
        bcast_target: o.bcast,
        bcast_ms: o.bcast_ms,
        rev_m: o.rev_m,
        cmd_delay_ms: o.delay_ms,
        macro_partial_apply: o.macro_partial,
    };

    let handle = match Simulator::start(cfg).await {
        Ok(h) => h,
        Err(e) => {
            eprintln!("tlx-sim: impossibile avviare l'ascolto su {}: {e}", o.listen);
            return ExitCode::FAILURE;
        }
    };

    println!(
        "tlx-sim {SIM_VERSION}: {} porte, {} schede, XOPTIONS {:04o}",
        o.ports,
        cards_for(o.ports),
        options_for(o.ports)
    );
    println!("API ASCII in ascolto su {}", handle.local_addr);
    match o.bcast {
        Some(t) => println!("stato UDP verso {t} ogni {} ms", o.bcast_ms),
        None => println!("broadcast di stato UDP disattivato (usa --bcast IP:PORTA)"),
    }
    if !o.verbose {
        println!("risposte non verbose");
    }
    if !o.rev_m {
        println!("firmware pre Rev. M: XLOS/XRXPWR/XTXPWR rispondono ER0001");
    }
    println!("Ctrl-C per fermare.");

    if let Err(e) = tokio::signal::ctrl_c().await {
        eprintln!("tlx-sim: attesa di Ctrl-C non disponibile ({e}); il simulatore resta attivo");
        std::future::pending::<()>().await;
    }
    println!("\ntlx-sim: arresto.");
    handle.shutdown();
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Option<Opzioni>, String> {
        parse_args(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn default_e_help() {
        let o = p(&[]).unwrap().unwrap();
        assert_eq!(o.ports, 24);
        assert_eq!(o.listen.port(), 17567);
        assert!(o.bcast.is_none());
        assert!(o.verbose && o.rev_m);
        assert!(p(&["--help"]).unwrap().is_none());
    }

    #[test]
    fn opzioni_complete() {
        let o = p(&[
            "--ports",
            "48",
            "--listen",
            "127.0.0.1:0",
            "--bcast",
            "239.255.13.9:17564",
            "--bcast-ms",
            "1000",
            "--delay-ms",
            "5",
            "--no-verbose",
            "--no-rev-m",
        ])
        .unwrap()
        .unwrap();
        assert_eq!(o.ports, 48);
        assert_eq!(o.listen.to_string(), "127.0.0.1:0");
        assert_eq!(o.bcast.unwrap().to_string(), "239.255.13.9:17564");
        assert_eq!(o.bcast_ms, 1000);
        assert_eq!(o.delay_ms, 5);
        assert!(!o.verbose && !o.rev_m);
    }

    #[test]
    fn errori() {
        assert!(p(&["--ports"]).is_err());
        assert!(p(&["--ports", "0"]).is_err());
        assert!(p(&["--ports", "x"]).is_err());
        assert!(p(&["--bcast-ms", "0"]).is_err());
        assert!(p(&["--listen", "non:valido:qui"]).is_err());
        assert!(p(&["--boh"]).is_err());
    }
}
