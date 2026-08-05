use tauri::State;

use crate::audio::types::{MicClient, MicConfig, MicTestAction, MicTestStatus, OutputDevice};
use crate::persistence::mic;
use crate::persistence::mic_presets::{self, MicPreset};
use crate::state::AppState;

#[tauri::command]
pub fn get_mic_config(state: State<'_, AppState>) -> Result<MicConfig, String> {
    let mixer = state.lock_mixer()?;
    Ok(mixer.mic.clone())
}

/// Apply and persist the mic chain configuration. The published label is
/// decorated per the device-naming preference at the backend boundary;
/// the stored config stays raw.
#[tauri::command]
pub fn set_mic_config(state: State<'_, AppState>, mut config: MicConfig) -> Result<(), String> {
    config.clamp_ranges();
    let mut applied = config.clone();
    applied.output_label = state.lock_mixer()?.prefs.decorate(&config.output_label);
    state
        .backend
        .set_mic_config(&applied)
        .map_err(|e| e.to_string())?;
    {
        let mut mixer = state.lock_mixer()?;
        mixer.mic = config.clone();
    }
    mic::save(&config).map_err(|e| e.to_string())
}

/// Hardware microphones available as the chain's input.
#[tauri::command]
pub fn get_input_devices(state: State<'_, AppState>) -> Result<Vec<OutputDevice>, String> {
    state
        .backend
        .list_input_devices()
        .map_err(|e| e.to_string())
}

/// Applications currently recording from the processed microphone.
#[tauri::command]
pub fn get_mic_clients(state: State<'_, AppState>) -> Result<Vec<MicClient>, String> {
    let mut clients = state
        .backend
        .list_mic_clients()
        .map_err(|error| error.to_string())?;
    for client in &mut clients {
        let binary = (client.match_prop == "application.process.binary")
            .then_some(client.match_value.as_str());
        let resolved = crate::audio::icons::resolve(
            &client.app_name,
            binary,
            client.icon_name.as_deref(),
            client.pid,
        );
        client.icon_path = resolved.icon_path;
        if let Some(name) = resolved.display_name {
            client.app_name = name;
        }
    }
    clients.sort_by(|left, right| left.app_name.cmp(&right.app_name));
    clients.dedup_by(|left, right| {
        left.match_prop == right.match_prop && left.match_value == right.match_value
    });
    Ok(clients)
}

#[tauri::command]
pub fn get_mic_test_status(state: State<'_, AppState>) -> Result<MicTestStatus, String> {
    state
        .backend
        .mic_test(MicTestAction::Status)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn start_mic_test_recording(state: State<'_, AppState>) -> Result<MicTestStatus, String> {
    state
        .backend
        .mic_test(MicTestAction::StartRecording)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn stop_mic_test_recording(state: State<'_, AppState>) -> Result<MicTestStatus, String> {
    state
        .backend
        .mic_test(MicTestAction::StopRecording)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn play_mic_test_loop(state: State<'_, AppState>) -> Result<MicTestStatus, String> {
    state
        .backend
        .mic_test(MicTestAction::StartLoop)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn stop_mic_test_playback(state: State<'_, AppState>) -> Result<MicTestStatus, String> {
    state
        .backend
        .mic_test(MicTestAction::StopLoop)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn list_mic_presets() -> Result<Vec<MicPreset>, String> {
    mic_presets::list().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn save_mic_preset(name: String, mut config: MicConfig) -> Result<(), String> {
    config.clamp_ranges();
    mic_presets::save(&name, &config).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn delete_mic_preset(name: String) -> Result<(), String> {
    mic_presets::delete(&name).map_err(|error| error.to_string())
}
