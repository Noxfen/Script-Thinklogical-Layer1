//! Tipi scambiati con la UI (camelCase, vedi docs/IPC.md) e comandi Tauri.

use crate::config::{self, AppConfig, LinkMode, Preset, PresetLink};
use crate::plan::DiscMode;
use crate::state::{now_ms, App};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::State;
use tauri_plugin_dialog::DialogExt;

type St<'a> = State<'a, Arc<App>>;

// ───────────────────────── DTO ─────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnStateDto {
    pub status: String,
    pub profile_id: Option<String>,
    pub endpoint: String,
    pub error: Option<String>,
    pub since: u64,
}

impl ConnStateDto {
    pub fn from_core(s: &tlx_core::client::ConnState, profile_id: &str) -> Self {
        Self {
            status: s.status.to_string(),
            profile_id: Some(profile_id.to_string()),
            endpoint: s.endpoint.clone(),
            error: s.error.clone(),
            since: s.since_ms,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatrixInfoDto {
    pub version: String,
    pub xtype: String,
    pub max_ports: u16,
    pub max_cards: u16,
    pub installed: Vec<bool>,
    pub options: u16,
    pub upper_only: bool,
    pub real_card_count: bool,
    pub p2p: bool,
    pub mcast_status: bool,
    pub xctime: bool,
    pub los_supported: Option<bool>,
}

impl From<tlx_core::protocol::MatrixInfo> for MatrixInfoDto {
    fn from(i: tlx_core::protocol::MatrixInfo) -> Self {
        Self {
            version: i.version,
            xtype: i.xtype,
            max_ports: i.max_ports,
            max_cards: i.max_cards,
            installed: i.installed,
            options: i.options,
            upper_only: i.upper_only,
            real_card_count: i.real_card_count,
            p2p: i.p2p,
            mcast_status: i.mcast_status,
            xctime: i.xctime,
            los_supported: i.los_supported,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutingDto {
    pub max_ports: u16,
    pub outputs: Vec<u16>,
    pub source: String,
    pub updated_at: u64,
}

impl RoutingDto {
    pub fn from_core(r: &tlx_core::client::RoutingUpdate) -> Self {
        Self {
            max_ports: r.routing.max_ports,
            outputs: r.routing.outputs.clone(),
            source: r.source.to_string(),
            updated_at: r.updated_at_ms,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsDto {
    pub alarm: Option<u16>,
    pub los: Vec<Option<bool>>,
    pub rx_uw: Vec<Option<u16>>,
    pub tx_uw: Vec<Option<u16>>,
    pub updated_at: u64,
}

impl DiagnosticsDto {
    pub fn from_core(d: tlx_core::client::Diagnostics, ts: u64) -> Self {
        Self {
            alarm: d.alarm,
            los: d.los.map(|v| v.into_iter().map(Some).collect()).unwrap_or_default(),
            rx_uw: d.rx_uw.map(|v| v.into_iter().map(Some).collect()).unwrap_or_default(),
            tx_uw: d.tx_uw.map(|v| v.into_iter().map(Some).collect()).unwrap_or_default(),
            updated_at: ts,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntryDto {
    pub id: u64,
    pub ts: u64,
    pub level: String,
    pub channel: String,
    pub message: String,
    pub latency_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CmdResultDto {
    pub ok: bool,
    pub command: String,
    pub response: Option<String>,
    pub data: Option<String>,
    pub error_code: Option<String>,
    pub comment: Option<String>,
    pub error: Option<String>,
    pub latency_ms: u64,
}

impl CmdResultDto {
    pub fn from_core(o: tlx_core::session::CmdOutcome) -> Self {
        let (ok, response, data, error_code, comment) = match &o.response {
            Some(r) => {
                (r.ok, Some(r.raw.clone()), r.data.clone(), r.error_code.clone(), r.comment.clone())
            }
            None => (false, None, None, None, None),
        };
        Self {
            ok: ok && o.error.is_none(),
            command: o.command,
            response,
            data,
            error_code,
            comment,
            error: o.error,
            latency_ms: o.latency_ms,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionResultDto {
    pub ok: bool,
    pub steps: Vec<CmdResultDto>,
    pub verified: Option<bool>,
    pub mismatches: Vec<String>,
    pub message: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotDto {
    pub conn: ConnStateDto,
    pub info: Option<MatrixInfoDto>,
    pub routing: Option<RoutingDto>,
    pub diagnostics: Option<DiagnosticsDto>,
    pub can_undo: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RouteChange {
    pub input: u16,
    pub output: u16,
}

// ───────────────────────── comandi ─────────────────────────

#[tauri::command]
pub fn get_config(st: St) -> AppConfig {
    st.config()
}

#[tauri::command]
pub fn save_config(st: St, config: AppConfig) -> Result<(), String> {
    st.save_config(config)
}

#[tauri::command]
pub async fn export_config(app: tauri::AppHandle, st: St<'_>) -> Result<Option<String>, String> {
    let Some(path) = app
        .dialog()
        .file()
        .set_file_name("tlx-control-configurazione.json")
        .add_filter("JSON", &["json"])
        .blocking_save_file()
    else {
        return Ok(None);
    };
    let path = path.into_path().map_err(|e| e.to_string())?;
    let txt = serde_json::to_string_pretty(&st.config()).map_err(|e| e.to_string())?;
    std::fs::write(&path, txt).map_err(|e| format!("Salvataggio fallito: {e}"))?;
    Ok(Some(path.display().to_string()))
}

#[tauri::command]
pub async fn import_config(app: tauri::AppHandle, st: St<'_>) -> Result<Option<AppConfig>, String> {
    let Some(path) = app.dialog().file().add_filter("JSON", &["json"]).blocking_pick_file() else {
        return Ok(None);
    };
    let path = path.into_path().map_err(|e| e.to_string())?;
    let txt = std::fs::read_to_string(&path).map_err(|e| format!("Lettura fallita: {e}"))?;
    let cfg: AppConfig = serde_json::from_str(&txt)
        .map_err(|e| format!("File di configurazione non valido: {e}"))?;
    st.save_config(cfg)?;
    Ok(Some(st.config()))
}

#[tauri::command]
pub async fn import_legacy_presets(
    app: tauri::AppHandle,
    st: St<'_>,
    profile_id: String,
) -> Result<u32, String> {
    let Some(path) = app
        .dialog()
        .file()
        .set_title("Scegli tlx_presets.json dello script Python")
        .add_filter("JSON", &["json"])
        .blocking_pick_file()
    else {
        return Ok(0);
    };
    let path = path.into_path().map_err(|e| e.to_string())?;
    let txt = std::fs::read_to_string(&path).map_err(|e| format!("Lettura fallita: {e}"))?;
    let new = config::parse_legacy_presets(&txt)?;
    let n = new.len() as u32;
    let mut cfg = st.config();
    if cfg.profile(&profile_id).is_none() {
        return Err("Profilo inesistente".into());
    }
    cfg.presets.entry(profile_id).or_default().extend(new);
    st.save_config(cfg)?;
    st.log("info", "app", &format!("Importati {n} preset da {}", path.display()), None);
    Ok(n)
}

#[tauri::command]
pub async fn export_log(app: tauri::AppHandle, st: St<'_>) -> Result<Option<String>, String> {
    let Some(path) = app
        .dialog()
        .file()
        .set_file_name("tlx-control-log.csv")
        .add_filter("CSV", &["csv"])
        .blocking_save_file()
    else {
        return Ok(None);
    };
    let path = path.into_path().map_err(|e| e.to_string())?;
    // BOM UTF-8: Excel apre correttamente accenti e frecce
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend(st.log_csv().into_bytes());
    std::fs::write(&path, bytes).map_err(|e| format!("Salvataggio fallito: {e}"))?;
    Ok(Some(path.display().to_string()))
}

#[tauri::command]
pub fn list_serial_ports() -> Vec<String> {
    tlx_core::transport::list_serial_ports()
}

#[tauri::command]
pub async fn connect(st: St<'_>, profile_id: String) -> Result<(), String> {
    st.connect(&profile_id).await
}

#[tauri::command]
pub async fn disconnect(st: St<'_>) -> Result<(), String> {
    st.disconnect().await;
    Ok(())
}

#[tauri::command]
pub fn get_snapshot(st: St) -> SnapshotDto {
    st.snapshot()
}

#[tauri::command]
pub async fn refresh_routing(st: St<'_>) -> Result<RoutingDto, String> {
    st.refresh().await
}

#[tauri::command]
pub async fn apply_routes(
    st: St<'_>,
    changes: Vec<RouteChange>,
) -> Result<ActionResultDto, String> {
    st.apply_routes(changes.into_iter().map(|c| (c.input, c.output)).collect()).await
}

#[tauri::command]
pub async fn link(st: St<'_>, a: u16, b: u16, mode: LinkMode) -> Result<ActionResultDto, String> {
    st.link(a, b, mode).await
}

#[tauri::command]
pub async fn disconnect_port(
    st: St<'_>,
    port: u16,
    mode: String,
) -> Result<ActionResultDto, String> {
    let m = match mode.as_str() {
        "input" => DiscMode::Input,
        "output" => DiscMode::Output,
        "both" => DiscMode::Both,
        other => return Err(format!("Modalità di scollegamento sconosciuta: {other}")),
    };
    st.disconnect_port(port, m).await
}

#[tauri::command]
pub async fn disconnect_all(st: St<'_>) -> Result<ActionResultDto, String> {
    st.disconnect_all().await
}

#[tauri::command]
pub async fn execute_preset(
    st: St<'_>,
    profile_id: String,
    preset_id: String,
) -> Result<ActionResultDto, String> {
    st.execute_preset(&profile_id, &preset_id).await
}

#[tauri::command]
pub async fn snapshot_to_preset(st: St<'_>, name: String) -> Result<Preset, String> {
    let links = st.snapshot_links().await?;
    if links.is_empty() {
        return Err("La matrice non ha collegamenti attivi da salvare".into());
    }
    Ok(Preset {
        id: config::new_id(),
        name: if name.trim().is_empty() {
            format!("Situazione {}", crate::state::fmt_ts(now_ms()))
        } else {
            name
        },
        links: links.into_iter().map(|(a, b, mode)| PresetLink { a, b, mode }).collect(),
        color: String::new(),
        scene: true,
    })
}

#[tauri::command]
pub async fn undo_last(st: St<'_>) -> Result<ActionResultDto, String> {
    st.undo().await
}

#[tauri::command]
pub async fn send_raw(st: St<'_>, command: String, force: bool) -> Result<CmdResultDto, String> {
    st.raw(&command, force).await
}

#[tauri::command]
pub async fn read_diagnostics(st: St<'_>) -> Result<DiagnosticsDto, String> {
    st.diagnostics().await
}

#[tauri::command]
pub fn clear_log(st: St) {
    st.clear_log()
}

#[tauri::command]
pub fn get_log(st: St) -> Vec<LogEntryDto> {
    st.get_log()
}
