//! Safe IPC errors: never serialize third-party diagnostics or credentials.
use memivy_core::{memory::DataError, model::ProbeError};
use serde::Serialize;
#[derive(Clone, Debug, Serialize)]
pub struct HostError {
    pub code: &'static str,
}
impl HostError {
    pub const fn new(code: &'static str) -> Self {
        Self { code }
    }
}
impl std::fmt::Display for HostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code)
    }
}
impl std::error::Error for HostError {}
impl From<DataError> for HostError {
    fn from(value: DataError) -> Self {
        Self::new(match value {
            DataError::McpDisabled => "mcp_disabled",
            DataError::Io => "io",
            DataError::Database => "database",
            DataError::Busy => "busy",
            DataError::CollectionName => "collection_name",
            DataError::NavigationLimit => "navigation_limit",
            DataError::Invalid => "invalid",
            DataError::SourceAttribution => "source_attribution",
            DataError::Unavailable => "unavailable",
            DataError::Conflict => "conflict",
            DataError::RequestConflict => "request_conflict",
            DataError::Schema => "schema",
            DataError::Integrity => "integrity",
            DataError::DestinationExists => "destination_exists",
            DataError::SearchBudget => "search_budget",
        })
    }
}
impl From<ProbeError> for HostError {
    fn from(value: ProbeError) -> Self {
        Self::new(match value {
            ProbeError::Configuration => "model_configuration",
            ProbeError::Endpoint => "model_endpoint",
            ProbeError::Network => "model_network",
            ProbeError::Status(401 | 403) => "model_authentication",
            ProbeError::Status(429) => "model_rate_limit",
            ProbeError::Status(_) => "model_status",
            ProbeError::TooLarge => "model_too_large",
            ProbeError::InvalidResponse => "model_invalid_response",
            ProbeError::Truncated => "model_truncated",
            ProbeError::ToolsUnsupported => "model_tools_unsupported",
        })
    }
}
pub const KNOWN_CODES: &[&str] = &[
    "model_test_agent",
    "model_test_agent_unsupported",
    "agent_budget",
    "model_cache_path",
    "model_cache_io",
    "model_cache_mismatch",
    "model_download_incomplete",
    "model_download_paused",
    "model_download_network",
    "model_download_range",
    "model_download_size",
    "embedding_disabled",
    "embedding_component_missing",
    "embedding_input_invalid",
    "embedding_warming",
    "embedding_connection_lost",
    "embedding_timeout",
    "embedding_response_invalid",
    "embedding_configuration_changed",
    "embedding_busy",
    "embedding_settings_invalid",
    "embedding_clearing",
    "embedding_load_failed",
    "embedding_index_failed",
    "embedding_model_missing",
    "embedding_index_missing",
    "embedding_index_rebuilding",
    "embedding_metal_unavailable",
    "embedding_clear_failed",
    "model_configuration_invalid",
    "model_configuration_path",
    "model_configuration_lock",
    "model_configuration_read",
    "model_configuration_save",
    "model_configuration_too_large",
    "model_connection_invalid",
    "model_connection_limit",
    "model_connection_missing",
    "model_dimensions_mismatch",
    "model_dimensions_required",
    "model_input_invalid",
    "model_local_binding",
    "voice_cache_unavailable",
    "voice_configuration_invalid",
    "voice_service_failed",
    "voice_download_failed",
    "app_path_unavailable",
    "audio_conversion_failed",
    "audio_interrupted",
    "busy",
    "cancelled",
    "capture_mode_invalid",
    "capture_origin_invalid",
    "capture_source_required",
    "capture_window_required",
    "capture_window_unavailable",
    "cleanup_busy",
    "cleanup_unavailable",
    "collection_name",
    "configuration_conflict",
    "configuration_restore_failed",
    "conflict",
    "context_limit",
    "database",
    "desktop_dialog_active",
    "desktop_settings_confirm_failed",
    "desktop_settings_invalid",
    "desktop_settings_save_failed",
    "desktop_settings_unreadable",
    "desktop_status_failed",
    "destination_exists",
    "discussion_unavailable",
    "display_unavailable",
    "embedding_settings_failed",
    "file_dialog_failed",
    "forbidden",
    "index_confirmation_required",
    "integration_failed",
    "integrity",
    "invalid",
    "io",
    "login_status_failed",
    "login_update_failed",
    "main_window_required",
    "main_window_unavailable",
    "mcp_diagnostic_failed",
    "mcp_disabled",
    "mcp_invalid_response",
    "mcp_missing",
    "mcp_protocol_failed",
    "mcp_settings_failed",
    "mcp_start_failed",
    "mcp_timeout",
    "mcp_tools_mismatch",
    "mcp_version_mismatch",
    "microphone_format",
    "microphone_missing",
    "microphone_permission",
    "microphone_unavailable",
    "microphone_start_timeout",
    "model_authentication",
    "model_configuration",
    "model_delete_failed",
    "model_endpoint",
    "model_invalid_response",
    "model_network",
    "model_rate_limit",
    "model_required",
    "model_status",
    "model_test_failed",
    "model_test_required",
    "model_test_unavailable",
    "model_too_large",
    "model_tools_unsupported",
    "model_truncated",
    "native_menu_unavailable",
    "navigation_limit",
    "operation_failed",
    "quit_dialog_active",
    "quit_draft_unconfirmed",
    "recommendation_failed",
    "request_conflict",
    "restore_busy",
    "schema",
    "search_budget",
    "source_attribution",
    "shortcut_invalid",
    "shortcut_modifiers_required",
    "shortcut_register_failed",
    "shortcut_remove_failed",
    "shortcut_reserved",
    "shortcut_taken",
    "speech_test_failed",
    "unavailable",
    "voice_action_invalid",
    "voice_audio_invalid",
    "voice_audio_save_failed",
    "voice_audio_unreadable",
    "voice_busy",
    "voice_component_missing",
    "voice_connection_lost",
    "voice_directory_invalid",
    "voice_disabled",
    "voice_download_required",
    "voice_draft_exists",
    "voice_endpoint_changed",
    "voice_incomplete",
    "voice_no_audio",
    "voice_no_transcription",
    "voice_initialization_failed",
    "voice_interrupted",
    "voice_not_loaded",
    "voice_pipe_unavailable",
    "voice_recording_active",
    "voice_request_invalid",
    "voice_response_invalid",
    "voice_session_ended",
    "voice_settings_invalid",
    "voice_settings_unreadable",
    "voice_shortcut_failed",
    "voice_target_invalid",
    "voice_timeout",
    "voice_transcription_failed",
    "window_drag_failed",
    "window_drag_invalid",
    "window_geometry_failed",
    "window_hide_failed",
    "window_not_ready",
    "window_operation_failed",
    "window_position_save_failed",
    "window_size_invalid",
];

