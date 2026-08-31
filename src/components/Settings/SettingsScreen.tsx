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
import { useI18n, type TranslationKey } from "../../i18n";
import { reloadLanguagePacks, type LanguagePackCatalog } from "../../languagePacks";
import type { MeterMode, OutputDevice, ProfileAutomationConfig } from "../../types";
import { Ms } from "../Icons";
import { ConfirmModal } from "../ConfirmModal";
import { HelpInfo } from "../HelpInfo";
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
  "sonux-language",
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

function backupStatusText(status: BackupStatus | null, locale: string, t: ReturnType<typeof useI18n>["t"]): string {
  if (!status) return t("settings.backups.checking");
  if (status.count === 0 || status.last_backup_at === null) return t("settings.backups.none");
  const count = t(status.count === 1 ? "settings.backups.countOne" : "settings.backups.countMany", { count: status.count });
  const created = new Date(status.last_backup_at * 1000);
  const date = new Intl.DateTimeFormat(locale, {
    day: "numeric",
    month: "short",
    year: "numeric",
  }).format(created);
  const time = new Intl.DateTimeFormat(locale, {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  }).format(created);
  return t("settings.backups.last", { count, date, time });
}

function fileName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

type LabelStyle = "plain" | "suffix" | "prefix";

const LABEL_STYLES: { value: LabelStyle; label: TranslationKey; example: TranslationKey }[] = [
  { value: "plain", label: "settings.naming.plain", example: "settings.naming.plainExample" },
  { value: "suffix", label: "settings.naming.suffix", example: "settings.naming.suffixExample" },
  { value: "prefix", label: "settings.naming.prefix", example: "settings.naming.prefixExample" },
];

const METER_MODES: { value: MeterMode; label: TranslationKey; detail: TranslationKey }[] = [
  { value: "monitor", label: "settings.meters.monitor.label", detail: "settings.meters.monitor.detail" },
  { value: "fps_144", label: "settings.meters.fps144.label", detail: "settings.meters.fps144.detail" },
  { value: "fps_120", label: "settings.meters.fps120.label", detail: "settings.meters.fps120.detail" },
  { value: "fps_100", label: "settings.meters.fps100.label", detail: "settings.meters.fps100.detail" },
  { value: "fps_60", label: "settings.meters.fps60.label", detail: "settings.meters.fps60.detail" },
  { value: "off", label: "settings.meters.off.label", detail: "settings.meters.off.detail" },
];

const SHORTCUT_ROWS: { action: ShortcutAction; label: TranslationKey; icon: string }[] = [
  { action: "toggle_game", label: "settings.shortcuts.game", icon: "sports_esports" },
  { action: "toggle_chat", label: "settings.shortcuts.chat", icon: "forum" },
  { action: "toggle_mic", label: "settings.shortcuts.microphone", icon: "mic" },
  { action: "restart_app", label: "settings.shortcuts.restart", icon: "restart_alt" },
];

function SettingTitle({ title, description }: Readonly<{ title: string; description?: string }>) {
  return (
    <div className="rtitle">
      <span>{title}</span>
      {description && <HelpInfo label={title} text={description} />}
    </div>
  );
}

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
        <SettingTitle title={title} description={sub} />
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

