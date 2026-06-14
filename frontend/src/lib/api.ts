// Thin typed wrapper around tauri::invoke commands.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Protocol =
  | "vless" | "vmess" | "trojan" | "shadowsocks"
  | "hysteria2" | "tuic" | "anytls" | "wireguard"
  | "ssh" | "socks" | "http";

export type ProxyProfile = {
  id: string;
  name: string;
  country_code: string | null;
  protocol: Protocol;
  server: string;
  port: number;
  settings: Record<string, unknown>;
  transport: { type: string; [k: string]: unknown };
  tls: {
    enabled: boolean;
    server_name: string | null;
    alpn: string[];
    allow_insecure: boolean;
    utls_fingerprint: string | null;
    reality: { public_key: string; short_id: string | null; spider_x: string | null } | null;
  };
  subscription_id: string | null;
};

export type SubscriptionMeta = {
  title: string | null;
  upload_bytes: number | null;
  download_bytes: number | null;
  total_bytes: number | null;
  expire_at: number | null;
  update_interval_hours: number | null;
  web_page_url: string | null;
  support_url: string | null;
};

export type ParsedSubscription = {
  profiles: ProxyProfile[];
  meta: SubscriptionMeta;
};

export type ConnectionMode = "proxy" | "tun";

export type ConnectionOptions = {
  mode: ConnectionMode;
  mixed_port: number;
  clash_api_port: number;
  ipv6: boolean;
  strict_dns: boolean;
  tun_interface_name: string;
  /** ISO-3166 alpha-2 codes whose traffic skips the VPN. */
  bypass_country_codes: string[];
  /** User-authored bypass rules (one per line). See settings UI. */
  custom_bypass_rules: string[];
};

export type ConnectionState =
  | { state: "idle" }
  | { state: "starting" }
  | { state: "connected" }
  | { state: "failed"; reason: string }
  | { state: "stopping" };

export type LogLine = {
  stream: "stdout" | "stderr";
  text: string;
};

export type PingResult = {
  profile_id: string;
  /** Round-trip in milliseconds; null = unreachable / timeout. */
  rtt_ms: number | null;
};

/** Health probe result for the *currently selected* outbound, emitted by
 *  the backend after every connect / hot-switch and periodically while
 *  Connected. UI uses this to colour the connection-state badge:
 *   - `latency_ms` present and small → 🟢 healthy
 *   - `latency_ms` present and high  → 🟡 slow
 *   - `latency_ms` null → 🔴 unreachable; show `error`. */
export type OutboundHealth = {
  tag: string;
  latency_ms: number | null;
  error: string | null;
  at: number;
};

export type ElevationStatus = {
  elevated: boolean;
  supported: boolean;
};

/** One entry in the network-repair report. */
export type RepairStep = {
  id: string;
  label_key: string;
  ok: boolean;
  detail: string;
  took_ms: number;
};

export type RepairReport = {
  steps: RepairStep[];
  started_at: number;
  finished_at: number;
};

export const api = {
  ping: () => invoke<string>("ping"),

  fetchSubscription: (url: string) =>
    invoke<ParsedSubscription>("subscription_fetch", { url }),
  parseText: (text: string) =>
    invoke<ParsedSubscription>("subscription_parse_text", { text }),
  parseUri: (uri: string) =>
    invoke<ProxyProfile>("subscription_parse_uri", { uri }),

  connect: (profile: ProxyProfile, mode?: ConnectionMode) =>
    invoke<void>("connect", { profile, mode }),
  connectSubscription: (
    profiles: ProxyProfile[],
    selectedId: string,
    mode?: ConnectionMode
  ) =>
    invoke<void>("connect_subscription", {
      profiles,
      selectedId,
      mode,
    }),
  switchServer: (profileId: string) =>
    invoke<void>("switch_server", { profileId }),
  disconnect: () => invoke<void>("disconnect"),
  connectionState: () => invoke<ConnectionState>("connection_state"),
  activeServerId: () => invoke<string | null>("active_server_id"),
  setConnectionMode: (mode: ConnectionMode) =>
    invoke<void>("set_connection_mode", { mode }),
  setRouting: (countryCodes: string[], customRules: string[]) =>
    invoke<void>("set_routing", { countryCodes, customRules }),
  getConnectionOptions: () => invoke<ConnectionOptions>("get_connection_options"),

  probeLatencyBatch: (profiles: ProxyProfile[]) =>
    invoke<PingResult[]>("probe_latency_batch", { profiles }),

  elevationStatus: () => invoke<ElevationStatus>("elevation_status"),
  restartAsAdmin: () => invoke<void>("restart_as_admin"),

  openLogsFolder: () => invoke<string>("open_logs_folder"),
  diagnostics: () => invoke<Record<string, unknown>>("diagnostics"),
  repairNetwork: () => invoke<RepairReport>("repair_network"),
};

export const events = {
  onConnectionState: (cb: (s: ConnectionState) => void): Promise<UnlistenFn> =>
    listen<ConnectionState>("connection-state", (e) => cb(e.payload)),
  onLogLine: (cb: (l: LogLine) => void): Promise<UnlistenFn> =>
    listen<LogLine>("log-line", (e) => cb(e.payload)),
  onOutboundHealth: (cb: (h: OutboundHealth) => void): Promise<UnlistenFn> =>
    listen<OutboundHealth>("outbound-health", (e) => cb(e.payload)),
};
