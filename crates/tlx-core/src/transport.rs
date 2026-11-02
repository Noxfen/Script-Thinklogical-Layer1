//! Trasporto a righe verso la matrice: TCP 17567 oppure RS-232 (spec §2).
//!
//! Il trasporto non interpreta le risposte: invia una riga terminata da `LF` e accumula i
//! byte ricevuti fino al terminatore (una lettura dal socket non è una risposta), togliendo
//! i `CR` in coda. Sulla seriale con XON/XOFF scarta i byte 0x11/0x13.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::Instant;
use tokio_serial::{SerialPort, SerialPortBuilderExt, SerialStream};

/// Porta TCP standard dell'API ASCII.
pub const DEFAULT_TCP_PORT: u16 = 17567;
/// Lunghezza massima di una riga di risposta (oltre: errore, sessione compromessa).
pub const MAX_LINE_LEN: usize = 256 * 1024;

const XON: u8 = 0x11;
const XOFF: u8 = 0x13;

/// Controllo di flusso della seriale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FlowControl {
    /// XON/XOFF software (default di fabbrica secondo CFG p.7, spec §2.2).
    #[default]
    XonXoff,
    /// Nessuno.
    None,
    /// RTS/CTS hardware.
    Hardware,
}

/// Configurazione del canale di comando.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TransportConfig {
    /// TCP verso l'indirizzo virtuale X (o un tunnel locale).
    #[serde(rename_all = "camelCase")]
    Tcp {
        /// Host o IP.
        host: String,
        /// Porta (17567).
        port: u16,
        /// Timeout di apertura in ms (0 = usa quello del client).
        connect_timeout_ms: u64,
    },
    /// RS-232 della Controller Card attiva (9600 8N1).
    #[serde(rename_all = "camelCase")]
    Serial {
        /// Nome della porta (`COM3`, `/dev/ttyUSB0`, `/dev/tty.usbserial-1410`).
        port: String,
        /// Velocità (9600).
        baud: u32,
        /// Controllo di flusso.
        flow: FlowControl,
    },
}

impl Default for TransportConfig {
    fn default() -> Self {
        TransportConfig::Tcp {
            host: "127.0.0.1".into(),
            port: DEFAULT_TCP_PORT,
            connect_timeout_ms: 3000,
        }
    }
}

impl TransportConfig {
    /// Descrizione leggibile dell'estremo (`192.168.13.15:17567`, `COM3 @ 9600`).
    pub fn endpoint(&self) -> String {
        match self {
            TransportConfig::Tcp { host, port, .. } => format!("{host}:{port}"),
            TransportConfig::Serial { port, baud, .. } => format!("{port} @ {baud}"),
        }
    }

    /// `true` per la seriale.
    pub fn is_serial(&self) -> bool {
        matches!(self, TransportConfig::Serial { .. })
    }
}

/// Errore di trasporto.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransportError {
    /// Apertura fallita.
    #[error("apertura del canale fallita: {0}")]
    Open(String),
    /// Errore di I/O.
    #[error("errore di comunicazione: {0}")]
    Io(String),
    /// Nessuna riga completa entro il timeout.
    #[error("timeout: nessuna risposta completa")]
    Timeout,
    /// Il canale è stato chiuso dall'altro capo.
    #[error("connessione chiusa dalla matrice")]
    Closed,
    /// Riga oltre [`MAX_LINE_LEN`].
    #[error("riga di risposta troppo lunga")]
    LineTooLong,
}

enum Inner {
    Tcp(TcpStream),
    Serial(Box<SerialStream>),
}

/// Canale a righe aperto (TCP o seriale).
pub struct LineTransport {
    inner: Inner,
    buf: Vec<u8>,
    filter_xonxoff: bool,
    endpoint: String,
}

impl std::fmt::Debug for LineTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LineTransport").field("endpoint", &self.endpoint).finish()
    }
}

