// Mirrors the Rust structs in src-tauri/src/audio/types.rs - keep in sync.

export interface AppStream {
  index: number;
  app_name: string;
  /** PipeWire property the identity was read from. */
  match_prop: string;
  /** Raw property value (stream identity for persistence). */
  match_value: string;
  /** User-chosen display name overriding app_name. */
  alias: string | null;
  icon_name: string | null;
  /** Resolved absolute icon file path (desktop-entry based). */
  icon_path: string | null;
  /** Canonical desktop-file id used to group helper processes. */
  desktop_id: string | null;
  /** Producing process id (used backend-side for icon resolution). */
  pid: number | null;
  /** Name of the virtual sink the stream is routed to, if any. */
  assigned_sink: string | null;
  volume_percent: number;
  muted: boolean;
  /** True while the stream is actively producing audio. */
  active: boolean;
}

/** An application currently recording from the processed virtual mic. */
export interface MicClient {
  index: number;
  mic_node: string;
  app_name: string;
  match_prop: string;
  match_value: string;
  icon_name: string | null;
  icon_path: string | null;
  pid: number | null;
  active: boolean;
}

export interface VirtualSink {
  /** e.g. "sink_game" */
  name: string;
  /** e.g. "Game" */
  label: string;
  /** Material Symbol for the strip icon. */
  icon: string | null;
  volume_percent: number;
  muted: boolean;
  /** Whether this channel feeds the Stream Mix source (OBS recording). */
  stream_mix: boolean;
}

export interface OutputDevice {
  index: number;
  name: string;
  description: string;
}

/** Visual-only VU meter refresh policy; audio processing is unaffected. */
export type MeterMode = "monitor" | "fps_144" | "fps_120" | "fps_100" | "fps_60" | "off";

/** Phase 3 mic chain configuration (mirrors Rust MicConfig). */
export interface MicConfig {
  /** Stable PipeWire node name; sink_mic is the permanent primary. */
  node_name: string;
  enabled: boolean;
  /** node.name of the hardware mic (null = system default). */
  input_device: string | null;
  /** What other apps list the processed mic as. */
  output_label: string;
  /** 0-200; 100 = unity. */
  gain_percent: number;
  gate_enabled: boolean;
  comp_enabled: boolean;
  limiter_enabled: boolean;
  muted: boolean;
  /** Parametric EQ applied before mic dynamics. */
  eq_enabled: boolean;
  eq_preamp_db: number;
  eq_bands: EqBand[];
  gate_threshold_db: number;
  comp_threshold_db: number;
  comp_ratio: number;
  limiter_ceiling_db: number;
}

/** Default DSP values (markers on the tuning sliders). */
export const MIC_DSP_DEFAULTS = {
  gate_threshold_db: -40,
  comp_threshold_db: -18,
  comp_ratio: 3,
  limiter_ceiling_db: -1,
} as const;

/** Parametric EQ band shapes (mirrors Rust EqBandKind). */
export type EqBandKind =
  | "peaking"
  | "low_shelf"
  | "high_shelf"
  | "low_pass"
  | "high_pass";

/** One parametric EQ band (mirrors Rust EqBand). */
export interface EqBand {
  kind: EqBandKind;
  freq_hz: number;
  /** Ignored by low_pass/high_pass. */
  gain_db: number;
  /** Peaking/LP/HP: filter Q. Shelves: RBJ shelf slope. */
  q: number;
}

/** A channel's parametric EQ (mirrors Rust EqConfig). */
export interface EqConfig {
  enabled: boolean;
  /** Headroom trim applied before the band cascade (dB). */
  preamp_db: number;
  bands: EqBand[];
  /** Separate broad tone stage; intentionally not drawn as parametric dots. */
  tone_bass_db: number;
  tone_voice_db: number;
  tone_treble_db: number;
  /** Post-EQ playback gain. */
  boost_db: number;
  gate_enabled: boolean;
  gate_threshold_db: number;
  comp_enabled: boolean;
  comp_threshold_db: number;
  comp_ratio: number;
  limiter_enabled: boolean;
  limiter_ceiling_db: number;
  /** Direct stereo for speakers; low-frequency crossfeed for headphones. */
  playback_mode: "speakers" | "headphones";
  /** Real 7.1-to-binaural HRTF virtualization on Game and Media. */
  spatial_enabled: boolean;
  /** 0 = crisp directional performance, 0.5 = neutral, 1 = diffuse immersion. */
  spatial_tuning: number;
  /** 0 = close/+2.5 dB, 0.5 = neutral, 1 = far/-2.5 dB. */
  spatial_distance: number;
}

