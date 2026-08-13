import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { useMixerStore } from "../store/mixer";
import { Ms } from "./Icons";

interface TestStatus {
  recording: boolean;
  playing: boolean;
  has_recording: boolean;
  recorded_peak_db?: number;
}

interface Diagnostic {
  id: string;
  icon: string;
  title: string;
}

const CHANNEL_DIAGNOSTICS: Record<string, Diagnostic[]> = {
  sink_game: [
    { id: "game_action", icon: "sports_esports", title: "Play CC0 action soundtrack" },
    { id: "game_footsteps", icon: "directions_walk", title: "Play CC0 footsteps" },
  ],
  sink_chat: [
    { id: "chat_female", icon: "record_voice_over", title: "Play CC0 female voice" },
    { id: "chat_male", icon: "spatial_audio_off", title: "Play CC0 male voice" },
  ],
  sink_media: [
    { id: "media_ambient", icon: "movie", title: "Play CC0 ambient soundtrack" },
    { id: "media_music", icon: "music_note", title: "Play CC0 music" },
  ],
  sink_aux: [
    { id: "media_ambient", icon: "movie", title: "Play CC0 ambient soundtrack" },
    { id: "media_music", icon: "music_note", title: "Play CC0 music" },
  ],
};

export function AudioTestControls({
  kind,
  sinkName,
  nodeName,
}: Readonly<{
  kind: "mic" | "channel";
  sinkName?: string;
  nodeName?: string;
}>) {
  const [status, setStatus] = useState<TestStatus>({
    recording: false,
    playing: false,
    has_recording: false,
  });
  const statusCommand = kind === "mic" ? "get_mic_test_status" : "get_channel_test_status";
  const args = kind === "channel" ? { sinkName } : { nodeName };

  useEffect(() => {
    let mounted = true;
    const refresh = () => {
      void invoke<TestStatus>(statusCommand, args)
        .then((next) => { if (mounted) setStatus(next); })
        .catch(() => {});
    };
    refresh();
    const timer = window.setInterval(refresh, 600);
    return () => {
      mounted = false;
      window.clearInterval(timer);
    };
  }, [statusCommand, sinkName, nodeName]);

  const run = (command: string, extra?: Record<string, unknown>) => {
    void invoke<TestStatus>(command, { ...args, ...extra })
      .then((next) => {
        setStatus(next);
        if (
          kind === "channel" &&
          command === "stop_channel_test_recording" &&
          next.has_recording &&
          (next.recorded_peak_db ?? -96) < -65
        ) {
          useMixerStore.setState({
            error: "The channel test captured silence. Start audio on this channel while Record is active.",
          });
        }
      })
      .catch((error) => useMixerStore.setState({ error: String(error) }));
  };
  const prefix = kind === "mic" ? "mic" : "channel";
  const diagnostics = kind === "channel"
    ? (CHANNEL_DIAGNOSTICS[sinkName ?? ""] ?? [
        { id: "media_ambient", icon: "movie", title: "Play CC0 ambient soundtrack" },
        { id: "media_music", icon: "music_note", title: "Play CC0 music" },
      ])
    : [];

  return (
    <div className="audio-test">
      <div className="audio-test-label">Test</div>
      <div className="audio-test-buttons">
        {diagnostics.map((diagnostic) => (
          <button
            type="button"
            key={diagnostic.id}
            className="audio-test-button"
            disabled={status.recording}
            title={diagnostic.title}
            onClick={() => run("play_channel_test_sample", { sample: diagnostic.id })}
          >
            <Ms name={diagnostic.icon} />
          </button>
        ))}
        <button
          type="button"
          className={"audio-test-button" + (status.recording ? " recording" : "")}
          disabled={status.playing}
          title={status.recording
            ? "Stop recording"
            : `Record up to ${kind === "mic" ? 30 : 12} seconds before processing`}
          onClick={() => run(status.recording
            ? `stop_${prefix}_test_recording`
            : `start_${prefix}_test_recording`)}
        >
          <Ms name={status.recording ? "stop" : "fiber_manual_record"} />
        </button>
        <button
          type="button"
          className={"audio-test-button" + (status.playing ? " playing" : "")}
          disabled={(!status.has_recording && !status.playing) || status.recording}
          title={status.playing
            ? "Stop playback"
            : kind === "channel" && status.has_recording
              ? `Play your recording through live processing (peak ${(status.recorded_peak_db ?? -96).toFixed(1)} dBFS)`
              : "Play your recording through the live processing"}
          onClick={() => run(status.playing
            ? `stop_${prefix}_test_playback`
            : `play_${prefix}_test_loop`)}
        >
          <Ms name={status.playing ? "stop" : "play_arrow"} />
        </button>
      </div>
    </div>
  );
}