impl LineTransport {
    /// Apre il canale. `default_connect_timeout` vale per il TCP quando
    /// `connect_timeout_ms` è 0. Sulla seriale svuota il buffer d'ingresso.
    pub async fn open(
        cfg: &TransportConfig,
        default_connect_timeout: Duration,
    ) -> Result<LineTransport, TransportError> {
        match cfg {
            TransportConfig::Tcp { host, port, connect_timeout_ms } => {
                let to = if *connect_timeout_ms > 0 {
                    Duration::from_millis(*connect_timeout_ms)
                } else {
                    default_connect_timeout
                };
                let addr = format!("{host}:{port}");
                let stream = tokio::time::timeout(to, TcpStream::connect(&addr))
                    .await
                    .map_err(|_| {
                        TransportError::Open(format!(
                            "{addr} non risponde entro {} ms",
                            to.as_millis()
                        ))
                    })?
                    .map_err(|e| TransportError::Open(format!("{addr}: {e}")))?;
                let _ = stream.set_nodelay(true);
                // Keepalive di kernel: rete di sicurezza oltre all'heartbeat applicativo.
                let ka = socket2::TcpKeepalive::new().with_time(Duration::from_secs(30));
                let _ = socket2::SockRef::from(&stream).set_tcp_keepalive(&ka);
                Ok(LineTransport {
                    inner: Inner::Tcp(stream),
                    buf: Vec::with_capacity(4096),
                    filter_xonxoff: false,
                    endpoint: addr,
                })
            }
            TransportConfig::Serial { port, baud, flow } => {
                let fc = match flow {
                    FlowControl::XonXoff => tokio_serial::FlowControl::Software,
                    FlowControl::None => tokio_serial::FlowControl::None,
                    FlowControl::Hardware => tokio_serial::FlowControl::Hardware,
                };
                let stream = tokio_serial::new(port.as_str(), *baud)
                    .data_bits(tokio_serial::DataBits::Eight)
                    .parity(tokio_serial::Parity::None)
                    .stop_bits(tokio_serial::StopBits::One)
                    .flow_control(fc)
                    .open_native_async()
                    .map_err(|e| TransportError::Open(format!("{port}: {e}")))?;
                let _ = stream.clear(tokio_serial::ClearBuffer::Input);
                Ok(LineTransport {
                    inner: Inner::Serial(Box::new(stream)),
                    buf: Vec::with_capacity(4096),
                    filter_xonxoff: *flow == FlowControl::XonXoff,
                    endpoint: format!("{port} @ {baud}"),
                })
            }
        }
    }

    /// Estremo del canale.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// `true` se è una seriale.
    pub fn is_serial(&self) -> bool {
        matches!(self.inner, Inner::Serial(_))
    }

    /// Invia una riga aggiungendo `LF`.
    pub async fn send_line(&mut self, line: &str) -> Result<(), TransportError> {
        let mut bytes = Vec::with_capacity(line.len() + 1);
        bytes.extend_from_slice(line.as_bytes());
        bytes.push(b'\n');
        let r = match &mut self.inner {
            Inner::Tcp(s) => match s.write_all(&bytes).await {
                Ok(()) => s.flush().await,
                Err(e) => Err(e),
            },
            Inner::Serial(s) => match s.write_all(&bytes).await {
                Ok(()) => s.flush().await,
                Err(e) => Err(e),
            },
        };
        r.map_err(|e| TransportError::Io(e.to_string()))
    }

    /// Scarta i byte già ricevuti e non letti (buffer interno e, sulla seriale, del driver).
    pub fn clear_input(&mut self) {
        self.buf.clear();
        if let Inner::Serial(s) = &self.inner {
            let _ = s.clear(tokio_serial::ClearBuffer::Input);
        }
    }

    fn take_line(&mut self) -> Option<String> {
        let pos = self.buf.iter().position(|&b| b == b'\n')?;
        let mut line: Vec<u8> = self.buf.drain(..=pos).collect();
        line.pop(); // LF
        while line.last() == Some(&b'\r') {
            line.pop();
        }
        Some(String::from_utf8_lossy(&line).into_owned())
    }

