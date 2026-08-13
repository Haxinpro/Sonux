import { useState } from "react";
import { useMixerStore } from "../../store/mixer";
import { MenuItem } from "../MenuItem";
import { Ms } from "../Icons";
import { Popover } from "../Popover";

function inputIcon(description: string): string {
  const value = description.toLowerCase();
  if (value.includes("headset")) return "headset_mic";
  if (value.includes("webcam") || value.includes("camera")) return "videocam";
  return "mic";
}

export function InputSelect({
  value,
  onChange,
}: Readonly<{
  value: string | null;
  onChange: (inputName: string | null) => void;
}>) {
  const [open, setOpen] = useState(false);
  const inputDevices = useMixerStore((state) => state.inputDevices);
  const current = value === null ? null : inputDevices.find((device) => device.name === value);
  const label = value === null ? "System default" : current?.description ?? value;
  const shortLabel = value === null ? "Default" : label.split(" ")[0];

  return (
    <div className="strip-input-select" style={{ position: "relative" }}>
      <button
        type="button"
        className="strip-route strip-route-btn"
        onClick={() => setOpen((shown) => !shown)}
        title={`Microphone input: ${label}`}
      >
        <Ms name={current ? inputIcon(current.description) : "mic"} />
        <span className="strip-route-name">{shortLabel}</span>
        <Ms name="expand_more" />
      </button>
      <Popover open={open} onClose={() => setOpen(false)} side="top" align="center">
        <MenuItem
          icon="mic"
          selected={value === null}
          showCheck
          onClick={() => {
            onChange(null);
            setOpen(false);
          }}
        >
          System default
        </MenuItem>
        {inputDevices.map((device) => (
          <MenuItem
            key={device.name}
            icon={inputIcon(device.description)}
            selected={device.name === value}
            showCheck
            onClick={() => {
              onChange(device.name);
              setOpen(false);
            }}
          >
            {device.description}
          </MenuItem>
        ))}
      </Popover>
    </div>
  );
}
