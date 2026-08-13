import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { useMixerStore } from "../../store/mixer";
import {
  DEFAULT_SHORTCUTS,
  shortcutFromKeyboardEvent,
  useShortcutSettings,
  type ShortcutAction,
} from "../../store/shortcuts";
import { useTheme, THEMES } from "../../store/theme";
import { restartApplication } from "../../hooks/useGlobalShortcuts";
import { stashRestoreWarning } from "../../lib/restoreWarning";
import type { MeterMode, OutputDevice, ProfileAutomationConfig } from "../../types";
import { Ms } from "../Icons";
import { ConfirmModal } from "../ConfirmModal";
import { MenuItem } from "../MenuItem";
import { Modal } from "../Modal";
import { Popover } from "../Popover";
import { ProcessingInfo } from "../ProcessingInfo";
import { Toggle } from "../Toggle";

interface DefaultDevices {
  output: string | null;
  input: string | null;
}

interface BackupStatus {
  count: number;
  last_backup_at: number | null;
}

interface RestoreBackupResult {
  frontend_state: Record<string, string>;
  recovery_backup: string;
  warning: string | null;
}

const BACKUP_FRONTEND_KEYS = [
  "sonux-theme",
  "sonux-global-shortcuts",
  "sonux-active-eq-presets",
  "sonux-profile-section-visibility",
] as const;

function frontendBackupState(): Record<string, string> {
  return Object.fromEntries(BACKUP_FRONTEND_KEYS.flatMap((key) => {
    const value = localStorage.getItem(key);
    return value === null ? [] : [[key, value]];
  }));
}

function applyFrontendBackupState(state: Record<string, string>) {
  for (const key of BACKUP_FRONTEND_KEYS) {
    localStorage.removeItem(key);
  }
  for (const [key, value] of Object.entries(state)) {
    if ((BACKUP_FRONTEND_KEYS as readonly string[]).includes(key)) {
      localStorage.setItem(key, value);
    }
  }
}

function backupStatusText(status: BackupStatus | null): string {
  if (!status) return "Checking backups…";
  if (status.count === 0 || status.last_backup_at === null) return "No backups yet";
  const count = `${status.count} ${status.count === 1 ? "backup" : "backups"}`;
  const created = new Date(status.last_backup_at * 1000);
  const date = new Intl.DateTimeFormat("en-GB", {
    day: "numeric",
    month: "short",
    year: "numeric",
  }).format(created);
  const time = new Intl.DateTimeFormat("en-GB", {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  }).format(created);
  return `${count} · Last backup: ${date} at ${time}`;
}

function fileName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

type LabelStyle = "plain" | "suffix" | "prefix";

const LABEL_STYLES: { value: LabelStyle; label: string; example: string }[] = [
  { value: "plain", label: "Plain", example: "Game" },
  { value: "suffix", label: "Suffix", example: "Game (Sonux)" },
  { value: "prefix", label: "Prefix", example: "Sonux · Game" },
];

const METER_MODES: { value: MeterMode; label: string; detail: string }[] = [
  { value: "monitor", label: "Monitor refresh rate", detail: "Match the display for the smoothest motion" },
  { value: "fps_144", label: "144 FPS", detail: "Cap live meter animation at 144 FPS" },
  { value: "fps_120", label: "120 FPS", detail: "Cap live meter animation at 120 FPS" },
  { value: "fps_100", label: "100 FPS", detail: "Cap live meter animation at 100 FPS" },
  { value: "fps_60", label: "60 FPS", detail: "Cap live meter animation at 60 FPS" },
  { value: "off", label: "Off", detail: "Disable live meter visuals" },
];

const SHORTCUT_ROWS: { action: ShortcutAction; label: string; icon: string }[] = [
  { action: "toggle_game", label: "Mute Game", icon: "sports_esports" },
  { action: "toggle_chat", label: "Mute Chat", icon: "forum" },
  { action: "toggle_mic", label: "Mute microphone", icon: "mic" },
  { action: "restart_app", label: "Restart application", icon: "restart_alt" },
];

