import { useState } from "react";
import type { ReactNode } from "react";
import { useMixerStore } from "../../store/mixer";
import { MASTER_BUS } from "../../types";
import { Ms, ICON_CHOICES } from "../Icons";
import { Modal } from "../Modal";
import { ChannelStrip } from "./ChannelStrip";
import { MicStrip } from "./MicStrip";
import { BusStrip } from "./StreamMixStrip";

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

export function MixerBoard({ onOpenChannel }: Readonly<{ onOpenChannel: (name: string) => void }>) {
  const channels = useMixerStore((s) => s.channels);
  const buses = useMixerStore((s) => s.buses);
  const appStreams = useMixerStore((s) => s.appStreams);
  const addChannel = useMixerStore((s) => s.addChannel);
  const addBus = useMixerStore((s) => s.addBus);
  const micConfig = useMixerStore((s) => s.micConfig);
  const backendNative = useMixerStore((s) => s.backendNative);

  const moveChannel = useMixerStore((s) => s.moveChannel);
  const commitChannelOrder = useMixerStore((s) => s.commitChannelOrder);

  const [addingChannel, setAddingChannel] = useState(false);
  const [channelLabel, setChannelLabel] = useState("");
  const [channelIcon, setChannelIcon] = useState(ICON_CHOICES[0]);
  const [addingMix, setAddingMix] = useState(false);
  const [mixLabel, setMixLabel] = useState("");
  const [draggingChannel, setDraggingChannel] = useState<string | null>(null);

  if (channels.length === 0) {
    return (
      <div className="content">
        <div className="empty-hint" style={{ margin: "auto" }}>
          Creating virtual channels…
        </div>
      </div>
    );
  }

  // The strip is a live mixer view: its count includes only identities that
  // currently own a PipeWire stream. Remembered/offline assignments remain
  // available in the channel membership popover and Applications screen.
  const identitiesByChannel = new Map<string, Set<string>>();
  for (const stream of appStreams) {
    const key = `${stream.match_prop}\0${stream.match_value}`;
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
  };
  const createChannel = () => {
    const label = channelLabel.trim();
    if (!label) return;
    void addChannel(label, channelIcon);
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
          {backendNative !== false && masterBus && (
            <>
              <MixGroup
                icon="instant_mix"
                label="Master"
                count="1"
                hint="The master profile and a capturable mix containing every playback channel."
                onAdd={extraBuses.length < MAX_BUSES ? () => setAddingMix(true) : undefined}
                addTitle="Add a mix (capturable source for OBS/recorders)"
              >
                <BusStrip bus={masterBus} />
              </MixGroup>
              <div className="group-div" />
            </>
          )}

          <MixGroup
            icon="apps"
            label="Channels"
            count={`${channels.length}`}
            hint="Playback: apps route into channels; each has its own volume, mute and output device."
            onAdd={channels.length < MAX_CHANNELS ? () => setAddingChannel(true) : undefined}
            addTitle="Add a channel"
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

          {micConfig?.enabled && (
            <>
              <div className="group-div" />
              <MixGroup
                icon="mic"
                label="Mic"
                count="1"
                hint="Your processed microphone. Apps capture the result as Sonar mic."
              >
                <MicStrip />
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
            label="Mixes"
            count={`${extraBuses.length}`}
            hint="Recordable copies of your channels. In OBS, add a mix as an audio input (mic/aux) - not Desktop Audio."
          >
            {extraBuses.map((bus) => (
              <BusStrip key={bus.name} bus={bus} />
            ))}
          </MixGroup>
          </>
          )}
        </div>
      </div>

      <Modal open={addingChannel} onClose={closeChannelModal} title="New channel">
        <input
          className="menu-input"
          placeholder="Channel name…"
          value={channelLabel}
          autoFocus
          maxLength={24}
          onChange={(e) => setChannelLabel(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") createChannel();
          }}
        />
        <div className="modal-label">Icon</div>
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
        <div className="modal-btns">
          <button type="button" className="modal-btn primary" onClick={createChannel} disabled={!channelLabel.trim()}>
            Create channel
          </button>
          <button type="button" className="modal-btn" onClick={closeChannelModal}>
            Cancel
          </button>
        </div>
      </Modal>

      <Modal open={addingMix} onClose={() => setAddingMix(false)} title="New mix">
        <p className="modal-text">
          A mix is a capturable source: pick which channels it carries, then
          select it by name in OBS or any recorder.
        </p>
        <input
          className="menu-input"
          placeholder="Mix name…"
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
            Create mix
          </button>
          <button type="button" className="modal-btn" onClick={() => setAddingMix(false)}>
            Cancel
          </button>
        </div>
      </Modal>
    </div>
  );
}
