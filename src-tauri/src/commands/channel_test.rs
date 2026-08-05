use tauri::State;

use crate::audio::types::{ChannelTestAction, ChannelTestStatus};
use crate::state::AppState;

fn validate_channel(state: &AppState, sink_name: &str) -> Result<(), String> {
    let exists = state
        .lock_mixer()?
        .channel_defs
        .channels
        .iter()
        .any(|channel| channel.name == sink_name);
    exists
        .then_some(())
        .ok_or_else(|| format!("unknown playback channel {sink_name}"))
}

fn run(
    state: &AppState,
    sink_name: &str,
    action: ChannelTestAction,
) -> Result<ChannelTestStatus, String> {
    validate_channel(state, sink_name)?;
    state
        .backend
        .channel_test(sink_name, action)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_channel_test_status(
    state: State<'_, AppState>,
    sink_name: String,
) -> Result<ChannelTestStatus, String> {
    run(&state, &sink_name, ChannelTestAction::Status)
}

#[tauri::command]
pub fn start_channel_test_recording(
    state: State<'_, AppState>,
    sink_name: String,
) -> Result<ChannelTestStatus, String> {
    run(&state, &sink_name, ChannelTestAction::StartRecording)
}

#[tauri::command]
pub fn stop_channel_test_recording(
    state: State<'_, AppState>,
    sink_name: String,
) -> Result<ChannelTestStatus, String> {
    run(&state, &sink_name, ChannelTestAction::StopRecording)
}

#[tauri::command]
pub fn play_channel_test_loop(
    state: State<'_, AppState>,
    sink_name: String,
) -> Result<ChannelTestStatus, String> {
    run(&state, &sink_name, ChannelTestAction::StartPlayback)
}

#[tauri::command]
pub fn stop_channel_test_playback(
    state: State<'_, AppState>,
    sink_name: String,
) -> Result<ChannelTestStatus, String> {
    run(&state, &sink_name, ChannelTestAction::StopPlayback)
}

fn pcm16(bytes: &[u8]) -> Vec<i16> {
    bytes
        .chunks_exact(2)
        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
        .collect()
}

fn bundled_sample(name: &str) -> Result<Vec<i16>, String> {
    let bytes: &[u8] = match name {
        "game_action" => include_bytes!("../../assets/test-audio/game_action.s16le"),
        "game_footsteps" => include_bytes!("../../assets/test-audio/game_footsteps.s16le"),
        "chat_female" => include_bytes!("../../assets/test-audio/chat_female.s16le"),
        "chat_male" => include_bytes!("../../assets/test-audio/chat_male.s16le"),
        "media_ambient" => include_bytes!("../../assets/test-audio/media_ambient.s16le"),
        "media_music" => include_bytes!("../../assets/test-audio/media_music.s16le"),
        _ => return Err(format!("unknown bundled test sample {name}")),
    };
    Ok(pcm16(bytes))
}

#[tauri::command]
pub fn play_channel_test_sample(
    state: State<'_, AppState>,
    sink_name: String,
    sample: String,
) -> Result<ChannelTestStatus, String> {
    let samples = bundled_sample(&sample)?;
    run(
        &state,
        &sink_name,
        ChannelTestAction::LoadAndPlay {
            samples,
            channels: 2,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_samples_are_stereo_pcm_with_audio() {
        for name in [
            "game_action",
            "game_footsteps",
            "chat_female",
            "chat_male",
            "media_ambient",
            "media_music",
        ] {
            let samples = bundled_sample(name).unwrap();
            assert_eq!(samples.len() % 2, 0, "{name}");
            assert!(samples.len() >= 48_000 * 2, "{name}");
            assert!(samples.iter().any(|sample| *sample != 0), "{name}");
        }
    }

    #[test]
    fn unknown_sample_is_rejected() {
        assert!(bundled_sample("unlicensed_sample").is_err());
    }
}
