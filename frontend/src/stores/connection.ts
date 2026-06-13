/* Connection state store — single source of truth.
 *
 * Reflects the supervisor state pushed from Rust via the `connection-state`
 * event. Components call `connectAction(profile)` / `disconnectAction()`
 * which returns a Promise; the store auto-updates from the event stream.
 */
import { createSignal, onCleanup, onMount } from "solid-js";
import {
  api,
  events,
  type ConnectionMode,
  type ConnectionOptions,
  type ConnectionState,
  type LogLine,
  type ProxyProfile,
} from "~/lib/api";

const [state, setState] = createSignal<ConnectionState>({ state: "idle" });
const [activeProfile, setActiveProfile] = createSignal<ProxyProfile | null>(null);
const [options, setOptions] = createSignal<ConnectionOptions | null>(null);
const [logs, setLogs] = createSignal<LogLine[]>([]);
const [pings, setPings] = createSignal<Record<string, number | null>>({});
const [probing, setProbing] = createSignal(false);
const MAX_LOGS = 500;

export {
  state as connectionState,
  activeProfile,
  options as connectionOptions,
  logs,
  pings,
  probing,
};

/** Hook the global event stream into our store. Idempotent. */
export function attachConnectionEvents() {
  let unState: (() => void) | undefined;
  let unLogs: (() => void) | undefined;

  onMount(async () => {
    // Initial fetch in case the supervisor was already running.
    try {
      setState(await api.connectionState());
      setOptions(await api.getConnectionOptions());
    } catch (e) {
      console.warn("init connection-state failed", e);
    }

    unState = await events.onConnectionState((s: ConnectionState) => setState(s));
    unLogs = await events.onLogLine((l: LogLine) => {
      setLogs((prev) => {
        const next = [...prev, l];
        return next.length > MAX_LOGS ? next.slice(-MAX_LOGS) : next;
      });
    });
  });

  onCleanup(() => {
    unState?.();
    unLogs?.();
  });
}

export async function connectAction(
  profile: ProxyProfile,
  mode?: ConnectionMode
): Promise<void> {
  setActiveProfile(profile);
  await api.connect(profile, mode);
}

/** Multi-profile connect: starts sing-box with the entire subscription
 * wired up to a `selector` outbound, so subsequent server changes can be
 * hot-swapped via clash_api without a restart. */
export async function connectSubscriptionAction(
  profiles: ProxyProfile[],
  selectedId: string,
  mode?: ConnectionMode
): Promise<void> {
  const picked = profiles.find((p) => p.id === selectedId) ?? profiles[0];
  if (picked) setActiveProfile(picked);
  await api.connectSubscription(profiles, selectedId, mode);
}

export async function disconnectAction(): Promise<void> {
  await api.disconnect();
  setActiveProfile(null);
}

export async function setMode(mode: ConnectionMode): Promise<void> {
  await api.setConnectionMode(mode);
  const opts = await api.getConnectionOptions();
  setOptions(opts);
}

export function clearLogs() {
  setLogs([]);
}

export async function probeAll(profiles: ProxyProfile[]): Promise<void> {
  if (profiles.length === 0) return;
  setProbing(true);
  try {
    const results = await api.probeLatencyBatch(profiles);
    const next: Record<string, number | null> = {};
    for (const r of results) next[r.profile_id] = r.rtt_ms;
    setPings(next);
  } finally {
    setProbing(false);
  }
}

export function pingFor(profileId: string): number | null | undefined {
  return pings()[profileId];
}

export function isConnected(s: ConnectionState | undefined = state()): boolean {
  return s.state === "connected";
}

export function isBusy(s: ConnectionState | undefined = state()): boolean {
  return s.state === "starting" || s.state === "stopping";
}
