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

/* What was the last connect call?
 *
 * Captured so the auto-reconnect daemon can replay it after a network
 * drop without depending on UI state. Cleared whenever the user
 * explicitly disconnects, so a manual disconnect never auto-retries.
 *
 * - `null`        — no intent, do not auto-retry
 * - `{ profiles, selectedId }` — last successful connect call's args
 */
const [lastConnectArgs, setLastConnectArgs] = createSignal<
  { profiles: ProxyProfile[]; selectedId: string } | null
>(null);

const MAX_LOGS = 500;

export {
  state as connectionState,
  activeProfile,
  options as connectionOptions,
  logs,
  pings,
  probing,
  lastConnectArgs,
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
  // Single-profile connect → store as a one-element subscription so the
  // auto-reconnect daemon can replay it the same way.
  setLastConnectArgs({ profiles: [profile], selectedId: profile.id });
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
  setLastConnectArgs({ profiles, selectedId });
  await api.connectSubscription(profiles, selectedId, mode);
}

export async function disconnectAction(): Promise<void> {
  // User-initiated disconnect → forget the last intent so the
  // auto-reconnect daemon doesn't try to bring it back.
  setLastConnectArgs(null);
  await api.disconnect();
  setActiveProfile(null);
}

/** Update the *selected* profile inside the persisted connect intent.
 *
 * Called after a successful clash-API hot-swap (`switch_server`), so the
 * auto-reconnect daemon — should the tunnel later fail — restarts with
 * the most recently chosen server, not the one the user originally
 * connected to. The full subscription profile list is unchanged because
 * a hot-swap never crosses subscriptions. */
export function updateActiveServer(selectedId: string): void {
  const cur = lastConnectArgs();
  if (!cur) return;
  setLastConnectArgs({ profiles: cur.profiles, selectedId });
}

export async function setMode(mode: ConnectionMode): Promise<void> {
  await api.setConnectionMode(mode);
  const opts = await api.getConnectionOptions();
  setOptions(opts);
}

export async function setRouting(
  countryCodes: string[],
  customRules: string[],
): Promise<void> {
  await api.setRouting(countryCodes, customRules);
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
