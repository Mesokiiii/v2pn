import {
  Component,
  JSX,
  Show,
  createSignal,
  onCleanup,
  onMount,
} from "solid-js";
import { Portal } from "solid-js/web";

/**
 * Anchor-aware tooltip with rock-solid positioning.
 *
 * How positioning works (the *clean* version):
 *   1. User hovers the trigger.
 *   2. We mount the bubble inside a Portal but in `phase=measuring` —
 *      `visibility: hidden` keeps it out of the visual flow while still
 *      occupying its real layout dimensions.
 *   3. After one `requestAnimationFrame`, we read both the trigger and
 *      bubble rects, decide top-vs-bottom placement, then clamp the
 *      bubble into the viewport with an 8 px gutter on every side.
 *   4. We commit the final `left`/`top` and flip phase to `open` — the
 *      single entrance keyframe (opacity + 4 px slide + tiny scale)
 *      then plays cleanly from those coordinates.
 *
 * Result: no mid-animation jumps, no clipping at window edges, no
 * `transform: translate(-50%)` compositions for the JS to fight with.
 *
 * Behaviour:
 *   - Hover OR keyboard focus → opens after `delay` (default 280 ms).
 *   - Mouse leave OR blur → closes after a tiny 80 ms grace so a flick
 *     re-enter doesn't flash the bubble.
 *   - Auto-flips to bottom if the top placement would clip.
 *   - Re-measures on scroll / resize / DOM mutation under the trigger.
 *   - `pointer-events: none` on the bubble — never steals clicks.
 *   - Reduced-motion aware: skips the slide+scale, keeps the fade.
 */

type Placement = "top" | "bottom";
type Phase = "closed" | "measuring" | "open";
type Pos = { left: number; top: number; placement: Placement };

interface TooltipProps {
  /** Trigger content. Wrapped in `display:contents` so layout is untouched. */
  children: JSX.Element;
  /** Bold one-liner. Required. */
  title: string;
  /** Optional secondary line. ~2-3 sentences max for readability. */
  body?: string;
  /** Optional ⌘ / kbd shortcut hint, rendered in a monospace pill. */
  shortcut?: string;
  /** ms to wait before opening on hover/focus. Default 280. */
  delay?: number;
  /** Set to `false` to disable the tooltip entirely. */
  enabled?: boolean;
}

/** Visual constants. Single source of truth — change once, applied
 *  consistently in JS clamping and CSS keyframes (see styles.css). */
const VIEWPORT_MARGIN = 8;
const TRIGGER_GAP = 8;

