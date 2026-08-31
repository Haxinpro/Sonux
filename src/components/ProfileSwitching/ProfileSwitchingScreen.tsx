import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { confirm, open as openDialog } from "@tauri-apps/plugin-dialog";
import type {
  ApplicationProfileRule,
  ProfileContent,
  ProfileAutomationConfig,
} from "../../types";
import { useMixerStore } from "../../store/mixer";
import { ConfirmModal } from "../ConfirmModal";
import { HelpInfo } from "../HelpInfo";
import { Ms } from "../Icons";
import { Modal } from "../Modal";
import { useI18n } from "../../i18n";

function fileName(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

const SECTION_STATE_KEY = "sonux-profile-section-visibility";

interface SectionVisibility {
  channels: boolean;
  applications: boolean;
}

function readSectionVisibility(): SectionVisibility {
  try {
    const saved = JSON.parse(localStorage.getItem(SECTION_STATE_KEY) ?? "null") as Partial<SectionVisibility> | null;
    return {
      channels: typeof saved?.channels === "boolean" ? saved.channels : true,
      applications: typeof saved?.applications === "boolean" ? saved.applications : true,
    };
  } catch {
    return { channels: true, applications: true };
  }
}

export function ProfileSwitchingScreen({
  onOpenMixer,
  onOpenSettings,
}: Readonly<{
  onOpenMixer: () => void;
  onOpenSettings: () => void;
}>) {
  const { t } = useI18n();
  const profiles = useMixerStore((state) => state.profiles);
  const activeProfile = useMixerStore((state) => state.activeProfile);
  const createBlankProfile = useMixerStore((state) => state.createBlankProfile);
  const copyProfile = useMixerStore((state) => state.copyProfile);
  const deleteProfile = useMixerStore((state) => state.deleteProfile);
  const renameProfile = useMixerStore((state) => state.renameProfile);
  const loadProfile = useMixerStore((state) => state.loadProfile);
  const micConfig = useMixerStore((state) => state.micConfig);
  const [selectedProfileName, setSelectedProfileName] = useState(activeProfile ?? "");
  const [config, setConfig] = useState<ProfileAutomationConfig | null>(null);
  const [profileContent, setProfileContent] = useState<ProfileContent | null>(null);
  const [creatingProfile, setCreatingProfile] = useState(false);
  const [newProfileName, setNewProfileName] = useState("");
  const [newProfileMode, setNewProfileMode] = useState<"fresh" | "copy">("fresh");
  const [newProfileMicEnabled, setNewProfileMicEnabled] = useState(true);
  const [copySource, setCopySource] = useState("");
  const [deletingProfileName, setDeletingProfileName] = useState<string | null>(null);
  const [renamingProfileName, setRenamingProfileName] = useState<string | null>(null);
  const [renameDraft, setRenameDraft] = useState("");
  const [sectionVisibility, setSectionVisibility] = useState(readSectionVisibility);
  const [error, setError] = useState<string | null>(null);

  const selectedProfile = profiles.find((profile) => profile.name === selectedProfileName)
    ?? profiles.find((profile) => profile.name === activeProfile)
    ?? profiles[0];
  const selectedRules = config?.rules.filter((rule) => rule.profile === selectedProfile?.name) ?? [];

  const applicationOwner = (executable: string) => config?.rules.find(
    (rule) => rule.executable.toLowerCase() === executable.toLowerCase(),
  );
  useEffect(() => {
    let mounted = true;
    void invoke<ProfileAutomationConfig>("get_profile_automation").then((saved) => {
      if (!mounted) return;
      setConfig(saved);
    }).catch((reason) => setError(String(reason)));
    return () => {
      mounted = false;
    };
  }, []);

  useEffect(() => {
    if (selectedProfile) setSelectedProfileName(selectedProfile.name);
  }, [selectedProfile?.name]);

  useEffect(() => {
    if (!selectedProfile?.name) {
      setProfileContent(null);
      return;
    }
    let mounted = true;
    void invoke<ProfileContent>("get_profile_content", { name: selectedProfile.name })
      .then((content) => { if (mounted) setProfileContent(content); })
      .catch((reason) => { if (mounted) setError(String(reason)); });
    return () => { mounted = false; };
  }, [selectedProfile?.name]);

  const save = async (next: ProfileAutomationConfig): Promise<boolean> => {
    try {
      await invoke("save_profile_automation", { config: next });
      setConfig(next);
      setError(null);
      return true;
    } catch (reason) {
      setError(String(reason));
      return false;
    }
  };

  const toggleSection = (section: keyof SectionVisibility) => {
    setSectionVisibility((current) => {
      const next = { ...current, [section]: !current[section] };
      localStorage.setItem(SECTION_STATE_KEY, JSON.stringify(next));
      return next;
    });
  };

  const addApplication = async (rawExecutable: string, path: string | null = null) => {
    if (!config || !selectedProfile || !rawExecutable.trim()) return;
    const executable = fileName(rawExecutable.trim());
    const owner = applicationOwner(executable);
    if (owner?.profile === selectedProfile.name) return;
    if (owner) {
      const move = await confirm(
        t("profiles.application.moveQuestion", { application: executable, from: owner.profile, to: selectedProfile.name }),
        { title: t("profiles.application.moveTitle"), kind: "warning", okLabel: t("profiles.application.move"), cancelLabel: t("profiles.application.keep") },
      );
      if (!move) return;
    }
    const rule: ApplicationProfileRule = {
      executable,
      path,
      profile: selectedProfile.name,
      enabled: true,
    };
    const next = {
      ...config,
      rules: [...config.rules.filter((item) => item.executable.toLowerCase() !== executable.toLowerCase()), rule],
    };
    if (await save(next)) {
      setError(null);
    }
  };

  const browse = async () => {
    const selected = await openDialog({ title: t("profiles.application.choose"), multiple: false, directory: false });
    if (typeof selected === "string") await addApplication(selected, selected);
  };

  const createProfile = async () => {
    const name = newProfileName.trim();
    if (!name) return;
    const succeeded = newProfileMode === "copy" && copySource
      ? await copyProfile(copySource, name)
      : await createBlankProfile(name, newProfileMicEnabled);
    if (!succeeded) return;
    setSelectedProfileName(name);
    setNewProfileName("");
    setNewProfileMode("fresh");
    setNewProfileMicEnabled(micConfig?.enabled ?? true);
    setCopySource("");
    setCreatingProfile(false);
  };

  const closeCreateProfile = () => {
    setCreatingProfile(false);
    setNewProfileName("");
    setNewProfileMode("fresh");
    setNewProfileMicEnabled(micConfig?.enabled ?? true);
    setCopySource("");
  };

  const deleteSelectedProfile = async () => {
    const name = deletingProfileName;
    if (!name) return;
    const remaining = profiles.filter((profile) => profile.name !== name);
    if (!await deleteProfile(name)) return;
    setDeletingProfileName(null);
    setSelectedProfileName(
      remaining.find((profile) => profile.name === activeProfile)?.name
        ?? remaining[0]?.name
        ?? "",
    );
    try {
      setConfig(await invoke<ProfileAutomationConfig>("get_profile_automation"));
    } catch (reason) {
      setError(String(reason));
    }
  };

  const renameSelectedProfile = async () => {
    const oldName = renamingProfileName;
    const newName = renameDraft.trim();
    if (!oldName || !newName || oldName === newName) return;
    if (!await renameProfile(oldName, newName)) return;
    if (selectedProfileName === oldName) setSelectedProfileName(newName);
    setRenamingProfileName(null);
    setRenameDraft("");
    try {
      setConfig(await invoke<ProfileAutomationConfig>("get_profile_automation"));
    } catch (reason) {
      setError(String(reason));
    }
  };

  if (!config) {
    return <div className="content"><div className="empty-hint">{t("profiles.loading")}</div></div>;
  }

  return (
    <div className="content profile-switching">
      <div className="screen-head">
        <h1>{t("profiles.title")}</h1>
        <HelpInfo label={t("profiles.title")} text={t("profiles.description")} className="screen-head-help" />
      </div>
      <div className="profile-page-layout">
          <aside className="profile-automation-library">
            <div className="profile-automation-list">
              {profiles.map((profile) => {
                const selected = profile.name === selectedProfile?.name;
                const active = profile.name === activeProfile;
                return (
                  <div className={`profile-library-row${selected ? " selected" : ""}`} key={profile.name}>
                    <button type="button" className="profile-library-select" onClick={() => setSelectedProfileName(profile.name)}>
                      <Ms name={active ? "check" : "bookmark"} />
                      <span><strong>{profile.name}</strong></span>
                      {active && <span className="sr-only">{t("profiles.active")}</span>}
                    </button>
                    <div className="profile-library-actions">
                      <button
                        type="button"
                        title={t("profiles.renameNamed", { profile: profile.name })}
                        aria-label={t("profiles.renameNamed", { profile: profile.name })}
                        onClick={() => { setRenamingProfileName(profile.name); setRenameDraft(profile.name); }}
                      ><Ms name="edit" /></button>
                      <button
                        type="button"
                        disabled={profile.protected || profiles.length <= 1}
                        title={profile.protected ? t("profiles.protectedHint") : profiles.length <= 1 ? t("profiles.keepOne") : t("profiles.deleteNamed", { profile: profile.name })}
                        aria-label={profile.protected ? t("profiles.protectedLabel", { profile: profile.name }) : t("profiles.deleteNamed", { profile: profile.name })}
                        onClick={() => setDeletingProfileName(profile.name)}
                      ><Ms name={profile.protected ? "lock" : "delete"} /></button>
                    </div>
                  </div>
                );
              })}
            </div>
            <div className="profile-create-area">
              <button
                type="button"
                className="profile-create-toggle"
                onClick={() => {
                  setNewProfileMicEnabled(micConfig?.enabled ?? true);
                  setCreatingProfile(true);
                }}
              ><Ms name="add" />{t("profiles.new")}</button>
            </div>
          </aside>

          <main className="profile-detail-scroll">
            {error && <div className="automation-error" role="alert">{error}</div>}
            <section className="profile-summary-card card">
            <div className="profile-detail-head profile-detail-overview">
              <div><strong>{selectedProfile?.name ?? t("profiles.generic")}</strong></div>
              <div className="profile-detail-actions">
                {selectedProfile?.name === activeProfile
                  ? <button type="button" className="select profile-summary-action" onClick={onOpenMixer}><Ms name="graphic_eq" />{t("profiles.openMixer")}</button>
                  : <button type="button" className="select profile-summary-action" disabled={!selectedProfile} onClick={() => selectedProfile && void loadProfile(selectedProfile.name)}><Ms name="play_arrow" />{t("profiles.activateSelected")}</button>}
              </div>
            </div>
            <div className="profile-channels-section">
              <button
                type="button"
                className={`profile-section-head profile-collapse-button${sectionVisibility.channels ? " open" : " collapsed"}`}
                aria-expanded={sectionVisibility.channels}
                aria-controls="profile-channel-list"
                onClick={() => toggleSection("channels")}
              >
                <div><strong>{t("mixer.group.channels")}</strong><small>{t("profiles.channels.saved", { count: (profileContent?.channels.length ?? 0) + 1 + (profileContent?.secondary_mics.length ?? 0) })}</small></div>
                <Ms name="chevron_right" />
              </button>
              <div
                id="profile-channel-list"
                className={`profile-section-reveal${sectionVisibility.channels ? " open" : ""}`}
                aria-hidden={!sectionVisibility.channels}
                ref={(element) => element?.toggleAttribute("inert", !sectionVisibility.channels)}
              >
                <div className="profile-section-reveal-inner">
                <div className="profile-channel-grid">
                {profileContent?.channels.map((channel) => (
                  <div key={channel.name}>
                    <Ms name={channel.icon ?? "tune"} />
                    <span><strong>{channel.label}</strong><small>{channel.muted ? t("channel.muted") : `${channel.volume_percent}%`}</small></span>
                  </div>
                ))}
                {profileContent && (
                  <div>
                    <Ms name="mic" />
                    <span><strong>{profileContent.mic.output_label}</strong><small>{!profileContent.mic.enabled ? t("profiles.disabled") : profileContent.mic.muted ? t("channel.muted") : `${profileContent.mic.gain_percent}%`}</small></span>
                  </div>
                )}
                {profileContent?.secondary_mics.map((mic) => (
                  <div key={mic.node_name}>
                    <Ms name="mic_external_on" />
                    <span><strong>{mic.output_label}</strong><small>{!mic.enabled ? t("profiles.disabled") : mic.muted ? t("channel.muted") : `${mic.gain_percent}%`}</small></span>
                  </div>
                ))}
                </div>
                </div>
              </div>
            </div>
            </section>

            <section className="profile-applications-card card">
            <div className="profile-application-head">
              <div className="profile-application-heading">
                <strong>{t("applications.title")}</strong>
                <HelpInfo
                  label={t("applications.title")}
                  text={t("profiles.applications.description", { profile: selectedProfile?.name ?? t("profiles.thisProfile") })}
                />
              </div>
              <button
                type="button"
                className={`select profile-application-add${sectionVisibility.applications ? "" : " hidden"}`}
                disabled={!selectedProfile || !sectionVisibility.applications}
                aria-hidden={!sectionVisibility.applications}
                tabIndex={sectionVisibility.applications ? 0 : -1}
                onClick={() => void browse()}
              >
                <Ms name="folder_open" />{t("profiles.applications.add")}
              </button>
              <button
                type="button"
                className={`profile-section-toggle${sectionVisibility.applications ? " open" : ""}`}
                aria-label={t("applications.title")}
                aria-expanded={sectionVisibility.applications}
                aria-controls="profile-application-list"
                onClick={() => toggleSection("applications")}
              >
                <Ms name="chevron_right" />
              </button>
            </div>

            <div
              id="profile-application-list"
              className={`profile-section-reveal${sectionVisibility.applications ? " open" : ""}`}
              aria-hidden={!sectionVisibility.applications}
              ref={(element) => element?.toggleAttribute("inert", !sectionVisibility.applications)}
            >
            <div className="profile-section-reveal-inner">
            {!config.enabled && (
              <button type="button" className="profile-automation-disabled" onClick={onOpenSettings}>
                <Ms name="info" />
                <span><strong>{t("profiles.automationDisabled.title")}</strong><small>{t("profiles.automationDisabled.body")}</small></span>
                <span>{t("profiles.openSettings")}</span>
                <Ms name="chevron_right" />
              </button>
            )}

            {selectedRules.length ? (
              <div className="profile-application-rules">
                {selectedRules.map((rule) => (
                  <div key={rule.executable.toLowerCase()}>
                    <Ms name="deployed_code" />
                    <span className="profile-rule-copy">
                      <strong>{rule.executable}</strong>
                      {rule.path && <small className="profile-rule-path" title={rule.path}>{rule.path}</small>}
                    </span>
                    <div className="profile-rule-actions">
                      <button
                        type="button"
                        role="switch"
                        aria-checked={rule.enabled}
                        className={`profile-rule-switch${rule.enabled ? " on" : ""}`}
                        title={t(rule.enabled ? "profiles.link.disable" : "profiles.link.enable")}
                        onClick={() => void save({ ...config, rules: config.rules.map((item) => item === rule ? { ...item, enabled: !item.enabled } : item) })}
                      ><i /></button>
                      <button type="button" aria-label={t("profiles.application.remove", { application: rule.executable })} title={t("profiles.application.removeHint")} onClick={() => void save({ ...config, rules: config.rules.filter((item) => item !== rule) })}><Ms name="close" /></button>
                    </div>
                  </div>
                ))}
              </div>
            ) : <div className="profile-application-empty"><Ms name="automation" /><span><strong>{t("profiles.applications.none")}</strong><small>{t("profiles.applications.manual")}</small></span></div>}
            </div>
            </div>

          </section>
          </main>
      </div>

      <Modal open={creatingProfile} onClose={closeCreateProfile} title={t("profiles.new")} className="profile-create-modal">
        <form className="profile-create-modal-form" onSubmit={(event) => { event.preventDefault(); void createProfile(); }}>
          <label>
            <span className="modal-label">{t("profiles.create.name")}</span>
            <input autoFocus value={newProfileName} maxLength={64} placeholder={t("profiles.create.placeholder")} onChange={(event) => setNewProfileName(event.target.value)} />
          </label>
          <div className="modal-label">{t("profiles.create.startWith")}</div>
          <div className="profile-create-modes">
            <button type="button" className={newProfileMode === "fresh" ? "selected" : ""} onClick={() => setNewProfileMode("fresh")}>
              <Ms name="draft" /><span><strong>{t("profiles.create.fresh")}</strong><small>{t("profiles.create.freshHint")}</small></span><Ms name={newProfileMode === "fresh" ? "radio_button_checked" : "radio_button_unchecked"} />
            </button>
            <button
              type="button"
              className={newProfileMode === "copy" ? "selected" : ""}
              onClick={() => {
                setNewProfileMode("copy");
                if (!copySource) setCopySource(activeProfile ?? profiles[0]?.name ?? "");
              }}
            >
              <Ms name="content_copy" /><span><strong>{t("profiles.create.copy")}</strong><small>{t("profiles.create.copyHint")}</small></span><Ms name={newProfileMode === "copy" ? "radio_button_checked" : "radio_button_unchecked"} />
            </button>
          </div>
          {newProfileMode === "copy" && (
            <label>
              <span className="modal-label">{t("profiles.create.copySource")}</span>
              <select className="select profile-copy-source" value={copySource} onChange={(event) => setCopySource(event.target.value)}>
                {profiles.map((profile) => <option value={profile.name} key={profile.name}>{profile.name}</option>)}
              </select>
            </label>
          )}
          {newProfileMode === "fresh" && (
            <label className="profile-create-mic-option">
              <input
                type="checkbox"
                checked={newProfileMicEnabled}
                onChange={(event) => setNewProfileMicEnabled(event.target.checked)}
              />
              <Ms name="mic" />
              <span>
                <strong>{t("profiles.create.enableMic")}</strong>
                <small>{t("profiles.create.enableMicHint")}</small>
              </span>
            </label>
          )}
          <div className="modal-btns">
            <button type="button" className="modal-btn" onClick={closeCreateProfile}>{t("common.action.cancel")}</button>
            <button type="submit" className="modal-btn primary" disabled={!newProfileName.trim() || (newProfileMode === "copy" && !copySource)}>{t("profiles.create.action")}</button>
          </div>
        </form>
      </Modal>

      <ConfirmModal
        open={deletingProfileName !== null}
        onClose={() => setDeletingProfileName(null)}
        title={t("profiles.delete.title", { profile: deletingProfileName ?? "" })}
        confirmLabel={t("profiles.delete.action")}
        onConfirm={() => void deleteSelectedProfile()}
      >
        {t("profiles.delete.body")}{deletingProfileName === activeProfile ? ` ${t("profiles.delete.activeBody")}` : ""}
      </ConfirmModal>

      <Modal
        open={renamingProfileName !== null}
        onClose={() => { setRenamingProfileName(null); setRenameDraft(""); }}
        title={t("profiles.rename.title", { profile: renamingProfileName ?? "" })}
      >
        <form className="profile-rename-form" onSubmit={(event) => { event.preventDefault(); void renameSelectedProfile(); }}>
          <label>
            <span className="modal-label">{t("profiles.create.name")}</span>
            <input autoFocus value={renameDraft} maxLength={64} onChange={(event) => setRenameDraft(event.target.value)} />
          </label>
          <div className="modal-btns">
            <button type="button" className="modal-btn" onClick={() => { setRenamingProfileName(null); setRenameDraft(""); }}>{t("common.action.cancel")}</button>
            <button type="submit" className="modal-btn primary" disabled={!renameDraft.trim() || renameDraft.trim() === renamingProfileName}>{t("profiles.rename.action")}</button>
          </div>
        </form>
      </Modal>
    </div>
  );
}
