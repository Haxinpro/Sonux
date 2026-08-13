import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { EqConfig } from "../../types";
import { Ms } from "../Icons";
import { Popover } from "../Popover";

interface EqPresetEntry {
  source: "bundled" | "user";
  preset: {
    schema: number;
    name: string;
    author?: string | null;
    description?: string | null;
    enabled?: boolean;
    preamp_db: number;
    bands: EqConfig["bands"];
    tone_bass_db?: number;
    tone_voice_db?: number;
    tone_treble_db?: number;
    boost_db: number;
    gate_enabled: boolean;
    gate_threshold_db: number;
    comp_enabled: boolean;
    comp_threshold_db: number;
    comp_ratio: number;
    limiter_enabled: boolean;
    limiter_ceiling_db: number;
    playback_mode: EqConfig["playback_mode"];
    spatial_enabled: boolean;
    spatial_tuning: number;
    spatial_distance: number;
  };
}

interface PresetSelection {
  source: EqPresetEntry["source"];
  name: string;
}

const ACTIVE_PRESET_KEY = "sonux-active-eq-presets";
const LEGACY_ACTIVE_PRESET_KEY = "sink-active-eq-presets";

function readPresetMap(): Record<string, PresetSelection> {
  const raw = localStorage.getItem(ACTIVE_PRESET_KEY) ?? localStorage.getItem(LEGACY_ACTIVE_PRESET_KEY);
  return JSON.parse(raw ?? "{}") as Record<string, PresetSelection>;
}

function readPresetSelection(sinkName: string): PresetSelection | null {
  try {
    const all = readPresetMap();
    const selected = all[sinkName];
    return selected && (selected.source === "bundled" || selected.source === "user") && selected.name
      ? selected
      : null;
  } catch {
    return null;
  }
}

function writePresetSelection(sinkName: string, selected: PresetSelection | null) {
  try {
    const all = readPresetMap();
    if (selected) all[sinkName] = selected;
    else delete all[sinkName];
    localStorage.setItem(ACTIVE_PRESET_KEY, JSON.stringify(all));
  } catch {
    // A preset still applies when web storage is unavailable; only its label
    // cannot be remembered between views in that unusual case.
  }
}

interface EqPresetMenuProps {
  sinkName: string;
  config: EqConfig;
  onApply: (config: EqConfig) => void;
  onError: (message: string) => void;
  compact?: boolean;
}

/** Preset picker + import/export. Bundled EQ starting points ship inside the
 * binary; user presets are stored in a library scoped to this channel. */
