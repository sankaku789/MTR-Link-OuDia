use std::path::Path;

use mtr_oudia_application::{
    BusinessError, CandidateId, ConversionService, ConversionSessionStore, ManualMappingInput,
    MtrEndpoint, PreviewId, SessionId,
};
use mtr_oudia_domain::OperationPolicy;
use mtr_oudia_infrastructure::{
    FileMinecraftLogProvider, FileOudiaRepository, JsonSettingsRepository, ReqwestMtrApiClient,
    SafeOudiaWriter, WindowsListeningPortProvider,
};
use serde::{Deserialize, Serialize};

type Service = ConversionService<
    'static,
    WindowsListeningPortProvider,
    ReqwestMtrApiClient,
    FileOudiaRepository,
    JsonSettingsRepository,
    SafeOudiaWriter,
>;

/// GUI 用 DTO に業務エラーの分類と安全な表示文だけを渡す。
#[derive(Debug, Clone, Serialize)]
pub struct ErrorDto {
    pub kind: String,
    pub message: String,
    pub detail: Option<String>,
}
impl From<BusinessError> for ErrorDto {
    fn from(value: BusinessError) -> Self {
        Self {
            kind: format!("{:?}", value.kind),
            message: value.message,
            detail: value.detail,
        }
    }
}

pub struct AppState {
    service: Service,
}

fn settings_path() -> std::path::PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from))
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".config"))
        })
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    base.join("MtrOudiaConverter").join("settings.json")
}

fn minecraft_log_paths() -> Vec<std::path::PathBuf> {
    let mut paths = Vec::new();
    if let Some(path) = std::env::var_os("MTR_OUDIA_MINECRAFT_LOG") {
        paths.push(std::path::PathBuf::from(path));
    }
    if let Some(appdata) = std::env::var_os("APPDATA") {
        paths.push(
            std::path::PathBuf::from(appdata)
                .join(".minecraft")
                .join("logs")
                .join("latest.log"),
        );
    }
    paths
}

