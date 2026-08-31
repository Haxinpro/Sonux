import { useState } from "react";
import { useMixerStore } from "../../store/mixer";
import { UNASSIGNED } from "../../types";
import { channelIcon, Ms } from "../Icons";
import { MenuItem } from "../MenuItem";
import { Popover } from "../Popover";
import { useI18n } from "../../i18n";

interface ChannelSelectProps {
  /** Currently assigned sink name, or null when unassigned. */
  value: string | null;
  /** Multiple raw identities in one app group currently use different routes. */
  mixed?: boolean;
  onChange: (sinkName: string) => void;
}

/** Dropdown to route an app stream onto a channel. */
export function ChannelSelect({ value, mixed = false, onChange }: Readonly<ChannelSelectProps>) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const channels = useMixerStore((s) => s.channels);

  const current = channels.find((c) => c.name === value);
  // An assignment outlives the channel it names - deleted here, or belonging
  // to a profile that isn't loaded. Reporting that as "Unrouted" would be a
  // lie, and picking Unrouted to confirm it would delete a real assignment.
  const missing = value !== null && !current;

  let icon: string;
  if (mixed) icon = "call_split";
  else if (current) icon = channelIcon(current);
  else if (missing) icon = "link_off";
  else icon = "block";

  let label: string;
  if (mixed) label = t("mixer.output.mixed");
  else if (current) label = current.label;
  else if (missing) label = t("applications.missing");
  else label = t("applications.unrouted");

  return (
    <div style={{ position: "relative" }}>
      <button
        type="button"
        className="select"
        onClick={() => setOpen((o) => !o)}
        title={
          missing
            ? t("applications.missingHint", { channel: value })
            : undefined
        }
      >
        <Ms name={icon} />
        <span style={{ minWidth: 52, textAlign: "left" }}>{label}</span>
        <Ms name="expand_more" />
      </button>
      <Popover open={open} onClose={() => setOpen(false)} side="bottom" align="end">
        {channels.map((c) => (
          <MenuItem
            key={c.name}
            icon={channelIcon(c)}
            selected={c.name === value}
            showCheck
            onClick={() => {
              onChange(c.name);
              setOpen(false);
            }}
          >
            {c.label}
          </MenuItem>
        ))}
        <MenuItem
          icon="block"
          selected={!mixed && value === null}
          showCheck
          onClick={() => {
            onChange(UNASSIGNED);
            setOpen(false);
          }}
        >
          {t("applications.unrouted")}
        </MenuItem>
      </Popover>
    </div>
  );
}