export function SettingsScreen() {
  const { availableLocales, locale, preference, setPreference, t } = useI18n();
  const { theme, setTheme } = useTheme();
  const [autostart, setAutostart] = useState<boolean | null>(null);
  const [startMinimized, setStartMinimized] = useState(false);
  const [version, setVersion] = useState("");
  const [defaults, setDefaults] = useState<DefaultDevices>({ output: null, input: null });
  const [labelStyle, setLabelStyle] = useState<LabelStyle>("plain");
  const [labelStyleOpen, setLabelStyleOpen] = useState(false);
  const [languageOpen, setLanguageOpen] = useState(false);
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
  const [languageCatalog, setLanguageCatalog] = useState<LanguagePackCatalog | null>(null);

  useEffect(() => {
    void invoke<boolean>("get_autostart").then(setAutostart);
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
    void reloadLanguagePacks().then(setLanguageCatalog).catch((reason) => setError(String(reason)));
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

  const selectedLanguageValue = preference.mode === "system"
    ? "system"
    : availableLocales.some((candidate) => candidate.locale === preference.locale)
      ? preference.locale
      : locale !== "en" ? locale : preference.locale;

  return (
    <div className="content narrow">
      <div className="screen-head">
        <h1>{t("settings.title")}</h1>
      </div>
      <div className="screen-scroll">
        {error && <div className="error-banner" style={{ borderRadius: 8 }}>{error}</div>}

        <div className="section-label">{t("settings.appearance.section")}</div>
        <div className="card" style={{ padding: "var(--sp-2)" }}>
          <div className="row">
            <div className="ricon">
              <Ms name="palette" />
            </div>
            <div className="rmain">
              <SettingTitle title={t("settings.theme.title")} description={t("settings.theme.description")} />
            </div>
            <div className="theme-picker">
              {THEMES.map((themeOption) => (
                <button
                  key={themeOption.id}
                  type="button"
                  className={"theme-swatch" + (themeOption.id === theme ? " active" : "")}
                  onClick={() => setTheme(themeOption.id)}
                  title={t(themeOption.labelKey)}
                >
                  <span className="theme-swatch-colors">
                    {themeOption.swatch.map((c) => (
                      <i key={c} style={{ background: c }} />
                    ))}
                  </span>
                  <span className="theme-swatch-label">{t(themeOption.labelKey)}</span>
                </button>
              ))}
            </div>
          </div>
          <div className="row">
            <div className="ricon">
              <Ms name="translate" />
            </div>
            <div className="rmain">
              <div className="rtitle">
                <span id="language-setting-title">{t("settings.language.title")}</span>
                {languageCatalog && languageCatalog.warnings.length > 0 && (
                  <ProcessingInfo
                    label={t("settings.language.warningDetails")}
                    text={languageCatalog.warnings.join("\n\n")}
                  />
                )}
              </div>
            </div>
            <div style={{ position: "relative" }}>
              <button
                type="button"
                className="select automation-settings-select"
                aria-labelledby="language-setting-title language-current-value"
                aria-haspopup="menu"
                aria-expanded={languageOpen}
                aria-controls="language-menu"
                onClick={() => setLanguageOpen((open) => !open)}
                onKeyDown={(event) => {
                  if (event.key === "ArrowDown" || event.key === "ArrowUp") {
                    event.preventDefault();
                    setLanguageOpen(true);
                  }
                }}
              >
                <span id="language-current-value">{selectedLanguageValue === "system"
                  ? t("settings.language.systemResolved", {
                    language: availableLocales.find((candidate) => candidate.locale === locale)?.nativeName ?? "English",
                  })
                  : availableLocales.find((candidate) => candidate.locale === selectedLanguageValue)?.nativeName
                    ?? t("settings.language.unavailableOption", { locale: selectedLanguageValue })}</span>
                <Ms name="expand_more" />
              </button>
              <Popover id="language-menu" open={languageOpen} onClose={() => setLanguageOpen(false)} side="bottom" align="end" style={{ minWidth: 240 }}>
                <MenuItem
                  selected={preference.mode === "system"}
                  showCheck
                  onClick={() => {
                    setPreference({ mode: "system" });
                    setLanguageOpen(false);
                  }}
                >
                  {t("settings.language.systemResolved", {
                    language: availableLocales.find((candidate) => candidate.locale === locale)?.nativeName ?? "English",
                  })}
                </MenuItem>
                {availableLocales.map((candidate) => (
                  <MenuItem
                    key={candidate.locale}
                    selected={preference.mode === "locale" && selectedLanguageValue === candidate.locale}
                    showCheck
                    onClick={() => {
                      setPreference({ mode: "locale", locale: candidate.locale });
                      setLanguageOpen(false);
                    }}
                  >
                    {candidate.nativeName}
                  </MenuItem>
                ))}
              </Popover>
            </div>
          </div>
          <div className="row">
            <div className="ricon">
              <Ms name="speed" />
            </div>
            <div className="rmain">
              <SettingTitle
                title={t("settings.meters.title")}
                description={`${t("settings.meters.description", {
                  detail: t(METER_MODES.find((option) => option.value === meterMode)?.detail ?? "settings.meters.off.detail"),
                })}${meterMode !== "off" ? ` ${t("settings.meters.cpuHint")}` : ""}`}
              />
            </div>
            <div style={{ position: "relative" }}>
              <button type="button" className="select" onClick={() => setMeterModeOpen((open) => !open)}>
                <span>{t(METER_MODES.find((option) => option.value === meterMode)?.label ?? "settings.meters.off.label")}</span>
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
                    {t(option.label)}
                  </MenuItem>
                ))}
              </Popover>
            </div>
          </div>
        </div>

        <div className="section-label">{t("settings.startup.section")}</div>
        <div className="card" style={{ padding: "var(--sp-2)" }}>
          <div className="row">
            <div className="ricon">
              <Ms name="rocket_launch" />
            </div>
            <div className="rmain">
              <SettingTitle title={t("settings.autostart.title")} description={t("settings.autostart.description")} />
            </div>
            {autostart !== null && <Toggle on={autostart} onClick={() => void toggleAutostart()} />}
          </div>
          {autostart && (
            <div className="row row-sub">
              <div className="ricon">
                <Ms name="dock_to_bottom" />
              </div>
              <div className="rmain">
                <SettingTitle title={t("settings.autostart.minimized.title")} description={t("settings.autostart.minimized.description")} />
              </div>
              <Toggle
                on={startMinimized}
                onClick={() => void toggleStartMinimized()}
              />
            </div>
          )}
        </div>

        <div className="section-label">{t("settings.preferences.section")}</div>
        <div className="card" style={{ padding: "var(--sp-2)" }}>
          <div className="row">
            <div className="ricon">
              <Ms name="label" />
            </div>
            <div className="rmain">
              <SettingTitle title={t("settings.naming.title")} description={t("settings.naming.description")} />
            </div>
            <div style={{ position: "relative" }}>
              <button type="button" className="select" onClick={() => setLabelStyleOpen((o) => !o)}>
                <span>{t(LABEL_STYLES.find((s) => s.value === labelStyle)?.label ?? "settings.naming.plain")}</span>
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
                    {t(s.example)}
                  </MenuItem>
                ))}
              </Popover>
            </div>
          </div>
          <DeviceRow
            icon="speaker"
            title={t("settings.defaults.output.title")}
            sub={t("settings.defaults.output.description")}
            devices={outputDevices}
            current={defaults.output}
            onPick={(name) => void pickDefault("output", name)}
          />
          <DeviceRow
            icon="mic"
            title={t("settings.defaults.input.title")}
            sub={t("settings.defaults.input.description")}
            devices={inputDevices}
            current={defaults.input}
            onPick={(name) => void pickDefault("input", name)}
          />
          <div className="row">
            <div className="ricon">
              <Ms name="mic_external_on" />
            </div>
            <div className="rmain">
              <SettingTitle title={t("settings.microphones.multiple.title")} description={t("settings.microphones.multiple.description")} />
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
                {t("settings.microphones.multiple.tip")}
              </span>
            </div>
          )}
          <div className="row">
            <div className="ricon">
              <Ms name="balance" />
            </div>
            <div className="rmain">
              <SettingTitle title={t("settings.balance.title")} description={t("settings.balance.description")} />
            </div>
            <Toggle on={showBalance} onClick={() => void setBalanceVisible(!showBalance)} />
          </div>
        </div>

        <div className="section-label">{t("settings.automation.section")}</div>
        <div className="card" style={{ padding: "var(--sp-2)" }}>
          <div className="row">
            <div className="ricon">
              <Ms name="automation" />
            </div>
            <div className="rmain">
              <div className="rtitle">
                <span>{t("settings.automation.enable.title")}</span>
                <HelpInfo
                  label={t("settings.automation.info.label")}
                  text={t("settings.automation.info.text")}
                />
              </div>
            </div>
            {profileAutomation && (
              <Toggle
                on={profileAutomation.enabled}
                onClick={() => void saveProfileAutomation({ ...profileAutomation, enabled: !profileAutomation.enabled })}
              />
            )}
          </div>
          <div className={`row automation-dependent-setting${profileAutomation?.enabled ? "" : " disabled"}`}>
            <div className="ricon">
              <Ms name="restore" />
            </div>
            <div className="rmain">
              <SettingTitle title={t("settings.automation.return.title")} description={t("settings.automation.return.description")} />
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
                <option value="">{t("settings.automation.return.previous")}</option>
                {profiles.map((profile) => (
                  <option key={profile.name} value={profile.name}>{t("settings.automation.return.named", { profile: profile.name })}</option>
                ))}
              </select>
            )}
          </div>
          <div className={`row automation-dependent-setting${profileAutomation?.enabled ? "" : " disabled"}`}>
            <div className="ricon">
              <Ms name="notifications" />
            </div>
            <div className="rmain">
              <SettingTitle title={t("settings.automation.notifications.title")} description={t("settings.automation.notifications.description")} />
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

        <div className="section-label">{t("settings.shortcuts.section")}</div>
        <div className="card" style={{ padding: "var(--sp-2)" }}>
          <div className="row">
            <div className="ricon">
              <Ms name="keyboard" />
            </div>
            <div className="rmain">
              <div className="rtitle">
                <span>{t("settings.shortcuts.enable.title")}</span>
                <HelpInfo
                  label={t("settings.shortcuts.info.label")}
                  text={t("settings.shortcuts.info.text")}
                />
              </div>
            </div>
            <Toggle on={shortcutsEnabled} onClick={() => setShortcutsEnabled(!shortcutsEnabled)} />
          </div>
          {SHORTCUT_ROWS.map(({ action, label, icon }) => (
            <label className="row shortcut-row" key={action}>
              <div className="ricon">
                <Ms name={icon} />
              </div>
              <div className="rmain">
                <div className="rtitle">{t(label)}</div>
              </div>
              <input
                className={`shortcut-input${recordingShortcut === action ? " recording" : ""}`}
                value={recordingShortcut === action ? t("settings.shortcuts.recording") : shortcutBindings[action]}
                placeholder={t("settings.shortcuts.placeholder")}
                spellCheck={false}
                readOnly
                title={t("settings.shortcuts.inputHint")}
                aria-label={recordingShortcut === action
                  ? t("settings.shortcuts.recordingLabel", { label: t(label) })
                  : t("settings.shortcuts.label", { label: t(label) })}
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
              {t("settings.shortcuts.restoreDefaults")}
            </button>
          </div>
        </div>

        <div className="section-label">{t("settings.backups.section")}</div>
        <div className="card" style={{ padding: "var(--sp-2)" }}>
          <div className="row">
            <div className="ricon">
              <Ms name="backup" />
            </div>
            <div className="rmain">
              <div className="rtitle">{t("settings.backups.manual.title")}</div>
              <div className="rsub">{backupStatusText(backupStatus, locale, t)}</div>
            </div>
            <button
              type="button"
              className="select"
              disabled={backupBusy !== null}
              onClick={() => void createBackup()}
            >
              <span>{t(backupBusy === "create" ? "settings.backups.creating" : "settings.backups.create")}</span>
            </button>
          </div>
          <div className="row">
            <div className="ricon">
              <Ms name="folder_open" />
            </div>
            <div className="rmain">
              <SettingTitle title={t("settings.backups.location.title")} description={t("settings.backups.location.description")} />
            </div>
            <button
              type="button"
              className="select"
              disabled={backupBusy !== null}
              onClick={() => void openBackupLocation()}
            >
              <span>{t("settings.backups.location.open")}</span>
            </button>
          </div>
          <div className="row">
            <div className="ricon">
              <Ms name="restore_page" />
            </div>
            <div className="rmain">
              <SettingTitle title={t("settings.backups.restore.title")} description={t("settings.backups.restore.description")} />
            </div>
            <button
              type="button"
              className="select"
              disabled={backupBusy !== null}
              onClick={() => void chooseBackup()}
            >
              <span>{t("settings.backups.restore.action")}</span>
            </button>
          </div>
        </div>

        <div className="section-label">{t("settings.about.section")}</div>
        <div className="card" style={{ padding: "var(--sp-2)" }}>
          <div className="row">
            <div className="ricon">
              <Ms name="info" />
            </div>
            <div className="rmain">
              <SettingTitle title={`${t("app.name")} ${version}`} description={t("settings.about.appDescription")} />
            </div>
          </div>
          <div className="row">
            <div className="ricon">
              <Ms name="school" />
            </div>
            <div className="rmain">
              <SettingTitle title={t("settings.about.tutorial.title")} description={t("settings.about.tutorial.description")} />
            </div>
            <button type="button" className="select" onClick={replayOnboarding}>
              <span>{t("common.action.replay")}</span>
            </button>
          </div>
          <div className="row">
            <div className="ricon">
              <Ms name="refresh" />
            </div>
            <div className="rmain">
              <SettingTitle title={t("settings.about.restart.title")} description={t("settings.about.restart.description")} />
            </div>
            <button type="button" className="select" onClick={restartApplication}>
              <span>{t("common.action.restart")}</span>
            </button>
          </div>
          <div className="row">
            <div className="ricon">
              <Ms name="restart_alt" />
            </div>
            <div className="rmain">
              <SettingTitle title={t("settings.about.reset.title")} description={t("settings.about.reset.description")} />
            </div>
            <button type="button" className="select" onClick={() => setConfirmingReset(true)}>
              <span>{t("settings.about.reset.action")}</span>
            </button>
          </div>
        </div>
      </div>
      <Modal open={confirmingMultipleMics} onClose={() => setConfirmingMultipleMics(false)} title={t("settings.multipleDialog.title")}>
        <p className="modal-text">
          {t("settings.multipleDialog.body")}
        </p>
        <p className="modal-text">{t("settings.multipleDialog.detail")}</p>
        <div className="modal-btns">
          <button
            type="button"
            className="modal-btn primary"
            onClick={() => {
              setConfirmingMultipleMics(false);
              void setMultipleMics(true);
            }}
          >
            {t("settings.multipleDialog.confirm")}
          </button>
          <button type="button" className="modal-btn" onClick={() => setConfirmingMultipleMics(false)}>{t("common.action.cancel")}</button>
        </div>
      </Modal>

      <ConfirmModal
        open={restorePath !== null}
        onClose={() => setRestorePath(null)}
        onCancel={() => {
          void invoke("cancel_backup_restore").catch((reason) => setError(String(reason)));
        }}
        title={t("settings.restoreDialog.title", { name: restorePath ? `“${fileName(restorePath)}”` : t("settings.restoreDialog.backup") })}
        confirmLabel={t("settings.restoreDialog.confirm")}
        onConfirm={() => {
          if (restorePath) void restoreBackup();
        }}
      >
        {t("settings.restoreDialog.body")}
      </ConfirmModal>

      <ConfirmModal
        open={confirmingReset}
        onClose={() => setConfirmingReset(false)}
        title={t("settings.resetDialog.title")}
        confirmLabel={t("settings.resetDialog.confirm")}
        onConfirm={() => void invoke("reset_app").catch((e) => setError(String(e)))}
      >
        {t("settings.resetDialog.body")}
      </ConfirmModal>
    </div>
  );
}