/** Card row with a device dropdown for picking a system default. */
function DeviceRow({
  icon,
  title,
  sub,
  devices,
  current,
  onPick,
}: Readonly<{
  icon: string;
  title: string;
  /** What this default is used for. */
  sub: string;
  devices: OutputDevice[];
  current: string | null;
  onPick: (name: string) => void;
}>) {
  const [open, setOpen] = useState(false);
  const currentDesc = devices.find((d) => d.name === current)?.description ?? current ?? "-";

  return (
    <div className="row">
      <div className="ricon">
        <Ms name={icon} />
      </div>
      <div className="rmain">
        <div className="rtitle">{title}</div>
        <div className="rsub">{sub}</div>
      </div>
      <div style={{ position: "relative" }}>
        <button type="button" className="select device-select" onClick={() => setOpen((o) => !o)}>
          <span className="device-select-name">{currentDesc}</span>
          <Ms name="expand_more" />
        </button>
        <Popover open={open} onClose={() => setOpen(false)} side="bottom" align="end">
          {devices.map((d) => (
            <MenuItem
              key={d.name}
              icon={icon}
              selected={d.name === current}
              showCheck
              onClick={() => {
                onPick(d.name);
                setOpen(false);
              }}
            >
              {d.description}
            </MenuItem>
          ))}
        </Popover>
      </div>
    </div>
  );
}

function engineDesc(native: boolean | null): string {
  if (native === null) return "…";
  return native
    ? "Native PipeWire (pipewire-rs) - live metering, passive routing"
    : "pactl fallback - native engine unavailable on this system";
}