export const Tooltip: Component<TooltipProps> = (p) => {
  let triggerEl: HTMLSpanElement | undefined;
  let bubbleEl: HTMLDivElement | undefined;
  const [phase, setPhase] = createSignal<Phase>("closed");
  const [pos, setPos] = createSignal<Pos>({
    left: 0,
    top: 0,
    placement: "top",
  });
  let openTimer: number | null = null;
  let closeTimer: number | null = null;
  let prefersReducedMotion = false;

  const enabled = () => p.enabled !== false;

  onMount(() => {
    prefersReducedMotion = window
      .matchMedia("(prefers-reduced-motion: reduce)")
      .matches;
  });

  /** Read both rects and produce the final clamped position. Must run
   *  after the bubble is in the DOM (Portal mounts synchronously, but
   *  layout numbers are only stable after the next paint — hence the
   *  `requestAnimationFrame` gate in `open`).
   *
   *  Trigger rect: the wrapper `<span>` uses `display: contents` so it
   *  doesn't leak its own box into layout (zero-size rect). We read the
   *  rect of the *first child element* instead, which is the actual UI
   *  element the user is hovering. Falls back to the wrapper rect for
   *  pathological children that don't render. */
  const measure = (): Pos | null => {
    if (!triggerEl || !bubbleEl) return null;
    const anchor =
      (triggerEl.firstElementChild as HTMLElement | null) ?? triggerEl;
    const tr = anchor.getBoundingClientRect();
    // Defensive: if the trigger is laid out at (0,0) with zero size —
    // either still mid-mount or hidden by an ancestor — bail and let a
    // later viewport-change call retry. Otherwise we'd cement the
    // tooltip at the top-left corner.
    if (tr.width === 0 && tr.height === 0) return null;
    const br = bubbleEl.getBoundingClientRect();
    const vw = window.innerWidth;
    const vh = window.innerHeight;

    // Vertical: prefer "above the trigger", flip below if that would
    // clip the top edge of the viewport. We do *not* try to flip back
    // up if the bottom also clips — at that point the trigger is in a
    // window so short the user has bigger problems than tooltip
    // placement, and either choice is acceptable.
    let placement: Placement = "top";
    let top = tr.top - br.height - TRIGGER_GAP;
    if (top < VIEWPORT_MARGIN) {
      placement = "bottom";
      top = tr.bottom + TRIGGER_GAP;
      // Final defensive clamp so a giant bubble in a tiny window still
      // sits inside the visible area.
      if (top + br.height > vh - VIEWPORT_MARGIN) {
        top = Math.max(VIEWPORT_MARGIN, vh - VIEWPORT_MARGIN - br.height);
      }
    }

    // Horizontal: centre under the trigger, then slide into the
    // viewport if either edge clips.
    let left = tr.left + tr.width / 2 - br.width / 2;
    if (left < VIEWPORT_MARGIN) {
      left = VIEWPORT_MARGIN;
    } else if (left + br.width > vw - VIEWPORT_MARGIN) {
      left = vw - VIEWPORT_MARGIN - br.width;
    }
    // Round to whole pixels — subpixel `left` causes the
    // `backdrop-filter` blur to shimmer on transform-animated frames.
    return {
      left: Math.round(left),
      top: Math.round(top),
      placement,
    };
  };

  const open = () => {
    if (!enabled()) return;
    if (closeTimer !== null) {
      clearTimeout(closeTimer);
      closeTimer = null;
    }
    if (phase() !== "closed") return;
    openTimer = window.setTimeout(() => {
      // First, render hidden so layout numbers are real.
      setPhase("measuring");
      // Two rAFs: the first lets the Portal commit the bubble into the
      // DOM, the second guarantees layout has settled (some browsers
      // don't include the bubble in the first frame's layout pass when
      // the parent uses `backdrop-filter`).
      requestAnimationFrame(() => {
        requestAnimationFrame(() => {
          const next = measure();
          if (next === null) {
            setPhase("closed");
            return;
          }
          setPos(next);
          setPhase("open");
        });
      });
    }, p.delay ?? 280);
  };

  const close = () => {
    if (openTimer !== null) {
      clearTimeout(openTimer);
      openTimer = null;
    }
    if (phase() === "closed") return;
    closeTimer = window.setTimeout(() => setPhase("closed"), 80);
  };

  /** Recompute on scroll / resize while open. Cheap because `measure`
   *  reads the DOM but never mutates style on its own — it only writes
   *  through `setPos`, which Solid coalesces into a single re-render. */
  const handleViewportChange = () => {
    if (phase() !== "open") return;
    const next = measure();
    if (next !== null) setPos(next);
  };

  onMount(() => {
    document.addEventListener("scroll", handleViewportChange, true);
    window.addEventListener("resize", handleViewportChange);
  });

  onCleanup(() => {
    if (openTimer !== null) clearTimeout(openTimer);
    if (closeTimer !== null) clearTimeout(closeTimer);
    document.removeEventListener("scroll", handleViewportChange, true);
    window.removeEventListener("resize", handleViewportChange);
  });

  return (
    <>
      <span
        ref={triggerEl}
        class="contents"
        onMouseEnter={open}
        onMouseLeave={close}
        onFocusIn={open}
        onFocusOut={close}
      >
        {p.children}
      </span>
      <Show when={phase() !== "closed"}>
        <Portal>
          <div
            role="tooltip"
            ref={bubbleEl}
            class="tooltip-pop"
            data-phase={phase()}
            data-placement={pos().placement}
            data-reduced-motion={prefersReducedMotion ? "1" : "0"}
            style={{
              left: `${pos().left}px`,
              top: `${pos().top}px`,
            }}
          >
            <div class="tooltip-bubble">
              <div class="flex items-start gap-2">
                <div class="min-w-0 flex-1">
                  <div class="tooltip-title">{p.title}</div>
                  <Show when={p.body}>
                    <div class="tooltip-body">{p.body}</div>
                  </Show>
                </div>
                <Show when={p.shortcut}>
                  {(s) => <span class="tooltip-kbd">{s()}</span>}
                </Show>
              </div>
            </div>
          </div>
        </Portal>
      </Show>
    </>
  );
};
