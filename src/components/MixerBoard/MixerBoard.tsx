import { useState } from "react";
import type { ReactNode } from "react";
import { useMixerStore } from "../../store/mixer";
import { MASTER_BUS } from "../../types";
import { Ms, ICON_CHOICES } from "../Icons";
import { Modal } from "../Modal";
import { ChannelStrip } from "./ChannelStrip";
import { MicStrip } from "./MicStrip";
import { BusStrip } from "./StreamMixStrip";
import { useI18n } from "../../i18n";
import { applicationGroupKey } from "../../lib/appGroups";

// UI-side gates only; the backend enforces the real limits.
const MAX_CHANNELS = 10;
const MAX_BUSES = 4;

/** Signal-flow group: header row (icon, label, count, optional +) above
 * its strips - per the updated design. */
function MixGroup({
  icon,
  label,
  count,
  hint,
  onAdd,
  addTitle,
  children,
}: Readonly<{
  icon: string;
  label: string;
  count: string;
  /** Hover explanation of what this group does. */
  hint: string;
  onAdd?: () => void;
  addTitle?: string;
  children: ReactNode;
}>) {
  return (
    <div className="mix-group">
      <div className="group-head" title={hint}>
        <Ms name={icon} className="gh-icon" />
        <span className="gh-label">{label}</span>
        <span className="gh-count">{count}</span>
        {onAdd && (
          <div className="gh-add-wrap">
            <button type="button" className="gh-add" onClick={onAdd} title={addTitle}>
              <Ms name="add" />
            </button>
          </div>
        )}
      </div>
      <div className="group-strips">{children}</div>
    </div>
  );
}

