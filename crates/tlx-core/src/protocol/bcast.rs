//! Datagramma di stato UDP 17564 (spec §6.1). Formato big-endian:
//!
//! | offset | dim. | campo |
//! |---|---|---|
//! | 0 | 4 | IPv4 del controller che emette |
//! | 4 | 2 | chassis (0 = superiore/unico, 1 = inferiore) |
//! | 6 | 2 | prima porta del blocco |
//! | 8 | 2 | ultima porta del blocco |
//! | 10 | 2·n | per ogni uscita: ingresso collegato (0 = nessuno) |
//!
//! Il TLX1280 invia due blocchi (1–640, 641–1280): [`BcastAssembler`] li ricompone.

use std::collections::BTreeMap;
use std::net::Ipv4Addr;

use super::routing::Routing;

/// Porta UDP standard del broadcast di stato.
pub const BCAST_PORT: u16 = 17564;
/// Gruppo multicast di default di `tlxapi --mcast`.
pub const DEFAULT_MCAST_GROUP: Ipv4Addr = Ipv4Addr::new(239, 255, 13, 9);

/// Errore di decodifica del datagramma.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BcastError {
    /// Meno di 10 byte.
    #[error("datagramma troppo corto ({0} byte)")]
    TooShort(usize),
    /// Prima/ultima porta incoerenti.
    #[error("intervallo di porte non valido ({first}–{last})")]
    BadRange {
        /// Prima porta.
        first: u16,
        /// Ultima porta.
        last: u16,
    },
    /// Lunghezza diversa da `10 + 2·(ultima − prima + 1)`.
    #[error("lunghezza {got} byte, attesi {expected}")]
    BadLength {
        /// Ricevuti.
        got: usize,
        /// Attesi.
        expected: usize,
    },
    /// Un ingresso non è un numero di porta possibile (`9999` = «tutte le porte», o oltre).
    #[error("uscita {output}: ingresso {input} non è un numero di porta valido")]
    BadInput {
        /// Uscita a cui si riferisce il valore.
        output: u16,
        /// Valore letto.
        input: u16,
    },
}

/// Un blocco decodificato.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BcastBlock {
    /// IP del controller dichiarato nel datagramma.
    pub sender: Ipv4Addr,
    /// Flag di chassis.
    pub chassis: u16,
    /// Prima porta del blocco.
    pub first: u16,
    /// Ultima porta del blocco.
    pub last: u16,
    /// Ingresso per ogni uscita `first..=last`.
    pub inputs: Vec<u16>,
}

/// Decodifica un datagramma 17564, validando la lunghezza prima di leggere.
pub fn parse_17564(buf: &[u8]) -> Result<BcastBlock, BcastError> {
    if buf.len() < 10 {
        return Err(BcastError::TooShort(buf.len()));
    }
    let be = |i: usize| u16::from_be_bytes([buf[i], buf[i + 1]]);
    let sender = Ipv4Addr::new(buf[0], buf[1], buf[2], buf[3]);
    let chassis = be(4);
    let first = be(6);
    let last = be(8);
    if first == 0 || last < first || last >= 9999 {
        return Err(BcastError::BadRange { first, last });
    }
    let n = (last - first + 1) as usize;
    let expected = 10 + 2 * n;
    if buf.len() != expected {
        return Err(BcastError::BadLength { got: buf.len(), expected });
    }
    // Oltre alla lunghezza (unica validazione chiesta dalla §6.1) si controllano i valori:
    // un datagramma corrotto darebbe altrimenti una fotografia con «uscita 3 ← ingresso
    // 9999». Si **scarta il datagramma intero**, come già per lunghezza e intervallo: un
    // singolo valore azzerato sarebbe indistinguibile da «uscita scollegata», cioè un altro
    // dato falso. `9999` è il sentinella «tutte le porte» (§3.1) e nessun modello ha tante
    // porte. Non si può invece limitare a `last`: nel primo blocco di un TLX1280 gli
    // ingressi sono numeri **globali** fino a 1280 (§6.1, V23); il confronto con il vero N
    // lo fa [`BcastAssembler::push`], che N lo conosce.
    let mut inputs = Vec::with_capacity(n);
    for k in 0..n {
        let input = be(10 + 2 * k);
        if input >= 9999 {
            return Err(BcastError::BadInput { output: first + k as u16, input });
        }
        inputs.push(input);
    }
    Ok(BcastBlock { sender, chassis, first, last, inputs })
}