    /// Legge una riga completa entro `timeout`, accumulando più letture.
    /// Restituisce la riga senza `LF` né `CR` in coda.
    pub async fn read_line(&mut self, timeout: Duration) -> Result<String, TransportError> {
        let deadline = Instant::now() + timeout;
        let mut tmp = [0u8; 4096];
        loop {
            if let Some(line) = self.take_line() {
                return Ok(line);
            }
            if self.buf.len() > MAX_LINE_LEN {
                return Err(TransportError::LineTooLong);
            }
            let fut = async {
                match &mut self.inner {
                    Inner::Tcp(s) => s.read(&mut tmp).await,
                    Inner::Serial(s) => s.read(&mut tmp).await,
                }
            };
            let n = match tokio::time::timeout_at(deadline, fut).await {
                Err(_) => return Err(TransportError::Timeout),
                Ok(Err(e)) => return Err(TransportError::Io(e.to_string())),
                Ok(Ok(0)) => return Err(TransportError::Closed),
                Ok(Ok(n)) => n,
            };
            if self.filter_xonxoff {
                self.buf.extend(tmp[..n].iter().copied().filter(|&b| b != XON && b != XOFF));
            } else {
                self.buf.extend_from_slice(&tmp[..n]);
            }
        }
    }

    /// Chiude il canale (TCP: shutdown della scrittura).
    pub async fn shutdown(&mut self) {
        if let Inner::Tcp(s) = &mut self.inner {
            let _ = tokio::time::timeout(Duration::from_millis(300), s.shutdown()).await;
        }
    }
}

/// Elenco delle porte seriali del sistema (ordinato; vuoto in caso di errore).
pub fn list_serial_ports() -> Vec<String> {
    let mut v: Vec<String> = tokio_serial::available_ports()
        .map(|ports| ports.into_iter().map(|p| p.port_name).collect())
        .unwrap_or_default();
    v.sort();
    v.dedup();
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[test]
    fn serde_config() {
        let c = TransportConfig::Tcp {
            host: "192.168.13.15".into(),
            port: 17567,
            connect_timeout_ms: 3000,
        };
        let j = serde_json::to_string(&c).unwrap();
        assert_eq!(
            j,
            r#"{"kind":"tcp","host":"192.168.13.15","port":17567,"connectTimeoutMs":3000}"#
        );
        let s: TransportConfig =
            serde_json::from_str(r#"{"kind":"serial","port":"COM3","baud":9600,"flow":"xonxoff"}"#)
                .unwrap();
        assert_eq!(
            s,
            TransportConfig::Serial { port: "COM3".into(), baud: 9600, flow: FlowControl::XonXoff }
        );
        assert_eq!(s.endpoint(), "COM3 @ 9600");
    }

    #[tokio::test]
    async fn accumulo_righe_frammentate_e_crlf() {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        let srv = tokio::spawn(async move {
            let (mut s, _) = l.accept().await.unwrap();
            let mut b = [0u8; 64];
            let n = s.read(&mut b).await.unwrap();
            assert_eq!(&b[..n], b"XGET\n");
            s.write_all(b"R0000OKI0001").await.unwrap();
            s.flush().await.unwrap();
            tokio::time::sleep(Duration::from_millis(50)).await;
            s.write_all(b"0002\r\nR0003OK\nR0007ER").await.unwrap();
            s.flush().await.unwrap();
            tokio::time::sleep(Duration::from_millis(50)).await;
            s.write_all(b"0001\r\n").await.unwrap();
            tokio::time::sleep(Duration::from_millis(100)).await;
        });
        let cfg = TransportConfig::Tcp {
            host: "127.0.0.1".into(),
            port: addr.port(),
            connect_timeout_ms: 1000,
        };
        let mut t = LineTransport::open(&cfg, Duration::from_secs(1)).await.unwrap();
        t.send_line("XGET").await.unwrap();
        assert_eq!(t.read_line(Duration::from_secs(1)).await.unwrap(), "R0000OKI00010002");
        assert_eq!(t.read_line(Duration::from_secs(1)).await.unwrap(), "R0003OK");
        assert_eq!(t.read_line(Duration::from_secs(1)).await.unwrap(), "R0007ER0001");
        assert_eq!(t.read_line(Duration::from_millis(50)).await, Err(TransportError::Timeout));
        srv.await.unwrap();
        assert_eq!(t.read_line(Duration::from_millis(500)).await, Err(TransportError::Closed));
    }

    #[tokio::test]
    async fn apertura_fallita() {
        let cfg =
            TransportConfig::Tcp { host: "127.0.0.1".into(), port: 1, connect_timeout_ms: 500 };
        assert!(matches!(
            LineTransport::open(&cfg, Duration::from_secs(1)).await,
            Err(TransportError::Open(_))
        ));
    }
}
