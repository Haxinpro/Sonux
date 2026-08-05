import { useState } from "react";
import { useMixerStore } from "../../store/mixer";
import type { VirtualSink } from "../../types";
import { defaultEqConfig, MAX_VOLUME } from "../../types";
import { volToDb } from "../../lib/audio";
import { HSlider } from "../AppList/HSlider";
import { EqEditor } from "../Eq/EqModal";
import { EqPresetMenu } from "../Eq/EqPresetMenu";
import { channelAccentClass, channelIcon, Ms } from "../Icons";
import { ChannelApps } from "../MixerBoard/ChannelApps";
import { OutputSelect } from "../MixerBoard/OutputSelect";
import { ChannelProcessing } from "./ChannelProcessing";
import { AudioTestControls } from "../AudioTestControls";

export function ChannelScreen({ channel }: Readonly<{ channel: VirtualSink }>) {
  const setChannelVolume = useMixerStore((state) => state.setChannelVolume);
  const toggleMute = useMixerStore((state) => state.toggleMute);
  const output = useMixerStore((state) => state.channelOutputs[channel.name] ?? null);
  const resolvedOutput = useMixerStore((state) => state.resolvedOutputs[channel.name] ?? null);
  const failover = useMixerStore((state) => state.channelFailover[channel.name] ?? true);
  const setChannelOutput = useMixerStore((state) => state.setChannelOutput);
  const setChannelFailover = useMixerStore((state) => state.setChannelFailover);
  const listening = useMixerStore((state) => state.monitors[channel.name] ?? false);
  const toggleMonitor = useMixerStore((state) => state.toggleMonitor);
  const appStreams = useMixerStore((state) => state.appStreams);
  const seenApps = useMixerStore((state) => state.seenApps);
  const eqConfig =
    useMixerStore((state) => state.eqConfigs[channel.name] ?? null) ??
    defaultEqConfig();
  const setChannelEq = useMixerStore((state) => state.setChannelEq);
  const [managingApps, setManagingApps] = useState(false);
  const setError = (message: string) => useMixerStore.setState({ error: message });

  const liveIds = new Set(
    appStreams.map((app) => `${app.match_prop}\0${app.match_value}`),
  );
  const appCount =
    appStreams.filter((app) => app.assigned_sink === channel.name).length +
    seenApps.filter(
      (app) =>
        !app.ignored &&
        app.assigned_sink === channel.name &&
        !liveIds.has(`${app.match_prop}\0${app.match_value}`),
    ).length;

  return (
    <div className={`content channel-page ${channelAccentClass(channel)}`}>
      <div className="screen-head channel-screen-head">
        <div className="channel-heading-icon">
          <Ms name={channelIcon(channel)} />
        </div>
        <h1>{channel.label}</h1>
        <div className="screen-head-actions">
          <button
            type="button"
            className={"select channel-head-control" + (channel.muted ? " channel-action-danger" : "")}
            onClick={() => void toggleMute(channel.name, !channel.muted)}
          >
            <Ms name={channel.muted ? "volume_off" : "volume_up"} />
            {channel.muted ? "Muted" : "Mute"}
          </button>
          <button
            type="button"
            className={"select channel-head-control" + (listening ? " on-mon" : "")}
            aria-pressed={listening}
            title="Listen to this channel on the default output"
            onClick={() => void toggleMonitor(channel.name)}
          >
            <Ms name="headphones" />
            {listening ? "Listening" : "Listen"}
          </button>
          <AudioTestControls kind="channel" sinkName={channel.name} />
        </div>
      </div>

      <div className="screen-scroll channel-scroll">
        <div className="section-label">Channel</div>
        <div className="card channel-controls-card">
          <div className="channel-control-block channel-preset-control">
            <div>
              <div className="rtitle">Presets</div>
            </div>
            <EqPresetMenu
              sinkName={channel.name}
              config={eqConfig}
              onApply={(config) => void setChannelEq(channel.name, config)}
              onError={setError}
            />
          </div>
          <div className="channel-control-block channel-volume-control">
            <div>
              <div className="rtitle">Volume</div>
            </div>
            <HSlider
              value={channel.volume_percent}
              max={MAX_VOLUME}
              valueLabel={`${channel.volume_percent}% · ${volToDb(channel.volume_percent)}`}
              onChange={(value) => void setChannelVolume(channel.name, value)}
            />
          </div>
          <div className="channel-control-block">
            <div>
              <div className="rtitle">Device</div>
            </div>
            <OutputSelect
              value={output}
              resolved={resolvedOutput}
              failover={failover}
              onFailoverChange={(enabled) => void setChannelFailover(channel.name, enabled)}
              onChange={(selected) => void setChannelOutput(channel.name, selected)}
            />
          </div>
          <div className="channel-control-block channel-app-control">
            <div style={{ position: "relative" }}>
              <button
                type="button"
                className="select"
                title="Choose applications assigned to this channel"
                onClick={() => setManagingApps(true)}
              >
                {appCount} {appCount === 1 ? "app" : "apps"}
              </button>
              <ChannelApps
                channel={channel}
                open={managingApps}
                onClose={() => setManagingApps(false)}
              />
            </div>
          </div>
        </div>

        <div className="card channel-eq-card">
          <EqEditor channel={channel} />
        </div>

        <div className="section-label">Playback processing</div>
        <ChannelProcessing channel={channel} />
      </div>
    </div>
  );
}