/// Codifica un blocco (usato dal simulatore).
pub fn encode_17564(b: &BcastBlock) -> Vec<u8> {
    let mut v = Vec::with_capacity(10 + 2 * b.inputs.len());
    v.extend_from_slice(&b.sender.octets());
    v.extend_from_slice(&b.chassis.to_be_bytes());
    v.extend_from_slice(&b.first.to_be_bytes());
    v.extend_from_slice(&b.last.to_be_bytes());
    for i in &b.inputs {
        v.extend_from_slice(&i.to_be_bytes());
    }
    v
}

/// Divide una routing nei blocchi che la matrice invierebbe (2 blocchi oltre le 640 porte).
pub fn blocks_for(routing: &Routing, sender: Ipv4Addr) -> Vec<BcastBlock> {
    let n = routing.max_ports;
    let ranges: Vec<(u16, u16, u16)> =
        if n > 640 { vec![(1, 640, 0), (641, n, 1)] } else { vec![(1, n, 0)] };
    ranges
        .into_iter()
        .map(|(first, last, chassis)| BcastBlock {
            sender,
            chassis,
            first,
            last,
            inputs: routing.outputs[(first - 1) as usize..last as usize].to_vec(),
        })
        .collect()
}

/// Ricompone i blocchi ricevuti in una [`Routing`] completa.
///
/// Un blocco che parte dalla porta 1 e arriva a N è già completo. Con più blocchi
/// (TLX1280) la fotografia è completa solo se tutti i blocchi che coprono `1..=N`
/// sono arrivati entro un periodo (`window_ms`).
///
/// La §5.4 indicizza ogni datagramma su **(chassis, prima porta)** e chiede il filtro sul
/// mittente: la §1.2 prevede più matrici sulla stessa subnet, tutte in broadcast su
/// `192.168.13.255:17564` e tutte con `first = 1`. L'assemblatore vale quindi per **una**
/// matrice: i blocchi di un altro mittente si scartano, non si mescolano con la
/// fotografia in corso. Senza questo filtro due TLX si sovrascriverebbero a vicenda e
/// l'operatore vedrebbe gli incroci di un'altra matrice attribuiti a questa.
#[derive(Debug, Clone)]
pub struct BcastAssembler {
    window_ms: u64,
    expected_ports: Option<u16>,
    /// Blocchi in finestra. Chiave `(prima porta, chassis)`: è l'indice `(chassis, prima
    /// porta)` della §5.4 con i due campi scambiati, perché la `BTreeMap` deve scorrere in
    /// ordine di **prima porta** per ricomporre `1..=N`.
    blocks: BTreeMap<(u16, u16), (BcastBlock, u64)>,
    /// Mittente della fotografia in corso; `None` quando la finestra è vuota.
    sender: Option<Ipv4Addr>,
}

impl BcastAssembler {
    /// `window_ms` = periodo di broadcast (default 4000 ms).
    pub fn new(window_ms: u64) -> Self {
        BcastAssembler { window_ms, expected_ports: None, blocks: BTreeMap::new(), sender: None }
    }

    /// Imposta N se noto (dall'handshake): finché è `None` nessuna fotografia viene
    /// emessa, perché il datagramma non dichiara N (§6.1) e il primo blocco di un
    /// TLX1280 è indistinguibile da una matrice da 640 porte.
    pub fn set_expected_ports(&mut self, n: Option<u16>) {
        self.expected_ports = n;
    }

