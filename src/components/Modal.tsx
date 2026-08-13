import { useId, useLayoutEffect, useRef } from "react";
import type { ReactNode } from "react";
import { createPortal } from "react-dom";
import { IconButton } from "./IconButton";

function focusableElements(dialog: HTMLElement): HTMLElement[] {
  return [...dialog.querySelectorAll<HTMLElement>(
    'button, [href], input, select, textarea, [contenteditable="true"], [tabindex]'
  )].filter((element) => {
    if (element.tabIndex < 0 || element.matches(":disabled")) return false;
    if (element.closest('[hidden], [aria-hidden="true"], [inert]')) return false;
    for (let current: HTMLElement | null = element; current; current = current.parentElement) {
      const style = window.getComputedStyle(current);
      if (style.display === "none" || style.visibility === "hidden") return false;
      if (current === dialog) break;
    }
    return true;
  });
}

/** Centered modal dialog with a dimming scrim. Escape or scrim-click closes. */
export function Modal({
  open,
  onClose,
  title,
  children,
  className,
}: Readonly<{
  open: boolean;
  onClose: () => void;
  title: string;
  children: ReactNode;
  /** Extra class on the dialog (e.g. a width variant). */
  className?: string;
}>) {
  const dialogRef = useRef<HTMLDivElement>(null);
  const titleId = useId();
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  useLayoutEffect(() => {
    if (!open) return;
    const previouslyFocused = document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
    const dialog = dialogRef.current;
    const focusable = () => dialog ? focusableElements(dialog) : [];
    // React's `autoFocus` runs before passive effects. Respect an intentional
    // child target instead of always stealing focus back to the close button.
    if (!dialog?.contains(document.activeElement)) (focusable()[0] ?? dialog)?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onCloseRef.current();
      } else if (e.key === "Tab" && dialog) {
        const items = focusable();
        if (items.length === 0) {
          e.preventDefault();
          dialog.focus();
          return;
        }
        const first = items[0];
        const last = items[items.length - 1];
        if (e.shiftKey && (document.activeElement === first || !dialog.contains(document.activeElement))) {
          e.preventDefault();
          last.focus();
        } else if (!e.shiftKey && (document.activeElement === last || !dialog.contains(document.activeElement))) {
          e.preventDefault();
          first.focus();
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      if (previouslyFocused?.isConnected) previouslyFocused.focus();
    };
  }, [open]);

  // Runs after the open/close lifecycle effect on every render. If a focused
  // conditional child disappeared during reconciliation, recover containment.
  useLayoutEffect(() => {
    if (!open) return;
    const dialog = dialogRef.current;
    if (dialog && !dialog.contains(document.activeElement)) {
      (focusableElements(dialog)[0] ?? dialog).focus();
    }
  });

  if (!open) return null;
  return createPortal(
    <div className="modal-scrim" onClick={onClose}>
      <div
        ref={dialogRef}
        className={"modal" + (className ? ` ${className}` : "")}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="modal-head">
          <div className="modal-title" id={titleId}>{title}</div>
          <IconButton boxed icon="close" title="Close" onClick={onClose} size={18} />
        </div>
        {children}
      </div>
    </div>,
    document.body,
  );
}
