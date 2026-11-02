//! Configurazione persistente dell'applicazione (profili, etichette, preset, impostazioni).
//! Formato JSON camelCase: vedi docs/IPC.md.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TransportKind {
    Tcp,
    Serial,
    Demo,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FlowControlCfg {
    Xonxoff,
    None,
    Hardware,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MatrixProfile {
    pub id: String,
    pub name: String,
    pub transport: TransportKind,
    pub host: String,
    pub port: u16,
    pub serial_port: String,
    pub baud: u32,
    pub flow_control: FlowControlCfg,
    pub demo_ports: u16,
    pub udp_listen: bool,
    pub udp_mcast_group: String,
}

impl Default for MatrixProfile {
    fn default() -> Self {
        Self {
            id: new_id(),
            name: "Nuova matrice".into(),
            transport: TransportKind::Tcp,
            host: "192.168.13.15".into(),
            port: 17567,
            serial_port: String::new(),
            baud: 9600,
            flow_control: FlowControlCfg::Xonxoff,
            demo_ports: 24,
            // Off di default: su Windows l'ascolto UDP fa comparire l'avviso del firewall,
            // che confonde l'operatore. Lo stato si aggiorna comunque (XLASTEVENT + XGET).
            udp_listen: false,
            udp_mcast_group: String::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PortRole {
    Tx,
    Rx,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortLabel {
    pub port: u16,
    #[serde(default)]
    pub name: String,
    #[serde(default = "role_other")]
    pub role: PortRole,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub notes: String,
}
fn role_other() -> PortRole {
    PortRole::Other
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LinkMode {
    /// Logica dello script Python, esclusiva: DI a, DI b, CI a O b, CI b O a.
    Bidir,
    /// a = TX, b = RX: DI b, CI a O b, CI b O a (non esclusiva sul TX).
    Kvm,
    /// Solo video: CI a O b.
    Oneway,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetLink {
    pub a: u16,
    pub b: u16,
    pub mode: LinkMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preset {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub links: Vec<PresetLink>,
    #[serde(default)]
    pub color: String,
    /// Scena: ripristina l'intera matrice come salvata (uscite non elencate spente).
    #[serde(default)]
    pub scene: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BidirStrategy {
    Sequence,
    Macro,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub timeout_connect_ms: u64,
    pub timeout_cmd_ms: u64,
    pub heartbeat_s: u64,
    pub poll_xget_s: u64,
    pub verify_after_switch: bool,
    pub delay_ms: u64,
    pub bidir_strategy: BidirStrategy,
    pub confirm_dangerous: bool,
    pub expert_mode: bool,
    pub theme: Theme,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            timeout_connect_ms: 3000,
            timeout_cmd_ms: 2000,
            heartbeat_s: 10,
            poll_xget_s: 60,
            verify_after_switch: true,
            delay_ms: 300,
            bidir_strategy: BidirStrategy::Sequence,
            confirm_dangerous: true,
            expert_mode: false,
            theme: Theme::System,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppConfig {
    pub version: u32,
    pub active_profile_id: Option<String>,
    pub profiles: Vec<MatrixProfile>,
    pub labels: HashMap<String, Vec<PortLabel>>,
    pub presets: HashMap<String, Vec<Preset>>,
    pub settings: Settings,
}

impl Default for AppConfig {
    /// Primo avvio: il profilo della matrice dello script Python (TLX24 su 192.168.13.15)
    /// con i suoi quattro preset di fabbrica, più un profilo Demo col simulatore integrato.
    fn default() -> Self {
        let tlx = MatrixProfile { name: "TLX24".into(), ..Default::default() };
        let demo = MatrixProfile {
            name: "Demo (simulatore)".into(),
            transport: TransportKind::Demo,
            host: "127.0.0.1".into(),
            udp_listen: false,
            ..Default::default()
        };
        let legacy = [
            ("Codec 1 ↔ Tablet A", 1, 5),
            ("Codec 1 ↔ Tablet B", 1, 8),
            ("Codec 2 ↔ Tablet A", 4, 5),
            ("Codec 2 ↔ Tablet B", 4, 8),
        ];
        let mk = || -> Vec<Preset> {
            legacy
                .iter()
                .map(|(n, a, b)| Preset {
                    id: new_id(),
                    name: (*n).into(),
                    links: vec![PresetLink { a: *a, b: *b, mode: LinkMode::Bidir }],
                    color: String::new(),
                    scene: false,
                })
                .collect()
        };
        let mut presets = HashMap::new();
        presets.insert(tlx.id.clone(), mk());
        presets.insert(demo.id.clone(), mk());
        Self {
            version: 1,
            active_profile_id: Some(tlx.id.clone()),
            profiles: vec![tlx, demo],
            labels: HashMap::new(),
            presets,
            settings: Settings::default(),
        }
    }
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

impl AppConfig {
    pub fn profile(&self, id: &str) -> Option<&MatrixProfile> {
        self.profiles.iter().find(|p| p.id == id)
    }

    pub fn locked_ports(&self, profile_id: &str) -> Vec<u16> {
        self.labels
            .get(profile_id)
            .map(|v| v.iter().filter(|l| l.locked).map(|l| l.port).collect())
            .unwrap_or_default()
    }

    /// Ruoli delle porte: dalle etichette (se diverse da «altro») e, in mancanza, dai link
    /// «Postazione KVM» dei preset (a = TX, b = RX).
    pub fn roles(&self, profile_id: &str) -> HashMap<u16, PortRole> {
        let mut r: HashMap<u16, PortRole> = HashMap::new();
        for p in self.presets.get(profile_id).into_iter().flatten() {
            for l in p.links.iter().filter(|l| l.mode == LinkMode::Kvm) {
                r.insert(l.a, PortRole::Tx);
                r.insert(l.b, PortRole::Rx);
            }
        }
        for l in self.labels.get(profile_id).into_iter().flatten() {
            if l.role != PortRole::Other {
                r.insert(l.port, l.role);
            }
        }
        r
    }

    pub fn label_name(&self, profile_id: &str, port: u16) -> String {
        self.labels
            .get(profile_id)
            .and_then(|v| v.iter().find(|l| l.port == port))
            .filter(|l| !l.name.trim().is_empty())
            .map(|l| format!("{} (P{port})", l.name.trim()))
            .unwrap_or_else(|| format!("P{port}"))
    }

    /// Carica dal file; se manca o è illeggibile ritorna il default (il file rotto viene
    /// rinominato `.bak` per non perderlo).
    pub fn load(path: &Path) -> (Self, Option<String>) {
        match std::fs::read_to_string(path) {
            Ok(txt) => match serde_json::from_str::<AppConfig>(&txt) {
                Ok(mut c) => {
                    c.sanitize();
                    (c, None)
                }
                Err(e) => {
                    let bak = path.with_extension("json.bak");
                    let _ = std::fs::rename(path, &bak);
                    (
                        Self::default(),
                        Some(format!(
                            "Configurazione illeggibile ({e}): salvata in {} e ripristinati i valori predefiniti",
                            bak.display()
                        )),
                    )
                }
            },
            Err(_) => (Self::default(), None),
        }
    }

    /// Scrittura atomica (file temporaneo + rename).
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("Impossibile creare {}: {e}", dir.display()))?;
        }
        let tmp: PathBuf = path.with_extension("json.tmp");
        let txt = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&tmp, txt).map_err(|e| format!("Scrittura configurazione fallita: {e}"))?;
        std::fs::rename(&tmp, path).map_err(|e| format!("Scrittura configurazione fallita: {e}"))
    }

    pub fn sanitize(&mut self) {
        self.version = 1;
        if self.profiles.is_empty() {
            *self = Self::default();
            return;
        }
        let ok = self
            .active_profile_id
            .as_deref()
            .map(|id| self.profiles.iter().any(|p| p.id == id))
            .unwrap_or(false);
        if !ok {
            self.active_profile_id = self.profiles.first().map(|p| p.id.clone());
        }
        for p in &mut self.profiles {
            if p.demo_ports == 0 {
                p.demo_ports = 24;
            }
        }
        for v in self.labels.values_mut() {
            v.retain(|l| l.port > 0);
        }
        for v in self.presets.values_mut() {
            for p in v.iter_mut() {
                p.links.retain(|l| l.a > 0 && l.b > 0);
            }
        }
    }
}

/// Preset dello script Python: `[{name, portA, portB}]`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyPreset {
    name: String,
    #[serde(rename = "portA")]
    port_a: serde_json::Value,
    #[serde(rename = "portB")]
    port_b: serde_json::Value,
}

fn as_port(v: &serde_json::Value) -> Option<u16> {
    match v {
        serde_json::Value::Number(n) => n.as_u64().and_then(|x| u16::try_from(x).ok()),
        serde_json::Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

pub fn parse_legacy_presets(txt: &str) -> Result<Vec<Preset>, String> {
    let raw: Vec<LegacyPreset> = serde_json::from_str(txt)
        .map_err(|e| format!("File non riconosciuto come tlx_presets.json: {e}"))?;
    let mut out = Vec::new();
    for p in raw {
        let (Some(a), Some(b)) = (as_port(&p.port_a), as_port(&p.port_b)) else {
            return Err(format!("Preset «{}»: porte non valide", p.name));
        };
        out.push(Preset {
            id: new_id(),
            name: p.name,
            links: vec![PresetLink { a, b, mode: LinkMode::Bidir }],
            color: String::new(),
            scene: false,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_import() {
        let txt = r#"[{"name":"Codec 1 ↔ Tablet A","portA":1,"portB":5},{"name":"x","portA":"4","portB":8}]"#;
        let p = parse_legacy_presets(txt).unwrap();
        assert_eq!(p.len(), 2);
        assert_eq!(p[1].links[0].a, 4);
        assert_eq!(p[0].links[0].mode, LinkMode::Bidir);
    }

    #[test]
    fn default_roundtrip() {
        let c = AppConfig::default();
        let s = serde_json::to_string(&c).unwrap();
        assert!(s.contains("\"activeProfileId\""));
        assert!(s.contains("\"bidirStrategy\":\"sequence\""));
        let back: AppConfig = serde_json::from_str(&s).unwrap();
        assert_eq!(back.profiles.len(), 2);
    }

    #[test]
    fn partial_config_gets_defaults() {
        let c: AppConfig = serde_json::from_str(r#"{"profiles":[{"name":"x"}]}"#).unwrap();
        assert_eq!(c.profiles[0].port, 17567);
        assert_eq!(c.settings.delay_ms, 300);
    }
}