    /// Aggiunge un blocco ricevuto all'istante `now_ms`; restituisce la routing completa
    /// se i blocchi in finestra coprono tutte le porte.
    pub fn push(&mut self, block: BcastBlock, now_ms: u64) -> Option<Routing> {
        // Prima si fanno scadere i blocchi fuori finestra: se la finestra resta vuota il
        // mittente si libera e la prima matrice che trasmette apre la nuova fotografia
        // (altrimenti un datagramma sbagliato bloccherebbe per sempre l'assemblatore).
        let window = self.window_ms;
        self.blocks.retain(|_, (_, t)| now_ms.saturating_sub(*t) <= window);
        if self.blocks.is_empty() {
            self.sender = None;
        }
        match self.sender {
            // Altra matrice: si scarta (§5.4, §1.2). Il chassis invece fa parte della
            // chiave e **non** è motivo di scarto: il secondo blocco del TLX1280 arriva
            // con chassis = 1 e la §6.1 dice esplicitamente di non scartarlo.
            Some(s) if s != block.sender => return None,
            Some(_) => {}
            None => self.sender = Some(block.sender),
        }
        // N è noto solo dall'handshake; il datagramma non dichiara il modello (§6.1).
        // Senza N non si emette nulla: dedurlo da `last` farebbe passare il solo blocco
        // 1–640 di un TLX1280 per una fotografia completa, con le porte 641–1280
        // dichiarate scollegate mentre sono collegate. Un dato assente è meno pericoloso
        // di un dato falso.
        let n = self.expected_ports?;
        if n == 0 {
            return None;
        }
        // Un ingresso oltre N non può venire da questa matrice (datagramma corrotto o di
        // un modello diverso): si scarta il blocco senza conservarlo.
        if block.inputs.iter().any(|&i| i > n) {
            return None;
        }
        self.blocks.insert((block.first, block.chassis), (block, now_ms));
        let mut r = Routing::new(n);
        let mut next = 1u16;
        for ((first, _), (b, _)) in &self.blocks {
            if *first != next {
                if *first > next {
                    return None;
                }
                continue; // blocco sovrapposto: ignorato
            }
            if b.last > n {
                return None;
            }
            for (k, &i) in b.inputs.iter().enumerate() {
                r.outputs[(b.first - 1) as usize + k] = i;
            }
            next = b.last + 1;
            if next > n {
                break;
            }
        }
        (next > n).then_some(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_ifc_p14_tlx48() {
        let mut buf = vec![
            0xc0, 0xa8, 0x0d, 0x19, 0x00, 0x00, 0x00, 0x01, 0x00, 0x30, 0x00, 0x11, 0x00, 0x12,
        ];
        buf.resize(10 + 2 * 48, 0);
        let b = parse_17564(&buf).unwrap();
        assert_eq!(b.sender, Ipv4Addr::new(192, 168, 13, 25));
        assert_eq!(b.chassis, 0);
        assert_eq!((b.first, b.last), (1, 48));
        assert_eq!(b.inputs[0], 17);
        assert_eq!(b.inputs[1], 18);
        assert_eq!(encode_17564(&b), buf);
    }

    #[test]
    fn errori() {
        assert_eq!(parse_17564(&[0; 5]), Err(BcastError::TooShort(5)));
        let mut buf = vec![192, 168, 13, 25, 0, 0, 0, 1, 0, 48];
        buf.resize(10 + 2 * 47, 0);
        assert!(matches!(parse_17564(&buf), Err(BcastError::BadLength { .. })));
        let buf = [192, 168, 13, 25, 0, 0, 0, 5, 0, 4, 0, 0];
        assert!(matches!(parse_17564(&buf), Err(BcastError::BadRange { .. })));
    }

    #[test]
    fn round_trip_e_assembler_singolo() {
        let mut r = Routing::new(24);
        r.apply_ci(10, &[10, 11, 12]);
        let blocks = blocks_for(&r, Ipv4Addr::LOCALHOST);
        assert_eq!(blocks.len(), 1);
        let bytes = encode_17564(&blocks[0]);
        assert_eq!(bytes.len(), 10 + 48);
        let back = parse_17564(&bytes).unwrap();
        let mut a = BcastAssembler::new(4000);
        // N arriva dall'handshake: senza di esso l'assemblatore non emette nulla.
        a.set_expected_ports(Some(24));
        assert_eq!(a.push(back, 0), Some(r));
    }

    #[test]
    fn tlx1280_due_blocchi() {
        let mut r = Routing::new(1280);
        r.apply_ci(700, &[1, 1000]);
        r.apply_ci(5, &[641]);
        let blocks = blocks_for(&r, Ipv4Addr::new(192, 168, 13, 15));
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[1].chassis, 1);
        let mut a = BcastAssembler::new(4000);
        a.set_expected_ports(Some(1280));
        assert_eq!(a.push(parse_17564(&encode_17564(&blocks[0])).unwrap(), 1000), None);
        let full = a.push(parse_17564(&encode_17564(&blocks[1])).unwrap(), 1500).unwrap();
        assert_eq!(full, r);

        // blocco mancante: il secondo arriva fuori finestra → nessuna fotografia
        let mut a = BcastAssembler::new(4000);
        a.set_expected_ports(Some(1280));
        assert_eq!(a.push(blocks[0].clone(), 0), None);
        assert_eq!(a.push(blocks[1].clone(), 9000), None);
        // ... finché non arriva di nuovo il primo entro la finestra
        assert_eq!(a.push(blocks[0].clone(), 10000), Some(r));
    }

    /// Rilievo 2: due matrici sulla stessa subnet trasmettono entrambe con `first = 1`
    /// (§1.2, §5.4). I blocchi di un altro mittente non devono comporre la fotografia.
    #[test]
    fn mittenti_diversi_non_si_mescolano() {
        let mut r1 = Routing::new(1280);
        r1.apply_ci(700, &[1]);
        let mut r2 = Routing::new(1280);
        r2.apply_ci(3, &[4]);
        let b1 = blocks_for(&r1, Ipv4Addr::new(192, 168, 13, 15));
        let b2 = blocks_for(&r2, Ipv4Addr::new(192, 168, 13, 17));

        let mut a = BcastAssembler::new(4000);
        a.set_expected_ports(Some(1280));
        // blocco 1–640 della matrice .15, poi blocco 641–1280 della matrice .17
        assert_eq!(a.push(b1[0].clone(), 0), None);
        assert_eq!(a.push(b2[1].clone(), 100), None, "non deve comporre matrici diverse");
        // la fotografia si chiude solo con il secondo blocco della matrice giusta
        assert_eq!(a.push(b1[1].clone(), 200), Some(r1.clone()));

        // Una sola matrice: comportamento invariato anche se il mittente cambia dopo che
        // la finestra è scaduta (failover: il campo IP può cambiare, §5.4 V5).
        let mut a = BcastAssembler::new(4000);
        a.set_expected_ports(Some(1280));
        assert_eq!(a.push(b1[0].clone(), 0), None);
        assert_eq!(a.push(b1[1].clone(), 100), Some(r1));
        assert_eq!(a.push(b2[0].clone(), 20000), None);
        assert_eq!(a.push(b2[1].clone(), 20100), Some(r2));
    }

    /// Rilievo 3: senza N dall'handshake il solo blocco 1–640 di un TLX1280 non deve
    /// diventare una fotografia «completa» da 640 porte.
    #[test]
    fn senza_expected_ports_nessuna_fotografia() {
        let mut r = Routing::new(1280);
        r.apply_ci(700, &[1000]);
        let blocks = blocks_for(&r, Ipv4Addr::new(192, 168, 13, 15));
        let mut a = BcastAssembler::new(4000);
        assert_eq!(a.push(blocks[0].clone(), 0), None);
        assert_eq!(a.push(blocks[1].clone(), 100), None);
        // anche una matrice piccola resta muta finché N non è noto
        let mut piccola = Routing::new(24);
        piccola.apply_ci(1, &[2]);
        let b = blocks_for(&piccola, Ipv4Addr::LOCALHOST);
        let mut a = BcastAssembler::new(4000);
        assert_eq!(a.push(b[0].clone(), 0), None);
        // ... e appena N arriva la fotografia si chiude
        a.set_expected_ports(Some(24));
        assert_eq!(a.push(b[0].clone(), 100), Some(piccola));
    }

    /// Rilievo 4: ingressi impossibili nel datagramma. `9999` e oltre fanno scartare il
    /// datagramma in `parse_17564`; un ingresso oltre N fa scartare il blocco nell'assemblatore.
    #[test]
    fn ingressi_fuori_intervallo_scartati() {
        // 24 porte, uscita 1 ← 9999
        let mut buf = vec![192, 168, 13, 15, 0, 0, 0, 1, 0, 24, 0x27, 0x0f];
        buf.resize(10 + 2 * 24, 0);
        assert_eq!(parse_17564(&buf), Err(BcastError::BadInput { output: 1, input: 9999 }));
        // uscita 3 ← 9999 (offset 10 + 2·2)
        let mut buf = vec![192, 168, 13, 15, 0, 0, 0, 1, 0, 24];
        buf.resize(10 + 2 * 24, 0);
        buf[14] = 0x27;
        buf[15] = 0x0f;
        assert!(matches!(parse_17564(&buf), Err(BcastError::BadInput { output: 3, .. })));
        // valore valido ma oltre N: il blocco si scarta, nessuna fotografia falsa
        let mut r = Routing::new(24);
        r.apply_ci(10, &[5]);
        let mut block = blocks_for(&r, Ipv4Addr::LOCALHOST)[0].clone();
        block.inputs[4] = 300;
        let mut a = BcastAssembler::new(4000);
        a.set_expected_ports(Some(24));
        assert_eq!(a.push(block, 0), None);
    }
}