export function EqPresetMenu({ sinkName, config, onApply, onError, compact = false }: Readonly<EqPresetMenuProps>) {
  const [menuOpen, setMenuOpen] = useState(false);
  const [presets, setPresets] = useState<EqPresetEntry[]>([]);
  const [saveName, setSaveName] = useState("");
  const [importing, setImporting] = useState(false);
  const [importText, setImportText] = useState("");
  // Name of the user preset awaiting a delete confirmation, if any.
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  const [selectedPreset, setSelectedPreset] = useState<PresetSelection | null>(
    () => readPresetSelection(sinkName),
  );

  useEffect(() => {
    setSelectedPreset(readPresetSelection(sinkName));
  }, [sinkName]);

  const refresh = () => {
    invoke<EqPresetEntry[]>("list_eq_presets", { sinkName })
      .then(setPresets)
      .catch((e) => onError(String(e)));
  };

  // Fetch on mount (so the button can name the active preset right away) and
  // again whenever the menu opens (to pick up newly saved user presets).
  useEffect(() => {
    refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [menuOpen, sinkName]);

  const applyPreset = (entry: EqPresetEntry) => {
    setMenuOpen(false);
    const selected = { source: entry.source, name: entry.preset.name };
    setSelectedPreset(selected);
    writePresetSelection(sinkName, selected);
    // A channel preset carries the complete EQ and playback-processing state.
    // Old presets predate the flag and retain their historical enable-on-apply
    // behaviour.
    onApply({
      enabled: entry.preset.enabled ?? true,
      preamp_db: entry.preset.preamp_db,
      bands: entry.preset.bands.map((b) => ({ ...b })),
      tone_bass_db: entry.preset.tone_bass_db ?? 0,
      tone_voice_db: entry.preset.tone_voice_db ?? 0,
      tone_treble_db: entry.preset.tone_treble_db ?? 0,
      boost_db: entry.preset.boost_db,
      gate_enabled: entry.preset.gate_enabled,
      gate_threshold_db: entry.preset.gate_threshold_db,
      comp_enabled: entry.preset.comp_enabled,
      comp_threshold_db: entry.preset.comp_threshold_db,
      comp_ratio: entry.preset.comp_ratio,
      limiter_enabled: entry.preset.limiter_enabled,
      limiter_ceiling_db: entry.preset.limiter_ceiling_db,
      playback_mode: entry.preset.playback_mode,
      spatial_enabled: entry.preset.spatial_enabled,
      spatial_tuning: entry.preset.spatial_tuning,
      spatial_distance: entry.preset.spatial_distance,
    });
  };

  const saveCurrent = async () => {
    const name = saveName.trim();
    if (!name) return;
    try {
      await invoke("save_user_eq_preset", { sinkName, name, config });
      const selected = { source: "user" as const, name };
      setSelectedPreset(selected);
      writePresetSelection(sinkName, selected);
      setSaveName("");
      refresh();
    } catch (e) {
      onError(String(e));
    }
  };

  const applyImported = (imported: EqConfig) => {
    // Imports parse to a disabled config (preview semantics); keep the
    // channel's current on/off state so importing never surprises.
    onApply({ ...imported, enabled: config.enabled });
    setSelectedPreset(null);
    writePresetSelection(sinkName, null);
    setImporting(false);
    setImportText("");
    setMenuOpen(false);
  };

  const importPasted = async () => {
    try {
      applyImported(await invoke<EqConfig>("import_eq_config", { text: importText }));
    } catch (e) {
      onError(String(e));
    }
  };

  const importFromFile = async () => {
    try {
      const imported = await invoke<EqConfig | null>("import_eq_file");
      if (imported) applyImported(imported);
    } catch (e) {
      onError(String(e));
    }
  };

  const exportToFile = async () => {
    try {
      if (await invoke<boolean>("export_channel_eq_to_file", { sinkName })) {
        setMenuOpen(false);
      }
    } catch (e) {
      onError(String(e));
    }
  };

  const deletePreset = async (name: string) => {
    try {
      await invoke("delete_user_eq_preset", { sinkName, name });
      if (selectedPreset?.source === "user" && selectedPreset.name === name) {
        setSelectedPreset(null);
        writePresetSelection(sinkName, null);
      }
      setConfirmDelete(null);
      refresh();
    } catch (e) {
      onError(String(e));
    }
  };

  const bundled = presets.filter((p) => p.source === "bundled");
  const user = presets.filter((p) => p.source === "user");

  // The button names whichever preset the current curve matches exactly; any
  // manual edit breaks the match and it falls back to the generic label.
  // Both sides come through the same f32 pipeline, so equality is safe.
  const sameBands = (a: EqConfig["bands"], b: EqConfig["bands"]) =>
    a.length === b.length &&
    a.every(
      (x, i) =>
        x.kind === b[i].kind &&
        x.freq_hz === b[i].freq_hz &&
        x.gain_db === b[i].gain_db &&
        x.q === b[i].q,
    );
  const activePreset = presets.find((e) => {
    const p = e.preset;
    return (
      (p.enabled ?? true) === config.enabled &&
      p.preamp_db === config.preamp_db &&
      sameBands(config.bands, p.bands) &&
      (p.tone_bass_db ?? 0) === config.tone_bass_db &&
      (p.tone_voice_db ?? 0) === config.tone_voice_db &&
      (p.tone_treble_db ?? 0) === config.tone_treble_db &&
      p.boost_db === config.boost_db &&
      p.gate_enabled === config.gate_enabled &&
      p.gate_threshold_db === config.gate_threshold_db &&
      p.comp_enabled === config.comp_enabled &&
      p.comp_threshold_db === config.comp_threshold_db &&
      p.comp_ratio === config.comp_ratio &&
      p.limiter_enabled === config.limiter_enabled &&
      p.limiter_ceiling_db === config.limiter_ceiling_db &&
      p.playback_mode === config.playback_mode &&
      p.spatial_enabled === config.spatial_enabled &&
      p.spatial_tuning === config.spatial_tuning &&
      p.spatial_distance === config.spatial_distance
    );
  });
  const rememberedPreset = selectedPreset
    ? presets.find((entry) =>
        entry.source === selectedPreset.source && entry.preset.name === selectedPreset.name)
    : undefined;
  const displayedPreset = rememberedPreset ?? activePreset;

  const presetRow = (entry: EqPresetEntry) => (
    <div key={`${entry.source}:${entry.preset.name}`} className="eqm-preset-row">
      <button
        type="button"
        className={"menu-item eqm-preset-apply" + (entry === displayedPreset ? " sel" : "")}
        title={entry.preset.description ?? undefined}
        onClick={() => applyPreset(entry)}
      >
        <Ms name="graphic_eq" />
        <span className="eqm-preset-name">{entry.preset.name}</span>
      </button>
      {entry.source === "user" &&
        (confirmDelete === entry.preset.name ? (
          <span className="eqm-preset-confirm">
            <button
              type="button"
              className="eqm-remove danger"
              title="Delete this preset"
              aria-label={`Confirm delete ${entry.preset.name}`}
              onClick={() => void deletePreset(entry.preset.name)}
            >
              <Ms name="check" style={{ fontSize: 14 }} />
            </button>
            <button
              type="button"
              className="eqm-remove"
              title="Keep it"
              aria-label="Cancel delete"
              onClick={() => setConfirmDelete(null)}
            >
              <Ms name="close" style={{ fontSize: 13 }} />
            </button>
          </span>
        ) : (
          <button
            type="button"
            className="eqm-remove"
            title="Delete preset"
            aria-label={`Delete preset ${entry.preset.name}`}
            onClick={() => setConfirmDelete(entry.preset.name)}
          >
            <Ms name="close" style={{ fontSize: 13 }} />
          </button>
        ))}
    </div>
  );

  return (
    <div className={compact ? "strip-preset-anchor" : undefined} style={{ position: "relative" }}>
      <button
        type="button"
        className={compact ? "strip-preset-button" : "select"}
        onClick={() => setMenuOpen((o) => !o)}
        title={displayedPreset ? `Preset: ${displayedPreset.preset.name}` : undefined}
      >
        <Ms name="library_music" style={{ fontSize: 15 }} />
        <span className="eqm-preset-btn-label">
          {displayedPreset ? displayedPreset.preset.name : compact ? "Custom" : "Channel preset"}
        </span>
        <Ms name="expand_more" />
      </button>
      <Popover
        open={menuOpen}
        onClose={() => {
          setMenuOpen(false);
          setImporting(false);
          setConfirmDelete(null);
        }}
        side="bottom"
        align={compact ? "center" : "end"}
        style={{ minWidth: 260 }}
      >
        {bundled.length > 0 && (
          <>
            <div className="eqm-preset-head">Bundled EQ presets</div>
            {bundled.map(presetRow)}
          </>
        )}
        {user.length > 0 && (
          <>
            <div className="eqm-preset-head">This channel</div>
            {user.map(presetRow)}
          </>
        )}

        <div className="menu-sep" />
        <div className={"eqm-save-label" + (displayedPreset ? "" : " custom")}>
          {displayedPreset ? "Save a copy of this channel" : "Save EQ and all channel processing"}
        </div>
        <div className="eqm-save-row">
          <input
            className="menu-input"
            placeholder="Preset name…"
            value={saveName}
            maxLength={64}
            onChange={(e) => setSaveName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void saveCurrent();
            }}
          />
          <button
            type="button"
            className="select"
            disabled={!saveName.trim()}
            title="Save current EQ and processing as a channel preset"
            onClick={() => void saveCurrent()}
          >
            <Ms name="save" style={{ fontSize: 15 }} />
          </button>
        </div>

        <div className="menu-sep" />
        <div className="eqm-io-row">
          <button
            type="button"
            className={"select eqm-io-btn" + (importing ? " on" : "")}
            aria-expanded={importing}
            title="Import a preset (paste JSON / AutoEq, or a file)"
            onClick={() => setImporting((v) => !v)}
          >
            <Ms name="content_paste" style={{ fontSize: 15 }} />
            <span>Import</span>
          </button>
          <button
            type="button"
            className="select eqm-io-btn"
            title="Export this channel preset to a JSON file"
            onClick={() => void exportToFile()}
          >
            <Ms name="download" style={{ fontSize: 15 }} />
            <span>Export</span>
          </button>
        </div>
        {importing && (
          <div className="eqm-import">
            <textarea
              className="eqm-import-text"
              placeholder={"Paste preset JSON or an AutoEq block:\nPreamp: -6.0 dB\nFilter 1: ON PK Fc 105 Hz Gain -2.4 dB Q 0.70"}
              value={importText}
              autoFocus
              onChange={(e) => setImportText(e.target.value)}
            />
            <div className="eqm-import-btns">
              <button
                type="button"
                className="select"
                disabled={!importText.trim()}
                onClick={() => void importPasted()}
              >
                Apply pasted
              </button>
              <button type="button" className="select" onClick={() => void importFromFile()}>
                From file…
              </button>
            </div>
          </div>
        )}
      </Popover>
    </div>
  );
}
