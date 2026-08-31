import { useEffect, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { TitleBar } from "./components/TitleBar/TitleBar";
import { MixerBoard } from "./components/MixerBoard/MixerBoard";
import { AppList } from "./components/AppList/AppList";
import { MicScreen } from "./components/Mic/MicScreen";
import { OnboardingModal } from "./components/Onboarding/OnboardingModal";
import { SettingsScreen } from "./components/Settings/SettingsScreen";
import { ChannelScreen } from "./components/Channel/ChannelScreen";
import { ResizeHandles } from "./components/ResizeHandles";
import { BalanceBar } from "./components/MixerBoard/BalanceBar";
import { ProfileMenu } from "./components/TitleBar/ProfileMenu";
import { ProfileSwitchingScreen } from "./components/ProfileSwitching/ProfileSwitchingScreen";
import { channelAccentClass, channelIcon, Ms } from "./components/Icons";
import { Tooltip } from "./components/Tooltip";
import { useAudio } from "./hooks/useAudio";
import { restartApplication, useGlobalShortcuts } from "./hooks/useGlobalShortcuts";
import { takeRestoreWarning } from "./lib/restoreWarning";
import { useI18n, type TranslationKey } from "./i18n";
import { useMixerStore } from "./store/mixer";

const SIDE_NAV = [
  { id: "mixer", icon: "graphic_eq", label: "navigation.mixer" },
  { id: "apps", icon: "grid_view", label: "navigation.apps" },
  { id: "switching", icon: "bookmarks", label: "navigation.profiles" },
] as const;

type NavId = (typeof SIDE_NAV)[number]["id"] | "mic" | "settings" | `channel:${string}`;

export default function App() {
  const { t } = useI18n();
  useAudio();
  useGlobalShortcuts();
  const [nav, setNav] = useState<NavId>("mixer");
  const [version, setVersion] = useState("");
  const error = useMixerStore((s) => s.error);
  const clearError = useMixerStore((s) => s.clearError);
  const channels = useMixerStore((s) => s.channels);
  const micConfigs = useMixerStore((s) => s.micConfigs);
  const multipleMics = useMixerStore((s) => s.multipleMics);
  const selectedMicNode = useMixerStore((s) => s.selectedMicNode);
  const selectMic = useMixerStore((s) => s.selectMic);
  const activeWorkspaceTab = useRef<HTMLButtonElement | null>(null);

  useEffect(() => {
    void getVersion().then(setVersion);
    const restoreWarning = takeRestoreWarning();
    if (restoreWarning) useMixerStore.setState({ error: restoreWarning });
  }, []);

  const selectedChannelName = nav.startsWith("channel:") ? nav.slice("channel:".length) : null;
  const selectedChannel = channels.find((channel) => channel.name === selectedChannelName);
  const selectedMic = micConfigs.find((mic) => mic.node_name === selectedMicNode) ?? micConfigs[0];
  const visibleMics = multipleMics ? micConfigs : micConfigs.slice(0, 1);

  useEffect(() => {
    if (selectedChannelName && !selectedChannel && channels.length > 0) setNav("mixer");
  }, [channels.length, selectedChannel, selectedChannelName]);

  let currentLabel = t("navigation.mixer");
  if (nav === "apps") currentLabel = t("navigation.applications");
  else if (nav === "switching") currentLabel = t("navigation.profiles");
  else if (nav === "mic") currentLabel = selectedMic?.output_label ?? t("navigation.microphone");
  else if (nav === "settings") currentLabel = t("navigation.settings");
  else if (selectedChannel) currentLabel = selectedChannel.label;

  let screen;
  if (nav === "mixer") {
    screen = (
      <MixerBoard
        onOpenChannel={(name) => setNav(`channel:${name}`)}
        onOpenProfiles={() => setNav("switching")}
        onOpenMic={() => setNav("mic")}
      />
    );
  }
  else if (nav === "apps") screen = <AppList />;
  else if (nav === "switching") screen = (
    <ProfileSwitchingScreen
      onOpenMixer={() => setNav("mixer")}
      onOpenSettings={() => setNav("settings")}
    />
  );
  else if (nav === "mic") screen = <MicScreen />;
  else if (nav === "settings") screen = <SettingsScreen />;
  else if (selectedChannel) screen = <ChannelScreen channel={selectedChannel} />;
  else screen = (
    <MixerBoard
      onOpenChannel={(name) => setNav(`channel:${name}`)}
      onOpenProfiles={() => setNav("switching")}
      onOpenMic={() => setNav("mic")}
    />
  );

  useEffect(() => {
    activeWorkspaceTab.current?.scrollIntoView({ behavior: "smooth", block: "nearest", inline: "nearest" });
  }, [nav, selectedChannelName, selectedMicNode, channels.length, micConfigs.length]);

  return (
    <div className="window">
      <TitleBar screen={currentLabel} />

      {error && (
        <div className="error-banner" role="alert">
          <span className="error-banner-msg">
            <strong>{t("errors.audio")}</strong> {error}
          </span>
          <button
            type="button"
            className="error-banner-restart"
            title={t("errors.restartHint")}
            onClick={restartApplication}
          >
            <Ms name="restart_alt" style={{ fontSize: 15 }} />
            {t("common.action.restart")}
          </button>
          <button
            type="button"
            className="error-banner-x"
            aria-label={t("common.action.dismiss")}
            title={t("common.action.dismiss")}
            onClick={clearError}
          >
            <Ms name="close" style={{ fontSize: 16 }} />
          </button>
        </div>
      )}

      <div className="body">
        <nav className="rail">
          {SIDE_NAV.map((n) => (
            <button
              type="button"
              key={n.id}
              className={"nav-item" + (n.id === nav ? " active" : "")}
              onClick={() => setNav(n.id)}
            >
              <Ms name={n.icon} />
              <span className="nav-label">{t(n.label as TranslationKey)}</span>
            </button>
          ))}
          <div className="rail-spacer" />
          <button
            type="button"
            className={"nav-item" + (nav === "settings" ? " active" : "")}
            onClick={() => setNav("settings")}
          >
            <Ms name="settings" />
            <span className="nav-label">{t("navigation.settings")}</span>
          </button>
          {version && <div className="rail-version">v{version.replace(/\.0$/, "")}</div>}
        </nav>

        <main className="workspace-shell">
          <div className="workspace-bar">
            <nav
              className="workspace-tabs"
              aria-label={t("navigation.audioWorkspace")}
              onWheel={(event) => {
                if (event.currentTarget.scrollWidth <= event.currentTarget.clientWidth || event.deltaY === 0) return;
                event.currentTarget.scrollLeft += event.deltaY;
                event.preventDefault();
              }}
            >
              <button
                type="button"
                className={"workspace-tab" + (nav === "mixer" ? " active" : "")}
                ref={nav === "mixer" ? activeWorkspaceTab : undefined}
                onClick={() => setNav("mixer")}
              >
                {t("navigation.mixer")}
              </button>
              {channels.map((channel) => (
                <button
                  type="button"
                  key={channel.name}
                  className={`workspace-tab ${channelAccentClass(channel)}` + (selectedChannelName === channel.name ? " active" : "")}
                  ref={selectedChannelName === channel.name ? activeWorkspaceTab : undefined}
                  onClick={() => setNav(`channel:${channel.name}`)}
                >
                  <Ms name={channelIcon(channel)} />
                  {channel.label}
                </button>
              ))}
              {visibleMics.map((mic, index) => {
                const active = nav === "mic" && selectedMicNode === mic.node_name;
                return (
                  <button
                    type="button"
                    key={mic.node_name}
                    className={"workspace-tab strip-accent-mic" + (active ? " active" : "")}
                    ref={active ? activeWorkspaceTab : undefined}
                    onClick={() => {
                      selectMic(mic.node_name);
                      setNav("mic");
                    }}
                  >
                    <Ms name={index === 0 ? "mic" : "mic_external_on"} />
                    {index === 0 ? t("navigation.microphone") : mic.output_label}
                  </button>
                );
              })}
            </nav>
            <div className="workspace-tab-tools">
              <BalanceBar />
              <ProfileMenu onManageProfiles={() => setNav("switching")} />
            </div>
          </div>
          {screen}
        </main>
      </div>

      <OnboardingModal />
      <Tooltip />
      <ResizeHandles />
    </div>
  );
}