export function MixerBoard({
  onOpenChannel,
  onOpenProfiles,
  onOpenMic,
}: Readonly<{
  onOpenChannel: (name: string) => void;
  onOpenProfiles: () => void;
  onOpenMic: () => void;
}>) {
  const { t } = useI18n();
  const channels = useMixerStore((s) => s.channels);
  const buses = useMixerStore((s) => s.buses);
  const appStreams = useMixerStore((s) => s.appStreams);
  const addChannel = useMixerStore((s) => s.addChannel);
  const addBus = useMixerStore((s) => s.addBus);
  const micConfig = useMixerStore((s) => s.micConfig);
  const micConfigs = useMixerStore((s) => s.micConfigs);
  const multipleMics = useMixerStore((s) => s.multipleMics);
  const selectMic = useMixerStore((s) => s.selectMic);
  const setMicCreationOpen = useMixerStore((s) => s.setMicCreationOpen);
  const moveMicChannel = useMixerStore((s) => s.moveMicChannel);
  const commitMicChannelOrder = useMixerStore((s) => s.commitMicChannelOrder);
  const backendNative = useMixerStore((s) => s.backendNative);

  const moveChannel = useMixerStore((s) => s.moveChannel);
  const commitChannelOrder = useMixerStore((s) => s.commitChannelOrder);

  const [addingChannel, setAddingChannel] = useState(false);
  const [channelLabel, setChannelLabel] = useState("");
  const [channelIcon, setChannelIcon] = useState(ICON_CHOICES[0]);
  const [channelSpatial, setChannelSpatial] = useState(false);
  const [addingMix, setAddingMix] = useState(false);
  const [mixLabel, setMixLabel] = useState("");
  const [draggingChannel, setDraggingChannel] = useState<string | null>(null);
  const [draggingMic, setDraggingMic] = useState<string | null>(null);

  if (channels.length === 0) {
    return (
      <div className="content">
        <div className="empty-hint" style={{ margin: "auto" }}>
          {t("mixer.loading")}
        </div>
      </div>
    );
  }

  // The strip is a live mixer view: its count includes only identities that
  // currently own a PipeWire stream. Remembered/offline assignments remain
  // available in the channel membership popover and Applications screen.
  const identitiesByChannel = new Map<string, Set<string>>();
  for (const stream of appStreams) {
    const key = applicationGroupKey(stream);
    if (stream.assigned_sink) {
      const identities = identitiesByChannel.get(stream.assigned_sink) ?? new Set<string>();
      identities.add(key);
      identitiesByChannel.set(stream.assigned_sink, identities);
    }
  }
  const counts = new Map(
    Array.from(identitiesByChannel, ([channel, identities]) => [channel, identities.size]),
  );

  const closeChannelModal = () => {
    setAddingChannel(false);
    setChannelLabel("");
    setChannelIcon(ICON_CHOICES[0]);
    setChannelSpatial(false);
  };
  const createChannel = () => {
    const label = channelLabel.trim();
    if (!label) return;
    void addChannel(label, channelIcon, channelSpatial);
    closeChannelModal();
  };
  const createMix = () => {
    const label = mixLabel.trim();
    setMixLabel("");
    setAddingMix(false);
    if (label) void addBus(label);
  };
  const masterBus = buses.find((bus) => bus.name === MASTER_BUS);
  const extraBuses = buses.filter((bus) => bus.name !== MASTER_BUS);

  return (
    <div className="content">
      <div className="screen-scroll" style={{ padding: 0 }}>
        <div className="mix-scroll">
          <div className="mix-canvas">
          {backendNative !== false && masterBus && (
            <>
              <MixGroup
                icon="instant_mix"
                label={t("mixer.group.master")}
                count="1"
                hint={t("mixer.group.masterHint")}
                onAdd={extraBuses.length < MAX_BUSES ? () => setAddingMix(true) : undefined}
                addTitle={t("mixer.group.addMix")}
              >
                <BusStrip bus={masterBus} onManageProfiles={onOpenProfiles} />
              </MixGroup>
              <div className="group-div" />
            </>
          )}

          <MixGroup
            icon="apps"
            label={t("mixer.group.channels")}
            count={`${channels.length}`}
            hint={t("mixer.group.channelsHint")}
            onAdd={channels.length < MAX_CHANNELS ? () => setAddingChannel(true) : undefined}
            addTitle={t("mixer.group.addChannel")}
          >
            {channels.map((channel) => (
              <ChannelStrip
                key={channel.name}
                channel={channel}
                appCount={counts.get(channel.name) ?? 0}
                dragging={draggingChannel === channel.name}
                onGripDragStart={(e) => {
                  setDraggingChannel(channel.name);
                  e.dataTransfer.effectAllowed = "move";
                  e.dataTransfer.setData("text/plain", channel.name);
                }}
                onGripDragEnd={() => {
                  setDraggingChannel(null);
                  void commitChannelOrder();
                }}
                onStripDragOver={(e) => {
                  if (draggingChannel && draggingChannel !== channel.name) {
                    e.preventDefault();
                    moveChannel(draggingChannel, channel.name);
                  }
                }}
                onOpenSettings={() => onOpenChannel(channel.name)}
              />
            ))}
          </MixGroup>

          {micConfig && (
            <>
              <div className="group-div" />
              <MixGroup
                icon="mic"
                label={t("mixer.group.microphone")}
                count={multipleMics ? `${micConfigs.filter((mic) => mic.enabled).length}/${micConfigs.length}` : (micConfig.enabled ? "1" : t("settings.meters.off.label"))}
                hint={micConfig.enabled
                  ? t("mixer.group.microphoneHint")
                  : t("mixer.group.microphoneDisabledHint")}
                onAdd={multipleMics ? () => {
                  selectMic("sink_mic");
                  setMicCreationOpen(true);
                  onOpenMic();
                } : undefined}
                addTitle={t("microphone.addChannel")}
              >
                {(multipleMics ? micConfigs : [micConfig]).map((mic) => (
                  <MicStrip
                    key={mic.node_name}
                    config={mic}
                    dragging={draggingMic === mic.node_name}
                    onGripDragStart={(event) => {
                      setDraggingMic(mic.node_name);
                      event.dataTransfer.effectAllowed = "move";
                      event.dataTransfer.setData("text/plain", mic.node_name);
                    }}
                    onGripDragEnd={() => {
                      setDraggingMic(null);
                      void commitMicChannelOrder();
                    }}
                    onStripDragOver={(event) => {
                      if (draggingMic && draggingMic !== mic.node_name) {
                        event.preventDefault();
                        moveMicChannel(draggingMic, mic.node_name);
                      }
                    }}
                    onOpenSettings={() => {
                      selectMic(mic.node_name);
                      onOpenMic();
                    }}
                  />
                ))}
              </MixGroup>
            </>
          )}

          {/* Mixes need the native backend; hide them on the pactl
           * fallback instead of showing strips that can't work. */}
          {backendNative !== false && extraBuses.length > 0 && (
          <>
          <div className="group-div" />
          <MixGroup
            icon="podcasts"
            label={t("mixer.group.mixes")}
            count={`${extraBuses.length}`}
            hint={t("mixer.group.mixesHint")}
          >
            {extraBuses.map((bus) => (
              <BusStrip key={bus.name} bus={bus} onManageProfiles={onOpenProfiles} />
            ))}
          </MixGroup>
          </>
          )}
          </div>
        </div>
      </div>

      <Modal open={addingChannel} onClose={closeChannelModal} title={t("mixer.channel.create.title")}>
        <input
          className="menu-input"
          placeholder={t("mixer.channel.create.placeholder")}
          value={channelLabel}
          autoFocus
          maxLength={24}
          onChange={(e) => setChannelLabel(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") createChannel();
          }}
        />
        <div className="modal-label">{t("mixer.channel.create.icon")}</div>
        <div className="icon-grid">
          {ICON_CHOICES.map((choice) => (
            <button
              type="button"
              key={choice}
              className={"icon-cell" + (choice === channelIcon ? " sel" : "")}
              onClick={() => setChannelIcon(choice)}
              aria-label={choice}
            >
              <Ms name={choice} />
            </button>
          ))}
        </div>
        <div className="modal-label">{t("mixer.channel.create.format")}</div>
        <div className="channel-format-choices">
          <button
            type="button"
            className={"channel-format-choice" + (!channelSpatial ? " selected" : "")}
            onClick={() => setChannelSpatial(false)}
          >
            <Ms name="speaker_group" />
            <span><strong>{t("mixer.channel.create.stereo")}</strong><small>{t("mixer.channel.create.stereoHint")}</small></span>
          </button>
          <button
            type="button"
            className={"channel-format-choice" + (channelSpatial ? " selected" : "")}
            onClick={() => setChannelSpatial(true)}
          >
            <Ms name="spatial_audio" />
            <span><strong>{t("mixer.channel.create.spatial")}</strong><small>{t("mixer.channel.create.spatialHint")}</small></span>
          </button>
        </div>
        <div className="modal-btns">
          <button type="button" className="modal-btn primary" onClick={createChannel} disabled={!channelLabel.trim()}>
            {t("mixer.channel.create.action")}
          </button>
          <button type="button" className="modal-btn" onClick={closeChannelModal}>
            {t("common.action.cancel")}
          </button>
        </div>
      </Modal>

      <Modal open={addingMix} onClose={() => setAddingMix(false)} title={t("mixer.mix.create.title")}>
        <p className="modal-text">
          {t("mixer.mix.create.body")}
        </p>
        <input
          className="menu-input"
          placeholder={t("mixer.mix.create.placeholder")}
          value={mixLabel}
          autoFocus
          maxLength={24}
          onChange={(e) => setMixLabel(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") createMix();
          }}
        />
        <div className="modal-btns">
          <button type="button" className="modal-btn primary" onClick={createMix} disabled={!mixLabel.trim()}>
            {t("mixer.mix.create.action")}
          </button>
          <button type="button" className="modal-btn" onClick={() => setAddingMix(false)}>
            {t("common.action.cancel")}
          </button>
        </div>
      </Modal>
    </div>
  );
}
