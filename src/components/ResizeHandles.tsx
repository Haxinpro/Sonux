import { getCurrentWindow } from "@tauri-apps/api/window";

type ResizeDirection =
  | "East"
  | "North"
  | "NorthEast"
  | "NorthWest"
  | "South"
  | "SouthEast"
  | "SouthWest"
  | "West";

const DIRECTIONS: { direction: ResizeDirection; className: string }[] = [
  { direction: "North", className: "resize-n" },
  { direction: "NorthEast", className: "resize-ne" },
  { direction: "East", className: "resize-e" },
  { direction: "SouthEast", className: "resize-se" },
  { direction: "South", className: "resize-s" },
  { direction: "SouthWest", className: "resize-sw" },
  { direction: "West", className: "resize-w" },
  { direction: "NorthWest", className: "resize-nw" },
];

/** Visible-window-edge hit areas for reliable resizing of the frameless window. */
export function ResizeHandles() {
  const window = getCurrentWindow();
  return (
    <>
      {DIRECTIONS.map(({ direction, className }) => (
        <div
          key={direction}
          className={`resize-handle ${className}`}
          onPointerDown={(event) => {
            if (event.button === 0) void window.startResizeDragging(direction);
          }}
        />
      ))}
    </>
  );
}
