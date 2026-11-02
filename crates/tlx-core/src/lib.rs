//! # tlx-core
//!
//! Libreria per il comando delle matrici in fibra **Thinklogical TLX** tramite l'API ASCII
//! (TCP 17567 o RS-232) e la ricezione dello stato via UDP 17564.
//!
//! Moduli:
//! * [`protocol`] — parser e generatori puri (risposte, comandi, routing, info, UDP 17564);
//! * [`transport`] — canale a righe su TCP o seriale;
//! * [`session`] — una sessione di comando con handshake e timeout;
//! * [`client`] — handle ad alto livello, thread-safe, con riconnessione ed eventi;
//! * [`sim`] — simulatore TLX in-process (modalità demo e test).
//!
//! Riferimento: «API TL — sviluppo modulo TT-AVC» v2.0 (§2–§6, App. B).

#![warn(missing_docs)]

pub mod client;
pub mod events;
pub mod protocol;
pub mod session;
pub mod sim;
pub mod transport;
pub(crate) mod util;

pub use client::{ClientConfig, ClientError, Diagnostics, MatrixClient, UdpConfig};
pub use protocol::{MatrixInfo, Response, Routing};
pub use session::CmdOutcome;
