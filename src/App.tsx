import { useEffect, useState } from "react";
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
import { channelAccentClass, channelIcon, Ms } from "./components/Icons";
import { Tooltip } from "./components/Tooltip";
import { useAudio } from "./hooks/useAudio";
import { restartApplication, useGlobalShortcuts } from "./hooks/useGlobalShortcuts";
import { useMixerStore } from "./store/mixer";

const SIDE_NAV = [
  { id: "mixer", icon: "graphic_eq", label: "Mixer" },
  { id: "apps", icon: "grid_view", label: "Apps" },
] as const;

type NavId = (typeof SIDE_NAV)[number]["id"] | "mic" | "settings" | `channel:${string}`;

export default function App() {
  useAudio();
  useGlobalShortcuts();
  const [nav, setNav] = useState<NavId>("mixer");
  const [version, setVersion] = useState("");
  const error = useMixerStore((s) => s.error);
  const clearError = useMixerStore((s) => s.clearError);
  const channels = useMixerStore((s) => s.channels);

  useEffect(() => {
    void getVersion().then(setVersion);
  }, []);

  const selectedChannelName = nav.startsWith("channel:") ? nav.slice("channel:".length) : null;
  const selectedChannel = channels.find((channel) => channel.name === selectedChannelName);

  useEffect(() => {
    if (selectedChannelName && !selectedChannel && channels.length > 0) setNav("mixer");
  }, [channels.length, selectedChannel, selectedChannelName]);

  let currentLabel = "Mixer";
  if (nav === "apps") currentLabel = "Apps";
  else if (nav === "mic") currentLabel = "Mic";
  else if (nav === "settings") currentLabel = "Settings";
  else if (selectedChannel) currentLabel = selectedChannel.label;

  let screen;
  if (nav === "mixer") {
    screen = <MixerBoard onOpenChannel={(name) => setNav(`channel:${name}`)} />;
  }
  else if (nav === "apps") screen = <AppList />;
  else if (nav === "mic") screen = <MicScreen />;
  else if (nav === "settings") screen = <SettingsScreen />;
  else if (selectedChannel) screen = <ChannelScreen channel={selectedChannel} />;
  else screen = <MixerBoard onOpenChannel={(name) => setNav(`channel:${name}`)} />;

  return (
    <div className="window">
      <TitleBar screen={currentLabel} />

      {error && (
        <div className="error-banner" role="alert">
          <span className="error-banner-msg">
            <strong>Audio error:</strong> {error}
          </span>
          <button
            type="button"
            className="error-banner-restart"
            title="Restart the application without deleting settings"
            onClick={restartApplication}
          >
            <Ms name="restart_alt" style={{ fontSize: 15 }} />
            Restart
          </button>
          <button
            type="button"
            className="error-banner-x"
            aria-label="Dismiss error"
            title="Dismiss"
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
              <span className="nav-label">{n.label}</span>
            </button>
          ))}
          <div className="rail-spacer" />
          <button
            type="button"
            className={"nav-item" + (nav === "settings" ? " active" : "")}
            onClick={() => setNav("settings")}
          >
            <Ms name="settings" />
            <span className="nav-label">Settings</span>
          </button>
          {version && <div className="rail-version">v{version.replace(/\.0$/, "")}</div>}
        </nav>

        <main className="workspace-shell">
          <div className="workspace-bar">
            <nav className="workspace-tabs" aria-label="Audio workspace">
              <button
                type="button"
                className={"workspace-tab" + (nav === "mixer" ? " active" : "")}
                onClick={() => setNav("mixer")}
              >
                Mixer
              </button>
              {channels.map((channel) => (
                <button
                  type="button"
                  key={channel.name}
                  className={`workspace-tab ${channelAccentClass(channel)}` + (selectedChannelName === channel.name ? " active" : "")}
                  onClick={() => setNav(`channel:${channel.name}`)}
                >
                  <Ms name={channelIcon(channel)} />
                  {channel.label}
                </button>
              ))}
              <button
                type="button"
                className={"workspace-tab strip-accent-mic" + (nav === "mic" ? " active" : "")}
                onClick={() => setNav("mic")}
              >
                <Ms name="mic" />
                Mic
              </button>
            </nav>
            <div className="workspace-tab-tools">
              <BalanceBar />
              <ProfileMenu />
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
