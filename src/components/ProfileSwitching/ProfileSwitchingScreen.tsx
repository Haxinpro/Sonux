import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { confirm, open as openDialog } from "@tauri-apps/plugin-dialog";
import type {
  ApplicationProfileRule,
  ProfileContent,
  ProfileAutomationConfig,
} from "../../types";
import { useMixerStore } from "../../store/mixer";
import { ConfirmModal } from "../ConfirmModal";
import { Ms } from "../Icons";
import { Modal } from "../Modal";

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
  const [profileSearch, setProfileSearch] = useState("");
  const [sectionVisibility, setSectionVisibility] = useState(readSectionVisibility);
  const [error, setError] = useState<string | null>(null);

  const selectedProfile = profiles.find((profile) => profile.name === selectedProfileName)
    ?? profiles.find((profile) => profile.name === activeProfile)
    ?? profiles[0];
  const selectedRules = config?.rules.filter((rule) => rule.profile === selectedProfile?.name) ?? [];

  const applicationOwner = (executable: string) => config?.rules.find(
    (rule) => rule.executable.toLowerCase() === executable.toLowerCase(),
  );
  const visibleProfiles = useMemo(() => {
    const query = profileSearch.trim().toLowerCase();
    if (!query) return profiles;
    return profiles.filter((profile) => {
      if (profile.name.toLowerCase().includes(query)) return true;
      return config?.rules.some((rule) => (
        rule.profile === profile.name && rule.executable.toLowerCase().includes(query)
      ));
    });
  }, [config?.rules, profileSearch, profiles]);

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
        `${executable} currently activates ${owner.profile}. Move it to ${selectedProfile.name}?`,
        { title: "Move application?", kind: "warning", okLabel: "Move application", cancelLabel: "Keep current profile" },
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
    const selected = await openDialog({ title: "Choose application executable", multiple: false, directory: false });
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
    return <div className="content"><div className="empty-hint">Loading profiles…</div></div>;
  }

  return (
    <div className="content profile-switching">
      <div className="screen-head">
        <h1>Profiles</h1>
        <div className="sub">Create, activate and manage complete audio setups</div>
      </div>
      <div className="profile-page-layout">
          <aside className="profile-automation-library">
            <div className="profile-panel-head"><div><strong>Your profiles</strong><small>{profiles.length} saved {profiles.length === 1 ? "profile" : "profiles"}</small></div></div>
            <label className="profile-library-search">
              <Ms name="search" />
              <input value={profileSearch} placeholder="Search profiles or applications" onChange={(event) => setProfileSearch(event.target.value)} />
              {profileSearch && <button type="button" aria-label="Clear search" onClick={() => setProfileSearch("")}><Ms name="close" /></button>}
            </label>
            <div className="profile-automation-list">
              {visibleProfiles.map((profile) => {
                const selected = profile.name === selectedProfile?.name;
                const active = profile.name === activeProfile;
                const linked = config.rules.filter((rule) => rule.profile === profile.name).length;
                return (
                  <div className={`profile-library-row${selected ? " selected" : ""}`} key={profile.name}>
                    <button type="button" className="profile-library-select" onClick={() => setSelectedProfileName(profile.name)}>
                      <Ms name={active ? "check" : "bookmark"} />
                      <span><strong>{profile.name}</strong><small>{linked} linked {linked === 1 ? "application" : "applications"}</small></span>
                      {active && <em>Active</em>}
                    </button>
                    <div className="profile-library-actions">
                      <button
                        type="button"
                        title={`Rename ${profile.name}`}
                        aria-label={`Rename ${profile.name}`}
                        onClick={() => { setRenamingProfileName(profile.name); setRenameDraft(profile.name); }}
                      ><Ms name="edit" /></button>
                      <button
                        type="button"
                        disabled={profile.protected || profiles.length <= 1}
                        title={profile.protected ? "This fallback profile is always kept" : profiles.length <= 1 ? "Keep at least one profile" : `Delete ${profile.name}`}
                        aria-label={profile.protected ? `${profile.name} is the protected fallback profile` : `Delete ${profile.name}`}
                        onClick={() => setDeletingProfileName(profile.name)}
                      ><Ms name={profile.protected ? "lock" : "delete"} /></button>
                    </div>
                  </div>
                );
              })}
              {visibleProfiles.length === 0 && <p className="profile-search-empty">No matching profiles</p>}
            </div>
            <div className="profile-create-area">
              <button
                type="button"
                className="profile-create-toggle"
                onClick={() => {
                  setNewProfileMicEnabled(micConfig?.enabled ?? true);
                  setCreatingProfile(true);
                }}
              ><Ms name="add" />New profile</button>
            </div>
          </aside>

          <main className="profile-detail-scroll">
            {error && <div className="automation-error" role="alert">{error}</div>}
            <section className="profile-summary-card card">
            <div className="profile-detail-head profile-detail-overview">
              <div><small>Audio profile</small><strong>{selectedProfile?.name ?? "Profile"}</strong></div>
              <div className="profile-detail-actions">
                {selectedProfile?.name === activeProfile
                  ? <button type="button" className="profile-activate-button" onClick={onOpenMixer}><Ms name="graphic_eq" />Open Mixer</button>
                  : <button type="button" className="profile-activate-button" disabled={!selectedProfile} onClick={() => selectedProfile && void loadProfile(selectedProfile.name)}><Ms name="play_arrow" />Activate selected profile</button>}
              </div>
            </div>
            <div className="profile-channels-section">
              <button
                type="button"
                className={`profile-section-head profile-collapse-button${sectionVisibility.channels ? "" : " collapsed"}`}
                aria-expanded={sectionVisibility.channels}
                aria-controls="profile-channel-list"
                onClick={() => toggleSection("channels")}
              >
                <div><strong>Channels</strong><small>{(profileContent?.channels.length ?? 0) + 1 + (profileContent?.secondary_mics.length ?? 0)} saved channels</small></div>
                <Ms name={sectionVisibility.channels ? "expand_more" : "chevron_right"} />
              </button>
              {sectionVisibility.channels && <div className="profile-channel-grid" id="profile-channel-list">
                {profileContent?.channels.map((channel) => (
                  <div key={channel.name}>
                    <Ms name={channel.icon ?? "tune"} />
                    <span><strong>{channel.label}</strong><small>{channel.muted ? "Muted" : `${channel.volume_percent}%`}</small></span>
                  </div>
                ))}
                {profileContent && (
                  <div>
                    <Ms name="mic" />
                    <span><strong>{profileContent.mic.output_label}</strong><small>{!profileContent.mic.enabled ? "Disabled" : profileContent.mic.muted ? "Muted" : `${profileContent.mic.gain_percent}%`}</small></span>
                  </div>
                )}
                {profileContent?.secondary_mics.map((mic) => (
                  <div key={mic.node_name}>
                    <Ms name="mic_external_on" />
                    <span><strong>{mic.output_label}</strong><small>{!mic.enabled ? "Disabled" : mic.muted ? "Muted" : `${mic.gain_percent}%`}</small></span>
                  </div>
                ))}
              </div>}
            </div>
            </section>

            <section className="profile-applications-card card">
            <div className="profile-application-head">
              <button
                type="button"
                className="profile-collapse-button"
                aria-expanded={sectionVisibility.applications}
                aria-controls="profile-application-list"
                onClick={() => toggleSection("applications")}
              >
                <div><strong>Applications</strong><small>Programs that activate {selectedProfile?.name ?? "this profile"} when they start</small></div>
                <Ms name={sectionVisibility.applications ? "expand_more" : "chevron_right"} />
              </button>
              {sectionVisibility.applications && (
                <button type="button" className="select" disabled={!selectedProfile} onClick={() => void browse()}>
                  <Ms name="folder_open" />Add application
                </button>
              )}
            </div>

            {sectionVisibility.applications && <div id="profile-application-list">
            {!config.enabled && (
              <button type="button" className="profile-automation-disabled" onClick={onOpenSettings}>
                <Ms name="info" />
                <span><strong>Automatic switching is disabled</strong><small>Enable it in Settings to use application links.</small></span>
                <span>Open Settings</span>
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
                        title={rule.enabled ? "Disable this application link" : "Enable this application link"}
                        onClick={() => void save({ ...config, rules: config.rules.map((item) => item === rule ? { ...item, enabled: !item.enabled } : item) })}
                      ><i /></button>
                      <button type="button" aria-label={`Remove ${rule.executable}`} title="Remove application" onClick={() => void save({ ...config, rules: config.rules.filter((item) => item !== rule) })}><Ms name="close" /></button>
                    </div>
                  </div>
                ))}
              </div>
            ) : <div className="profile-application-empty"><Ms name="automation" /><span><strong>No linked applications</strong><small>This profile is activated manually.</small></span></div>}
            </div>}

          </section>
          </main>
      </div>

      <Modal open={creatingProfile} onClose={closeCreateProfile} title="New profile" className="profile-create-modal">
        <form className="profile-create-modal-form" onSubmit={(event) => { event.preventDefault(); void createProfile(); }}>
          <label>
            <span className="modal-label">Profile name</span>
            <input autoFocus value={newProfileName} maxLength={64} placeholder="e.g. Competitive gaming" onChange={(event) => setNewProfileName(event.target.value)} />
          </label>
          <div className="modal-label">Start with</div>
          <div className="profile-create-modes">
            <button type="button" className={newProfileMode === "fresh" ? "selected" : ""} onClick={() => setNewProfileMode("fresh")}>
              <Ms name="draft" /><span><strong>Fresh setup</strong><small>Start with the default channels</small></span><Ms name={newProfileMode === "fresh" ? "radio_button_checked" : "radio_button_unchecked"} />
            </button>
            <button
              type="button"
              className={newProfileMode === "copy" ? "selected" : ""}
              onClick={() => {
                setNewProfileMode("copy");
                if (!copySource) setCopySource(activeProfile ?? profiles[0]?.name ?? "");
              }}
            >
              <Ms name="content_copy" /><span><strong>Copy an existing profile</strong><small>Reuse its current audio setup</small></span><Ms name={newProfileMode === "copy" ? "radio_button_checked" : "radio_button_unchecked"} />
            </button>
          </div>
          {newProfileMode === "copy" && (
            <label>
              <span className="modal-label">Profile to copy</span>
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
                <strong>Enable microphone</strong>
                <small>Create the processed Sonux microphone with this profile</small>
              </span>
            </label>
          )}
          <div className="modal-btns">
            <button type="button" className="modal-btn" onClick={closeCreateProfile}>Cancel</button>
            <button type="submit" className="modal-btn primary" disabled={!newProfileName.trim() || (newProfileMode === "copy" && !copySource)}>Create and activate</button>
          </div>
        </form>
      </Modal>

      <ConfirmModal
        open={deletingProfileName !== null}
        onClose={() => setDeletingProfileName(null)}
        title={`Delete profile "${deletingProfileName ?? ""}"?`}
        confirmLabel="Delete profile"
        onConfirm={() => void deleteSelectedProfile()}
      >
        This permanently deletes the profile and all of its application links. Its saved channel layout, levels, routing, outputs, EQ and mixes cannot be recovered.{deletingProfileName === activeProfile ? " This is the active profile, so Sonux will activate another profile before deleting it." : ""}
      </ConfirmModal>

      <Modal
        open={renamingProfileName !== null}
        onClose={() => { setRenamingProfileName(null); setRenameDraft(""); }}
        title={`Rename "${renamingProfileName ?? ""}"`}
      >
        <form className="profile-rename-form" onSubmit={(event) => { event.preventDefault(); void renameSelectedProfile(); }}>
          <label>
            <span className="modal-label">Profile name</span>
            <input autoFocus value={renameDraft} maxLength={64} onChange={(event) => setRenameDraft(event.target.value)} />
          </label>
          <div className="modal-btns">
            <button type="button" className="modal-btn" onClick={() => { setRenamingProfileName(null); setRenameDraft(""); }}>Cancel</button>
            <button type="submit" className="modal-btn primary" disabled={!renameDraft.trim() || renameDraft.trim() === renamingProfileName}>Rename profile</button>
          </div>
        </form>
      </Modal>
    </div>
  );
}
