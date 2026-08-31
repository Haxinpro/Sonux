import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { CSSProperties, ReactNode } from "react";
import { createPortal } from "react-dom";

interface PopoverProps {
  id?: string;
  open: boolean;
  onClose: () => void;
  children: ReactNode;
  /** Which side of the anchor to open on. */
  side?: "top" | "bottom";
  /** Horizontal alignment relative to the anchor. */
  align?: "start" | "center" | "end";
  /** Extra styles (e.g. minWidth) merged onto the menu. */
  style?: CSSProperties;
}

const MARGIN = 8;
const GAP = 6;

/**
 * Anchored popover menu, rendered through a portal so it can never be
 * clipped by scroll containers or stack under the nav rail. The anchor is
 * the parent element of the marker span (call sites wrap trigger+Popover
 * in a relative container, which keeps working unchanged).
 */
export function Popover({ id, open, onClose, children, side = "bottom", align = "start", style }: Readonly<PopoverProps>) {
  const markerRef = useRef<HTMLSpanElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState<CSSProperties | null>(null);

  useLayoutEffect(() => {
    if (!open) {
      setPosition(null);
      return;
    }
    const anchor = markerRef.current?.parentElement;
    const menu = menuRef.current;
    if (!anchor || !menu) return;

    const rect = anchor.getBoundingClientRect();
    const menuRect = menu.getBoundingClientRect();
    const vw = window.innerWidth;
    const vh = window.innerHeight;

    let left: number;
    if (align === "center") {
      left = rect.left + rect.width / 2 - menuRect.width / 2;
    } else if ((align === "end") !== (document.documentElement.dir === "rtl")) {
      left = rect.right - menuRect.width;
    } else {
      left = rect.left;
    }
    left = Math.max(MARGIN, Math.min(left, vw - menuRect.width - MARGIN));

    let top: number;
    if (side === "top") {
      top = rect.top - menuRect.height - GAP;
      if (top < MARGIN) top = rect.bottom + GAP; // flip when cramped
    } else {
      top = rect.bottom + GAP;
      if (top + menuRect.height > vh - MARGIN) top = rect.top - menuRect.height - GAP;
    }
    top = Math.max(MARGIN, Math.min(top, vh - menuRect.height - MARGIN));

    setPosition({ left, top });
  }, [open, side, align]);

  // Latest onClose without making it an effect dependency: callers pass an
  // inline arrow, so depending on it would re-run the effect on every parent
  // render and steal focus back into the menu (breaking typing in menu
  // inputs). The effect must run only when `open` flips.
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  // Keyboard handling while open: Escape closes (like the scrim click)
  // and Tab is contained inside the menu so focus can't wander into the
  // UI underneath. Focus moves into the menu on open and back to the
  // trigger on close.
  useEffect(() => {
    if (!open) return;
    const previous = document.activeElement as HTMLElement | null;
    const menu = menuRef.current;
    const selected = menu?.querySelector<HTMLElement>('[aria-checked="true"]');
    (selected ?? menu)?.focus();

    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        onCloseRef.current();
        return;
      }
      if (["ArrowDown", "ArrowUp", "Home", "End"].includes(e.key)) {
        const menu = menuRef.current;
        if (!menu) return;
        const target = e.target as HTMLElement | null;
        if (target?.matches("input, select, textarea, [contenteditable=true]")) return;
        const items = [...menu.querySelectorAll<HTMLElement>('[role^="menuitem"], .menu-item')];
        if (items.length === 0) return;
        const active = document.activeElement as HTMLElement | null;
        if (active !== menu && !items.includes(active as HTMLElement)) return;
        e.preventDefault();
        const current = items.indexOf(active as HTMLElement);
        const next = e.key === "Home" ? 0
          : e.key === "End" ? items.length - 1
            : e.key === "ArrowUp" ? (current <= 0 ? items.length - 1 : current - 1)
              : (current + 1) % items.length;
        items[next].focus();
        return;
      }
      if (e.key !== "Tab") return;
      const menu = menuRef.current;
      if (!menu) return;
      const focusables = menu.querySelectorAll<HTMLElement>(
        'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])',
      );
      if (focusables.length === 0) {
        // Nothing tabbable (item rows are click-driven) - keep focus put.
        e.preventDefault();
        return;
      }
      const first = focusables[0];
      const last = focusables[focusables.length - 1];
      const active = document.activeElement;
      if (e.shiftKey && (active === first || !menu.contains(active))) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && (active === last || !menu.contains(active))) {
        e.preventDefault();
        first.focus();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      previous?.focus?.();
    };
    // Intentionally only `open`: onClose is read via ref (see above).
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  return (
    <>
      <span ref={markerRef} style={{ display: "none" }} aria-hidden="true" />
      {open &&
        createPortal(
          <>
            <div className="scrim" onClick={onClose} />
            <div
              id={id}
              ref={menuRef}
              className="menu"
              role="menu"
              tabIndex={-1}
              style={{
                position: "fixed",
                visibility: position ? "visible" : "hidden",
                ...(position ?? { left: 0, top: 0 }),
                ...style,
              }}
            >
              {children}
            </div>
          </>,
          document.body,
        )}
    </>
  );
}