fn compose_state() -> Result<AppState, String> {
    // Tauri のプロセス寿命と同じ managed state なので、adapter は一度だけ確保して参照する。
    let ports = Box::leak(Box::new(WindowsListeningPortProvider));
    let minecraft_log = Box::leak(Box::new(
        FileMinecraftLogProvider::new(minecraft_log_paths()).with_running_minecraft(),
    ));
    let client = Box::leak(Box::new(
        ReqwestMtrApiClient::new().map_err(|error| error.to_string())?,
    ));
    let repository = Box::leak(Box::new(FileOudiaRepository));
    let settings = Box::leak(Box::new(JsonSettingsRepository::new(settings_path())));
    let saver = Box::leak(Box::new(SafeOudiaWriter::new()));
    Ok(AppState {
        service: ConversionService::new(
            ConversionSessionStore::new(16),
            ports,
            client,
            repository,
            settings,
            saver,
        )
        .with_minecraft_log(minecraft_log),
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRequest {
    session_id: Option<String>,
}
fn session(state: &AppState, input: SessionRequest) -> SessionId {
    input
        .session_id
        .map(SessionId)
        .unwrap_or_else(|| state.service.create_session())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotRequest {
    session_id: String,
    endpoint: String,
    dimension: u32,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OudiaRequest {
    session_id: String,
    path: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidatesRequest {
    session_id: String,
    route_id: String,
    diagram_index: Option<usize>,
    train_type: Option<usize>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewRequest {
    session_id: String,
    candidate_id: String,
    manual_mappings: Option<ManualMappingInput>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveRequest {
    session_id: String,
    preview_id: String,
    output_path: String,
    policy: String,
}

#[tauri::command]
async fn detect_mtr_endpoints(
    state: tauri::State<'_, AppState>,
    input: SessionRequest,
) -> Result<(String, Vec<mtr_oudia_application::EndpointDto>), ErrorDto> {
    let id = session(&state, input);
    let endpoints = state.service.detect_mtr_endpoint(&id).await?;
    Ok((id.0, endpoints))
}

#[tauri::command]
async fn fetch_mtr_snapshot(
    state: tauri::State<'_, AppState>,
    input: SnapshotRequest,
) -> Result<mtr_oudia_application::SnapshotDto, ErrorDto> {
    let endpoint = MtrEndpoint::parse(&input.endpoint).map_err(|_| ErrorDto {
        kind: "Validation".into(),
        message: "接続先 URL が不正です".into(),
        detail: None,
    })?;
    state
        .service
        .fetch_mtr_snapshot(&SessionId(input.session_id), &endpoint, input.dimension)
        .await
        .map_err(Into::into)
}

#[tauri::command]
fn inspect_oudia(
    state: tauri::State<'_, AppState>,
    input: OudiaRequest,
) -> Result<mtr_oudia_application::InspectionDto, ErrorDto> {
    state
        .service
        .inspect_oudia(&SessionId(input.session_id), Path::new(&input.path))
        .map_err(Into::into)
}

#[tauri::command]
fn find_route_candidates(
    state: tauri::State<'_, AppState>,
    input: CandidatesRequest,
) -> Result<Vec<mtr_oudia_application::RouteCandidateDto>, ErrorDto> {
    state
        .service
        .find_route_candidates(
            &SessionId(input.session_id),
            &input.route_id,
            input.diagram_index,
            input.train_type,
        )
        .map_err(Into::into)
}

#[tauri::command]
fn build_preview(
    state: tauri::State<'_, AppState>,
    input: PreviewRequest,
) -> Result<mtr_oudia_application::PreviewDto, ErrorDto> {
    state
        .service
        .build_preview(
            &SessionId(input.session_id),
            Some(&CandidateId(input.candidate_id)),
            input.manual_mappings,
        )
        .map_err(Into::into)
}

#[tauri::command]
fn save_conversion(
    state: tauri::State<'_, AppState>,
    input: SaveRequest,
) -> Result<mtr_oudia_application::SaveReceipt, ErrorDto> {
    let policy = match input.policy.as_str() {
        "preserve" => OperationPolicy::Preserve,
        "remove_target_train" => OperationPolicy::RemoveTargetTrain,
        _ => {
            return Err(ErrorDto {
                kind: "Validation".into(),
                message: "Operation 方針が不正です".into(),
                detail: None,
            })
        }
    };
    state
        .service
        .save_conversion(
            &SessionId(input.session_id),
            &PreviewId(input.preview_id),
            Path::new(&input.output_path),
            policy,
        )
        .map_err(Into::into)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let state = compose_state().expect("Infrastructure の初期化に失敗しました");
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            detect_mtr_endpoints,
            fetch_mtr_snapshot,
            inspect_oudia,
            find_route_candidates,
            build_preview,
            save_conversion
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn business_error_dto_does_not_expose_internal_error() {
        let dto = ErrorDto::from(BusinessError {
            kind: mtr_oudia_application::BusinessErrorKind::Io,
            message: "保存できません".into(),
            detail: None,
        });
        assert_eq!(dto.kind, "Io");
        assert_eq!(dto.message, "保存できません");
    }

    #[test]
    fn preview_request_accepts_index_mapping_without_patch_data() {
        let json = serde_json::to_string(&serde_json::json!({
            "sessionId":"s", "candidateId":"c",
            "manualMappings":{"station_mappings":[{"mtr_station_index":0,"oudia_station_slot":3}]}
        }))
        .unwrap();
        let request: PreviewRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(request.session_id, "s");
        assert_eq!(request.candidate_id, "c");
        assert_eq!(
            request.manual_mappings.unwrap().station_mappings[0].oudia_station_slot,
            3
        );
        assert!(!json.contains("sourceRange") && !json.contains("patch") && !json.contains("time"));
    }
}
