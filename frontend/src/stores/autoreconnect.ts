/* Auto-reconnect daemon.
 *
 * What it does
 * ────────────
 *  - Watches `connectionState()` and the browser's `online` / `offline`
 *    events.
 *  - When sing-box transitions to `failed` while a user-intended
 *    connection is in flight (i.e. `lastConnectArgs()` is non-null),
 *    schedules a retry with exponential backoff.
 *  - When the OS reports the network came back (`window.online` event),
 *    cancels any pending backoff and tries to reconnect immediately
 *    after a small grace period (let DHCP / DNS resume).
 *  - When the OS goes offline, cancels pending retries — there's no
 *    point burning attempts while the link is down. We'll wake on the
 *    next `online`.
 *  - When sing-box reaches `connected`, resets the attempt counter so
 *    the next failure starts the backoff fresh.
 *  - When the user explicitly clicks Disconnect, `lastConnectArgs()`
 *    becomes `null` (handled by `connection.ts`), and the daemon
 *    refuses to retry.
 *
 * Why frontend
 * ────────────
 *  Auto-reconnect is a user-policy decision (toggle in Settings), not
 *  a supervisor-internal invariant. Keeping it in the renderer means:
 *    - Toggle persistence is trivial (`localStorage`).
 *    - We can use the browser's own connectivity events, which are
 *      already debounced and cross-platform.
 *    - The supervisor stays single-purpose: "run sing-box when asked,
 *      stop when asked".
 *
 * Persistence
 * ───────────
 *  The user's choice is stored under `v2pn:autoReconnect`. Default is
 *  ON — that's what nine out of ten desktop VPN users expect after a
 *  Wi-Fi blip.
 */

import { createEffect, createSignal, onCleanup, onMount } from "solid-js";
import {
  connectSubscriptionAction,
  connectionState,
  lastConnectArgs,
} from "./connection";

/* ─── persistence ──────────────────────────────────────────────────── */

const STORAGE_KEY = "v2pn:autoReconnect";

function loadEnabled(): boolean {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw === null) return true; // sensible default
    return raw === "1";
  } catch {
    return true;
  }
}
function persist(v: boolean): void {
  try {
    localStorage.setItem(STORAGE_KEY, v ? "1" : "0");
  } catch {
    /* ignored — toggle still works in-memory for this session */
  }
}

const [enabled, setEnabledSig] = createSignal<boolean>(loadEnabled());
export const autoReconnectEnabled = enabled;

export function setAutoReconnect(next: boolean): void {
  setEnabledSig(next);
  persist(next);
  if (!next) cancelRetry();
}

/* ─── retry status (for UI surfacing) ──────────────────────────────── */

/** What the daemon is doing right now. Surfaced so the UI can show a
 *  subtle indicator instead of a bare `failed` red pill. */
type RetryStatus =
  | { kind: "idle" }
  | { kind: "waiting-network" }
  | { kind: "scheduled"; attempt: number; nextAtMs: number }
  | { kind: "in-flight"; attempt: number };

const [status, setStatus] = createSignal<RetryStatus>({ kind: "idle" });
export const autoReconnectStatus = status;

/* ─── internal state ──────────────────────────────────────────────── */

/** Backoff schedule in seconds. Capped at 2 min so a long outage doesn't
 *  hammer the box once it returns. */
const BACKOFF_SEC = [2, 5, 10, 20, 60, 120];

let retryHandle: number | null = null;
let attemptIdx = 0;

function cancelRetry(): void {
  if (retryHandle != null) {
    window.clearTimeout(retryHandle);
    retryHandle = null;
  }
  setStatus({ kind: "idle" });
}

function scheduleRetry(): void {
  cancelRetry();
  if (!enabled() || !lastConnectArgs()) return;

  if (typeof navigator !== "undefined" && navigator.onLine === false) {
    // No point burning attempts while the link is down — wait for the
    // `online` event to wake us up.
    setStatus({ kind: "waiting-network" });
    return;
  }

  const delaySec = BACKOFF_SEC[Math.min(attemptIdx, BACKOFF_SEC.length - 1)] ?? 120;
  const attempt = attemptIdx + 1;
  attemptIdx = attempt;
  const nextAtMs = Date.now() + delaySec * 1000;
  setStatus({ kind: "scheduled", attempt, nextAtMs });
  retryHandle = window.setTimeout(() => void attemptReconnect(), delaySec * 1000);
}

async function attemptReconnect(): Promise<void> {
  retryHandle = null;
  const args = lastConnectArgs();
  if (!enabled() || !args) {
    setStatus({ kind: "idle" });
    return;
  }
  if (typeof navigator !== "undefined" && navigator.onLine === false) {
    setStatus({ kind: "waiting-network" });
    return;
  }

  // Don't trample an in-flight transition.
  const s = connectionState().state;
  if (s === "starting" || s === "connected" || s === "stopping") {
    setStatus({ kind: "idle" });
    return;
  }

  setStatus({ kind: "in-flight", attempt: attemptIdx });
  try {
    await connectSubscriptionAction(args.profiles, args.selectedId);
    // The state-change effect will reset attemptIdx and clear status
    // when the supervisor reports "connected". If start-up fails, the
    // supervisor will push a "failed" event and the effect re-schedules.
  } catch {
    // The connect call itself rejected (Tauri-level error). Treat the
    // same as a `failed` transition: schedule the next attempt.
    scheduleRetry();
  }
}

/* ─── event handlers ──────────────────────────────────────────────── */

function onOnline(): void {
  if (!enabled() || !lastConnectArgs()) return;
  const s = connectionState().state;
  if (s === "connected" || s === "starting") return;

  // Network just came back. Cancel any pending timer, reset the
  // attempt counter (give it a fresh budget — the previous failures
  // were caused by *no network*, not by anything sing-box did wrong),
  // and try again after a tiny grace period so DHCP / DNS settle.
  cancelRetry();
  attemptIdx = 0;
  setStatus({ kind: "scheduled", attempt: 1, nextAtMs: Date.now() + 1500 });
  retryHandle = window.setTimeout(() => void attemptReconnect(), 1500);
}

function onOffline(): void {
  cancelRetry();
  if (lastConnectArgs() && enabled()) {
    setStatus({ kind: "waiting-network" });
  }
}

/* ─── public hook ─────────────────────────────────────────────────── */

/** Install the auto-reconnect daemon. Call once from the app root —
 *  hooks `connectionState()` reactivity and the browser's network
 *  events. Idempotent on re-mount thanks to onCleanup. */
export function attachAutoReconnect(): void {
  // React to backend state transitions.
  createEffect(() => {
    const s = connectionState();
    switch (s.state) {
      case "connected":
        // Fresh successful connection → reset attempt budget, clear
        // any "scheduled / in-flight" UI status.
        attemptIdx = 0;
        cancelRetry();
        break;
      case "failed":
        // Only retry when the user *intends* to be connected. Manual
        // Disconnect clears `lastConnectArgs`, so this naturally skips
        // user-initiated stops.
        if (enabled() && lastConnectArgs()) {
          scheduleRetry();
        }
        break;
      case "idle":
        // Could be a user stop or could be the natural rest state
        // before a connect. `lastConnectArgs` distinguishes them.
        break;
      // starting / stopping → in-flight, do nothing
    }
  });

  onMount(() => {
    if (typeof window !== "undefined") {
      window.addEventListener("online", onOnline);
      window.addEventListener("offline", onOffline);
    }
  });

  onCleanup(() => {
    if (typeof window !== "undefined") {
      window.removeEventListener("online", onOnline);
      window.removeEventListener("offline", onOffline);
    }
    cancelRetry();
  });
}