export const MAX_EQ_BANDS = 10;
export const EQ_GAIN_RANGE_DB = 24;
export const TONE_GAIN_RANGE_DB = 12;
export const EQ_FREQ_MIN_HZ = 20;
export const EQ_FREQ_MAX_HZ = 20000;

/** The gaming-audio starting layout (mirrors Rust default_eq_bands). */
export const DEFAULT_EQ_BANDS: EqBand[] = [
  { kind: "low_shelf", freq_hz: 100, gain_db: 0, q: 0.71 },
  { kind: "peaking", freq_hz: 500, gain_db: 0, q: 1 },
  { kind: "peaking", freq_hz: 1500, gain_db: 0, q: 1 },
  { kind: "peaking", freq_hz: 5000, gain_db: 0, q: 1 },
  { kind: "high_shelf", freq_hz: 10000, gain_db: 0, q: 0.71 },
];

/** A channel's EQ when it has never been configured. */
export function defaultEqConfig(): EqConfig {
  return {
    enabled: false,
    preamp_db: 0,
    bands: DEFAULT_EQ_BANDS.map((b) => ({ ...b })),
    tone_bass_db: 0,
    tone_voice_db: 0,
    tone_treble_db: 0,
    boost_db: 0,
    gate_enabled: false,
    gate_threshold_db: -48,
    comp_enabled: false,
    comp_threshold_db: -18,
    comp_ratio: 3,
    limiter_enabled: false,
    limiter_ceiling_db: -1,
    playback_mode: "speakers",
    spatial_enabled: false,
    spatial_tuning: 0.5,
    spatial_distance: 0.5,
  };
}

/** App history entry (mirrors Rust SeenApp). */
export interface SeenApp {
  match_prop: string;
  match_value: string;
  display_name: string;
  icon_name: string | null;
  icon_path: string | null;
  /** Canonical desktop-file id; raw match fields still drive routing. */
  desktop_id: string | null;
  /** Unix seconds of the last sighting. */
  last_seen: number;
  ignored: boolean;
  assigned_sink: string | null;
  alias: string | null;
}

export interface AppIdentity {
  match_prop: string;
  match_value: string;
}

/** A UI-level application backed by one or more exact routing identities. */
export interface SeenAppGroup extends SeenApp {
  group_key: string;
  identities: AppIdentity[];
  assignment_mixed: boolean;
  assigned_sinks: string[];
}

/** A user-defined mix (record bus). The label is what recorders display. */
export interface BusDef {
  name: string;
  label: string;
  /** Manual mode: carried channels. Auto-include mode: excluded channels. */
  channels: string[];
  /** True = carries everything except `channels`; new channels join automatically. */
  exclude: boolean;
  /** Playback level recorders hear (0-150%). Persisted with the mix. */
  volume_percent: number;
  /** Muted for recorders (they hear silence). Persisted with the mix. */
  muted: boolean;
}

/** The channels a mix actually carries, given the full channel set. */
export function busMembers(bus: BusDef, allChannels: string[]): string[] {
  return bus.exclude
    ? allChannels.filter((c) => !bus.channels.includes(c))
    : bus.channels;
}

/** Profile listing entry (Phase 5: trigger_device auto-loads the profile). */
export interface ProfileInfo {
  name: string;
  trigger_device: string | null;
  protected: boolean;
}

export interface ProfileContent {
  channels: VirtualSink[];
  mic: MicConfig;
  secondary_mics: MicConfig[];
}

export interface ApplicationProfileRule {
  executable: string;
  path: string | null;
  profile: string;
  enabled: boolean;
}

export interface ProfileAutomationConfig {
  enabled: boolean;
  return_profile: string | null;
  notifications: boolean;
  rules: ApplicationProfileRule[];
}

export interface RunningApplication {
  executable: string;
  path: string;
}

export interface ProfileAutomationStatus {
  automatic_profile: string | null;
  matched_executable: string | null;
  manual_override: boolean;
  error: string | null;
}

/** Sent as sink_name to unassign a stream (backend moves it to the default sink). */
export const UNASSIGNED = "";

export const MAX_VOLUME = 150;
export const MAX_MIC_GAIN = 200;
/** Levels key for the mic chain. */
export const MIC_LEVEL_KEY = "sink_mic";
/** Node name of the always-on master mix (carries every channel). */
export const MASTER_BUS = "sink_stream";
