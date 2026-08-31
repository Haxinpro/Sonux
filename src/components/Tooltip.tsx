import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

/** How long the pointer must rest on an element before its tooltip appears.
 *  Single source of truth for tooltip timing across the whole app. */
export const TOOLTIP_DELAY_MS = 1200;

/** Gap from the anchor, and minimum inset from the viewport edges (px). */
const GAP = 6;
const MARGIN = 8;

interface TooltipContent {
  heading?: string;
  text: string;
}

/**
 * One global tooltip that stands in for WebKitGTK's native `title` popups,
 * which appear instantly and can spill off-screen. It reads the same `title`
 * attributes already on elements, waits {@link TOOLTIP_DELAY_MS}, and clamps
 * itself inside the window. The title is stashed while hovered (and restored
 * on leave) so the native tooltip stays suppressed without losing the
 * accessible name assistive tech reads from it.
 */
export function Tooltip() {
  const [content, setContent] = useState<TooltipContent | null>(null);
  const tipRef = useRef<HTMLDivElement>(null);
  const anchor = useRef<Element | null>(null);
  const timer = useRef<number>();

  useEffect(() => {
    const suppressed = new Map<Element, string>();
    let primary: { el: Element; text: string; heading?: string } | null = null;
    let visible = false;

    const restoreAll = () => {
      for (const [element, title] of suppressed) {
        if (element.getAttribute("title") === "") element.setAttribute("title", title);
      }
      suppressed.clear();
      anchor.current = null;
    };
    const hide = () => {
      window.clearTimeout(timer.current);
      primary = null;
      visible = false;
      restoreAll();
      setContent(null);
    };

    const reconcile = (target: Element | null, immediate = false) => {
      const ancestry: Element[] = [];
      for (let element = target; element; element = element.parentElement) ancestry.push(element);
      const inside = new Set(ancestry);

      for (const [element, title] of [...suppressed]) {
        if (!inside.has(element)) {
          if (element.getAttribute("title") === "") element.setAttribute("title", title);
          suppressed.delete(element);
        }
      }
      // Suppress every titled ancestor, not only the nearest one. Otherwise
      // entering a titled child can revive the parent's native tooltip.
      for (const element of ancestry) {
        const liveTitle = element.getAttribute("title");
        if (liveTitle !== null && liveTitle !== "") {
          suppressed.set(element, liveTitle);
          // Leave the title present but empty to suppress the native tooltip and
          // ensure a later removeAttribute call produces an observable mutation.
          element.setAttribute("title", "");
        }
      }

      let next: typeof primary = null;
      for (const element of ancestry) {
        const detailed = element.getAttribute("data-tooltip-text");
        const text = detailed ?? suppressed.get(element);
        if (text?.trim()) {
          next = {
            el: element,
            text,
            heading: detailed
              ? (element.getAttribute("data-tooltip-title") ?? undefined)
              : undefined,
          };
          break;
        }
      }
      if (!next) {
        window.clearTimeout(timer.current);
        primary = null;
        visible = false;
        anchor.current = null;
        setContent(null);
        return;
      }
      anchor.current = next.el;
      if (primary?.el === next.el && primary.text === next.text && primary.heading === next.heading) {
        if (immediate && !visible) {
          window.clearTimeout(timer.current);
          visible = true;
          setContent({ heading: next.heading, text: next.text });
        }
        return;
      }
      const sameAnchor = primary?.el === next.el;
      primary = next;
      window.clearTimeout(timer.current);
      if (immediate || (sameAnchor && visible)) {
        visible = true;
        setContent({ heading: next.heading, text: next.text });
      } else {
        visible = false;
        setContent(null);
        timer.current = window.setTimeout(() => {
          if (primary === next) {
            visible = true;
            setContent({ heading: next.heading, text: next.text });
          }
        }, TOOLTIP_DELAY_MS);
      }
    };

    const onOver = (event: MouseEvent) => reconcile(event.target as Element | null);

    const onOut = (e: MouseEvent) => {
      const related = e.relatedTarget;
      const focused = document.activeElement;
      const target = related instanceof Element
        ? related
        : focused instanceof Element ? focused : null;
      reconcile(target, target === focused);
    };
    const onFocusIn = (event: FocusEvent) => reconcile(event.target as Element | null, true);
    const onFocusOut = (event: FocusEvent) => reconcile(event.relatedTarget as Element | null, true);
    const onClick = (event: MouseEvent) => reconcile(event.target as Element | null, true);

    const observer = new MutationObserver((records) => {
      for (const record of records) {
        const element = record.target as Element;
        if (!suppressed.has(element)) continue;
        const liveTitle = element.getAttribute("title");
        if (liveTitle === "") continue; // our own suppression mutation
        if (liveTitle === null) {
          // Application code removed the hovered title. Discard its saved value
          // so it is not restored on exit, then try the next tooltip ancestor.
          suppressed.delete(element);
          reconcile(element);
          continue;
        }
        suppressed.set(element, liveTitle);
        element.setAttribute("title", "");
        if (primary?.el === element) reconcile(element);
      }
    });
    observer.observe(document.documentElement, {
      subtree: true,
      attributes: true,
      attributeFilter: ["title"],
    });

    document.addEventListener("mouseover", onOver, true);
    document.addEventListener("mouseout", onOut, true);
    document.addEventListener("focusin", onFocusIn, true);
    document.addEventListener("focusout", onFocusOut, true);
    document.addEventListener("click", onClick, true);
    window.addEventListener("scroll", hide, true);
    document.addEventListener("pointerdown", hide, true);
    window.addEventListener("blur", hide);
    return () => {
      document.removeEventListener("mouseover", onOver, true);
      document.removeEventListener("mouseout", onOut, true);
      document.removeEventListener("focusin", onFocusIn, true);
      document.removeEventListener("focusout", onFocusOut, true);
      document.removeEventListener("click", onClick, true);
      window.removeEventListener("scroll", hide, true);
      document.removeEventListener("pointerdown", hide, true);
      window.removeEventListener("blur", hide);
      observer.disconnect();
      window.clearTimeout(timer.current);
      restoreAll();
    };
  }, []);

  // Position against the anchor and clamp inside the viewport once sized.
  useLayoutEffect(() => {
    const tip = tipRef.current;
    if (content === null || !tip || !anchor.current) return;
    const a = anchor.current.getBoundingClientRect();
    const t = tip.getBoundingClientRect();
    let left = a.left + a.width / 2 - t.width / 2;
    left = Math.max(MARGIN, Math.min(left, window.innerWidth - t.width - MARGIN));
    let top = a.bottom + GAP;
    if (top + t.height > window.innerHeight - MARGIN) top = a.top - t.height - GAP;
    top = Math.max(MARGIN, top);
    tip.style.left = `${Math.round(left)}px`;
    tip.style.top = `${Math.round(top)}px`;
    tip.style.visibility = "visible";
  }, [content]);

  if (content === null) return null;
  const paragraphs = content.text.split(/\n\s*\n/).filter(Boolean);
  return createPortal(
    <div
      ref={tipRef}
      className={"app-tooltip" + (content.heading ? " app-tooltip-info" : "")}
      aria-hidden="true"
      style={{ visibility: "hidden" }}
    >
      {content.heading && <div className="app-tooltip-heading">{content.heading}</div>}
      <div className="app-tooltip-copy">
        {paragraphs.map((paragraph, index) => (
          <p key={`${index}-${paragraph.slice(0, 12)}`}>{paragraph}</p>
        ))}
      </div>
    </div>,
    document.body,
  );
}