export function SettingsScreen() {
  const { theme, setTheme } = useTheme();
  const [autostart, setAutostart] = useState<boolean | null>(null);
  const [startMinimized, setStartMinimized] = useState(false);
  const [backendNative, setBackendNative] = useState<boolean | null>(null);
  const [version, setVersion] = useState("");
  const [defaults, setDefaults] = useState<DefaultDevices>({ output: null, input: null });
  const [labelStyle, setLabelStyle] = useState<LabelStyle>("plain");
  const [labelStyleOpen, setLabelStyleOpen] = useState(false);
  const [meterModeOpen, setMeterModeOpen] = useState(false);
  const [profileAutomation, setProfileAutomation] = useState<ProfileAutomationConfig | null>(null);
  const [confirmingReset, setConfirmingReset] = useState(false);
  const [confirmingMultipleMics, setConfirmingMultipleMics] = useState(false);
  const [backupStatus, setBackupStatus] = useState<BackupStatus | null>(null);
  const [backupBusy, setBackupBusy] = useState<"create" | "restore" | "open" | null>(null);
  const [restorePath, setRestorePath] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const outputDevices = useMixerStore((s) => s.outputDevices);
  const inputDevices = useMixerStore((s) => s.inputDevices);
  const profiles = useMixerStore((s) => s.profiles);
  const replayOnboarding = useMixerStore((s) => s.replayOnboarding);
  const showBalance = useMixerStore((s) => s.showBalance);
  const setBalanceVisible = useMixerStore((s) => s.setBalanceVisible);
  const multipleMics = useMixerStore((s) => s.multipleMics);
  const setMultipleMics = useMixerStore((s) => s.setMultipleMics);
  const meterMode = useMixerStore((s) => s.meterMode);
  const setMeterMode = useMixerStore((s) => s.setMeterMode);
  const shortcutsEnabled = useShortcutSettings((s) => s.enabled);
  const shortcutBindings = useShortcutSettings((s) => s.bindings);
  const setShortcutsEnabled = useShortcutSettings((s) => s.setEnabled);
  const setShortcutBindings = useShortcutSettings((s) => s.setBindings);
  const [recordingShortcut, setRecordingShortcut] = useState<ShortcutAction | null>(null);

  useEffect(() => {
    void invoke<boolean>("get_autostart").then(setAutostart);
    void invoke<{ native: boolean }>("get_backend_info").then((i) => setBackendNative(i.native));
    void invoke<DefaultDevices>("get_default_devices").then(setDefaults).catch(() => {});
    void invoke<ProfileAutomationConfig>("get_profile_automation").then(setProfileAutomation).catch(() => {});
    void invoke<BackupStatus>("get_backup_status").then(setBackupStatus).catch(() => {});
    void invoke<{ device_label_style: LabelStyle; start_minimized: boolean }>("get_prefs")
      .then((p) => {
        setLabelStyle(p.device_label_style);
        setStartMinimized(p.start_minimized);
      })
      .catch(() => {});
    void getVersion().then(setVersion);
  }, []);

  const pickDefault = async (kind: "output" | "input", name: string) => {
    try {
      await invoke(kind === "output" ? "set_default_output" : "set_default_input", { name });
      setDefaults((d) => ({ ...d, [kind]: name }));
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  };

  const pickLabelStyle = async (style: LabelStyle) => {
    try {
      await invoke("set_device_label_style", { style });
      setLabelStyle(style);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  };

  const toggleAutostart = async () => {
    if (autostart === null) return;
    try {
      const actual = await invoke<boolean>("set_autostart", { enabled: !autostart });
      setAutostart(actual);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  };

  const toggleStartMinimized = async () => {
    const next = !startMinimized;
    setStartMinimized(next);
    try {
      await invoke("set_start_minimized", { minimized: next });
      setError(null);
    } catch (e) {
      setStartMinimized(!next);
      setError(String(e));
    }
  };

  const saveProfileAutomation = async (next: ProfileAutomationConfig) => {
    try {
      await invoke("save_profile_automation", { config: next });
      setProfileAutomation(next);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  };

  const createBackup = async () => {
    setBackupBusy("create");
    try {
      const status = await invoke<BackupStatus>("create_backup", {
        frontendState: frontendBackupState(),
      });
      setBackupStatus(status);
      setError(null);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBackupBusy(null);
    }
  };

  const chooseBackup = async () => {
    try {
      const selected = await invoke<string | null>("choose_backup_for_restore");
      if (selected) setRestorePath(selected);
    } catch (reason) {
      setError(String(reason));
    }
  };

  const restoreBackup = async () => {
    setBackupBusy("restore");
    try {
      const restored = await invoke<RestoreBackupResult>("restore_backup", {
        frontendState: frontendBackupState(),
      });
      applyFrontendBackupState(restored.frontend_state);
      // A peripheral restore warning must not leave the old in-memory mixer
      // running against the newly replaced config tree. Carry the warning
      // across the mandatory restart and show it in the fresh process.
      stashRestoreWarning(restored.warning);
      await invoke("restart_app");
    } catch (reason) {
      setError(String(reason));
      setBackupBusy(null);
      void invoke<BackupStatus>("get_backup_status").then(setBackupStatus).catch(() => {});
    }
  };

  const openBackupLocation = async () => {
    setBackupBusy("open");
    try {
      await invoke("open_backup_location");
      setError(null);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBackupBusy(null);
    }
  };

  return (
    <div className="content narrow">
      <div className="screen-head">
        <h1>Settings</h1>
      </div>
      <div className="screen-scroll">
        {error && <div className="error-banner" style={{ borderRadius: 8 }}>{error}</div>}

        <div className="section-label">Appearance</div>
        <div className="card" style={{ padding: "var(--sp-2)" }}>
          <div className="row">
            <div className="ricon">
              <Ms name="palette" />
            </div>
            <div className="rmain">
              <div className="rtitle">Theme</div>
              <div className="rsub">Original, or Tokyo Night to match your desktop</div>
            </div>
            <div className="theme-picker">
              {THEMES.map((t) => (
                <button
                  key={t.id}
                  type="button"
                  className={"theme-swatch" + (t.id === theme ? " active" : "")}
                  onClick={() => setTheme(t.id)}
                  title={t.label}
                >
                  <span className="theme-swatch-colors">
                    {t.swatch.map((c) => (
                      <i key={c} style={{ background: c }} />
                    ))}
                  </span>
                  <span className="theme-swatch-label">{t.label}</span>
                </button>
              ))}
            </div>
          </div>
          <div className="row">
            <div className="ricon">
              <Ms name="speed" />
            </div>
            <div className="rmain">
              <div className="rtitle">Live meter refresh rate</div>
              <div className="rsub">
                Mixer tab only, while it is open. {METER_MODES.find((option) => option.value === meterMode)?.detail}.
                {meterMode !== "off" && " Higher refresh rates use more CPU."}
              </div>
            </div>
            <div style={{ position: "relative" }}>
              <button type="button" className="select" onClick={() => setMeterModeOpen((open) => !open)}>
                <span>{METER_MODES.find((option) => option.value === meterMode)?.label}</span>
                <Ms name="expand_more" />
              </button>
              <Popover open={meterModeOpen} onClose={() => setMeterModeOpen(false)} side="bottom" align="end">
                {METER_MODES.map((option) => (
                  <MenuItem
                    key={option.value}
                    selected={option.value === meterMode}
                    showCheck
                    onClick={() => {
                      void setMeterMode(option.value);
                      setMeterModeOpen(false);
                    }}
                  >
                    {option.label}
                  </MenuItem>
                ))}
              </Popover>
            </div>
          </div>
        </div>

        <div className="section-label">Preferences</div>
        <div className="card" style={{ padding: "var(--sp-2)" }}>
          <div className="row">
            <div className="ricon">
              <Ms name="label" />
            </div>
            <div className="rmain">
              <div className="rtitle">Device naming</div>
              <div className="rsub">Naming scheme for Sonux-managed devices</div>
            </div>
            <div style={{ position: "relative" }}>
              <button type="button" className="select" onClick={() => setLabelStyleOpen((o) => !o)}>
                <span>{LABEL_STYLES.find((s) => s.value === labelStyle)?.label}</span>
                <Ms name="expand_more" />
              </button>
              <Popover open={labelStyleOpen} onClose={() => setLabelStyleOpen(false)} side="bottom" align="end">
                {LABEL_STYLES.map((s) => (
                  <MenuItem
                    key={s.value}
                    selected={s.value === labelStyle}
                    showCheck
                    onClick={() => {
                      void pickLabelStyle(s.value);
                      setLabelStyleOpen(false);
                    }}
                  >
                    {s.example}
                  </MenuItem>
                ))}
              </Popover>
            </div>
          </div>
          <DeviceRow
            icon="speaker"
            title="Default output"
            sub="Where channels set to “System default” play"
            devices={outputDevices}
            current={defaults.output}
            onPick={(name) => void pickDefault("output", name)}
          />
          <DeviceRow
            icon="mic"
            title="Default input"
            sub="System microphone used when a profile follows the default input"
            devices={inputDevices}
            current={defaults.input}
            onPick={(name) => void pickDefault("input", name)}
          />
          <div className="row">
            <div className="ricon">
              <Ms name="mic_external_on" />
            </div>
            <div className="rmain">
              <div className="rtitle">Multiple microphone channels</div>
              <div className="rsub">Advanced · publish separate processed microphones for applications</div>
            </div>
            <Toggle
              on={multipleMics}
              onClick={() => {
                if (multipleMics) void setMultipleMics(false);
                else setConfirmingMultipleMics(true);
              }}
            />
          </div>
          {multipleMics && (
            <div className="mic-tip settings-mic-tip">
              <Ms name="warning" />
              <span>
                One virtual microphone is recommended for most setups. Add another only when an application or production workflow needs an independently processed input.
              </span>
            </div>
          )}
          <div className="row">
            <div className="ricon">
              <Ms name="balance" />
            </div>
            <div className="rmain">
              <div className="rtitle">Balance slider</div>
              <div className="rsub">ChatMix-style blend of two channels in the workspace bar</div>
            </div>
            <Toggle on={showBalance} onClick={() => void setBalanceVisible(!showBalance)} />
          </div>
          <div className="row">
            <div className="ricon">
              <Ms name="rocket_launch" />
            </div>
            <div className="rmain">
              <div className="rtitle">Start at login</div>
              <div className="rsub">systemd user service, starts with your desktop session</div>
            </div>
            {autostart !== null && <Toggle on={autostart} onClick={() => void toggleAutostart()} />}
          </div>
          {autostart && (
            <div className="row row-sub">
              <div className="ricon">
                <Ms name="dock_to_bottom" />
              </div>
              <div className="rmain">
                <div className="rtitle">Start minimized</div>
                <div className="rsub">Boot to the tray instead of opening the window</div>
              </div>
              <Toggle
                on={startMinimized}
                onClick={() => void toggleStartMinimized()}
              />
            </div>
          )}
        </div>

        <div className="section-label">Automatic profile activation</div>
        <div className="card" style={{ padding: "var(--sp-2)" }}>
          <div className="row">
            <div className="ricon">
              <Ms name="automation" />
            </div>
            <div className="rmain">
              <div className="rtitle">Enable automatic activation</div>
              <div className="rsub">Activate profiles when their linked games or applications start</div>
            </div>
            {profileAutomation && (
              <div className="processing-card-head-actions">
                <Toggle
                  on={profileAutomation.enabled}
                  onClick={() => void saveProfileAutomation({ ...profileAutomation, enabled: !profileAutomation.enabled })}
                />
                <ProcessingInfo
                  label="How profile switching works"
                  text="When a linked application starts, Sonux activates its profile. If several linked applications run, the newest one takes priority. Selecting a profile manually overrides automation until the matched application closes."
                />
              </div>
            )}
          </div>
          <div className={`row automation-dependent-setting${profileAutomation?.enabled ? "" : " disabled"}`}>
            <div className="ricon">
              <Ms name="restore" />
            </div>
            <div className="rmain">
              <div className="rtitle">After linked applications close</div>
              <div className="rsub">Choose which profile Sonux should use next</div>
            </div>
            {profileAutomation && (
              <select
                className="select automation-settings-select"
                disabled={!profileAutomation.enabled}
                value={profileAutomation.return_profile ?? ""}
                onChange={(event) => void saveProfileAutomation({
                  ...profileAutomation,
                  return_profile: event.target.value || null,
                })}
              >
                <option value="">Restore the previous profile</option>
                {profiles.map((profile) => (
                  <option key={profile.name} value={profile.name}>Return to {profile.name}</option>
                ))}
              </select>
            )}
          </div>
          <div className={`row automation-dependent-setting${profileAutomation?.enabled ? "" : " disabled"}`}>
            <div className="ricon">
              <Ms name="notifications" />
            </div>
            <div className="rmain">
              <div className="rtitle">Profile switch notifications</div>
              <div className="rsub">Show a desktop notification when Sonux changes profiles automatically</div>
            </div>
            {profileAutomation && (
              <Toggle
                on={profileAutomation.notifications}
                disabled={!profileAutomation.enabled}
                onClick={() => void saveProfileAutomation({
                  ...profileAutomation,
                  notifications: !profileAutomation.notifications,
                })}
              />
            )}
          </div>
        </div>

        <div className="section-label">Global shortcuts</div>
        <div className="card" style={{ padding: "var(--sp-2)" }}>
          <div className="row">
            <div className="ricon">
              <Ms name="keyboard" />
            </div>
            <div className="rmain">
              <div className="rtitle">Enable shortcuts</div>
              <div className="rsub">Control audio while games and other apps are focused</div>
            </div>
            <div className="processing-card-head-actions">
              <Toggle on={shortcutsEnabled} onClick={() => setShortcutsEnabled(!shortcutsEnabled)} />
              <ProcessingInfo
                label="Recording shortcuts"
                text={'Click a binding, then press the keyboard shortcut you want. The change is saved immediately.\n\nPress Backspace or Delete to clear one binding, or Escape to cancel.'}
              />
            </div>
          </div>
          {SHORTCUT_ROWS.map(({ action, label, icon }) => (
            <label className="row shortcut-row" key={action}>
              <div className="ricon">
                <Ms name={icon} />
              </div>
              <div className="rmain">
                <div className="rtitle">{label}</div>
              </div>
              <input
                className={`shortcut-input${recordingShortcut === action ? " recording" : ""}`}
                value={recordingShortcut === action ? "Press a key to bind" : shortcutBindings[action]}
                placeholder="Click and press keys"
                spellCheck={false}
                readOnly
                title="Click, then press a keyboard shortcut. Backspace or Delete clears it; Escape cancels."
                aria-label={recordingShortcut === action ? `${label}: press a key to bind` : `${label} shortcut`}
                onFocus={() => setRecordingShortcut(action)}
                onBlur={() => setRecordingShortcut((current) => current === action ? null : current)}
                onKeyDown={(event) => {
                  event.preventDefault();
                  if (!event.altKey && !event.ctrlKey && !event.metaKey && !event.shiftKey && event.key === "Escape") {
                    event.currentTarget.blur();
                    return;
                  }
                  const shortcut = shortcutFromKeyboardEvent(event);
                  if (shortcut === null) return;
                  setShortcutBindings({ ...shortcutBindings, [action]: shortcut });
                  event.currentTarget.blur();
                }}
              />
            </label>
          ))}
          <div className="shortcut-footer">
            <button
              type="button"
              className="select"
              disabled={JSON.stringify(shortcutBindings) === JSON.stringify(DEFAULT_SHORTCUTS)}
              onClick={() => setShortcutBindings({ ...DEFAULT_SHORTCUTS })}
            >
              Restore defaults
            </button>
          </div>
        </div>

        <div className="section-label">Backups</div>
        <div className="card" style={{ padding: "var(--sp-2)" }}>
          <div className="row">
            <div className="ricon">
              <Ms name="backup" />
            </div>
            <div className="rmain">
              <div className="rtitle">Manual backups</div>
              <div className="rsub">{backupStatusText(backupStatus)}</div>
            </div>
            <button
              type="button"
              className="select"
              disabled={backupBusy !== null}
              onClick={() => void createBackup()}
            >
              <span>{backupBusy === "create" ? "Creating…" : "Create backup"}</span>
            </button>
          </div>
          <div className="row">
            <div className="ricon">
              <Ms name="folder_open" />
            </div>
            <div className="rmain">
              <div className="rtitle">Backup location</div>
              <div className="rsub">Backups remain there until you delete them</div>
            </div>
            <button
              type="button"
              className="select"
              disabled={backupBusy !== null}
              onClick={() => void openBackupLocation()}
            >
              <span>Open backup location</span>
            </button>
          </div>
          <div className="row">
            <div className="ricon">
              <Ms name="restore_page" />
            </div>
            <div className="rmain">
              <div className="rtitle">Restore backup</div>
              <div className="rsub">Creates an automatic recovery backup first</div>
            </div>
            <button
              type="button"
              className="select"
              disabled={backupBusy !== null}
              onClick={() => void chooseBackup()}
            >
              <span>Restore backup…</span>
            </button>
          </div>
        </div>

        <div className="section-label">About</div>
        <div className="card" style={{ padding: "var(--sp-2)" }}>
          <div className="row">
            <div className="ricon">
              <Ms name="cable" />
            </div>
            <div className="rmain">
              <div className="rtitle">Audio engine</div>
              <div className="rsub">
                {engineDesc(backendNative)}
              </div>
            </div>
            {backendNative !== null && (
              <span className={"tag" + (backendNative ? " live" : "")}>
                {backendNative ? "native" : "fallback"}
              </span>
            )}
          </div>
          <div className="row">
            <div className="ricon">
              <Ms name="info" />
            </div>
            <div className="rmain">
              <div className="rtitle">Sonux {version}</div>
              <div className="rsub">Customized from Sink · GPL-3.0 · config in ~/.config/sonux</div>
            </div>
          </div>
          <div className="row">
            <div className="ricon">
              <Ms name="school" />
            </div>
            <div className="rmain">
              <div className="rtitle">Tutorial</div>
              <div className="rsub">Replay the first-run tour</div>
            </div>
            <button type="button" className="select" onClick={replayOnboarding}>
              <span>Replay</span>
            </button>
          </div>
          <div className="row">
            <div className="ricon">
              <Ms name="refresh" />
            </div>
            <div className="rmain">
              <div className="rtitle">Restart application</div>
              <div className="rsub">Reload the audio engine and interface without deleting settings</div>
            </div>
            <button type="button" className="select" onClick={restartApplication}>
              <span>Restart</span>
            </button>
          </div>
          <div className="row">
            <div className="ricon">
              <Ms name="restart_alt" />
            </div>
            <div className="rmain">
              <div className="rtitle">Reset Sonux</div>
              <div className="rsub">
                Erase all channels, mixes, profiles, app history and preferences
              </div>
            </div>
            <button type="button" className="select" onClick={() => setConfirmingReset(true)}>
              <span>Reset…</span>
            </button>
          </div>
        </div>
      </div>
      <Modal open={confirmingMultipleMics} onClose={() => setConfirmingMultipleMics(false)} title="Enable multiple microphones?">
        <p className="modal-text">
          Most users should publish only one virtual microphone. Multiple channels capture and process audio independently and can make device lists and application setup more complex.
        </p>
        <p className="modal-text">Enable this only when you need separate processed inputs, alternate processing, or production stems.</p>
        <div className="modal-btns">
          <button
            type="button"
            className="modal-btn primary"
            onClick={() => {
              setConfirmingMultipleMics(false);
              void setMultipleMics(true);
            }}
          >
            Enable advanced feature
          </button>
          <button type="button" className="modal-btn" onClick={() => setConfirmingMultipleMics(false)}>Cancel</button>
        </div>
      </Modal>

      <ConfirmModal
        open={restorePath !== null}
        onClose={() => setRestorePath(null)}
        onCancel={() => {
          void invoke("cancel_backup_restore").catch((reason) => setError(String(reason)));
        }}
        title={`Restore ${restorePath ? `“${fileName(restorePath)}”` : "backup"}?`}
        confirmLabel="Restore and restart"
        onConfirm={() => {
          if (restorePath) void restoreBackup();
        }}
      >
        Your current Sonux setup will be replaced. Before restoring, Sonux will save it as a clearly labelled Automatic Recovery Backup, then restart.
      </ConfirmModal>

      <ConfirmModal
        open={confirmingReset}
        onClose={() => setConfirmingReset(false)}
        title="Reset Sonux?"
        confirmLabel="Reset everything"
        onConfirm={() => void invoke("reset_app").catch((e) => setError(String(e)))}
      >
        Everything you've set up - channels, mixes, profiles, app assignments,
        history and preferences - is permanently deleted, and Sonux relaunches
        as if freshly installed.
      </ConfirmModal>
    </div>
  );
}