// Map string errors from native subsystems to known error codes.
// Never inspect translated text to infer an error type.
impl From<String> for HostError {
    fn from(value: String) -> Self {
        Self::from(value.as_str())
    }
}
impl From<&str> for HostError {
    fn from(value: &str) -> Self {
        Self::new(
            KNOWN_CODES
                .iter()
                .copied()
                .find(|code| *code == value)
                .unwrap_or("operation_failed"),
        )
    }
}
impl From<HostError> for String {
    fn from(value: HostError) -> Self {
        value.code.to_owned()
    }
}

impl From<memivy_core::models::Error> for HostError {
    fn from(value: memivy_core::models::Error) -> Self {
        Self::new(value.code())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn known_codes_have_both_translations_and_legacy_chinese_is_not_inferred() {
        let en: serde_json::Value =
            serde_json::from_str(include_str!("../../locales/en/errors.json")).unwrap();
        let zh: serde_json::Value =
            serde_json::from_str(include_str!("../../locales/zh-CN/errors.json")).unwrap();
        for code in KNOWN_CODES {
            assert!(
                en.get(code)
                    .and_then(|v| v.as_str())
                    .is_some_and(|v| !v.trim().is_empty()),
                "{code}"
            );
            assert!(
                zh.get(code)
                    .and_then(|v| v.as_str())
                    .is_some_and(|v| !v.trim().is_empty()),
                "{code}"
            );
            assert_eq!(HostError::from(*code).code, *code);
        }
        assert_eq!(
            HostError::from(zh["conflict"].as_str().unwrap()).code,
            "operation_failed"
        );
    }
    #[test]
    fn preserves_conflict_and_redacts_unknown() {
        assert_eq!(HostError::from(DataError::Conflict).code, "conflict");
        assert_eq!(
            HostError::from(ProbeError::Status(429)).code,
            "model_rate_limit"
        );
        assert_eq!(
            serde_json::to_string(&HostError::from("secret api key".to_owned())).unwrap(),
            r#"{"code":"operation_failed"}"#
        );
    }
}
