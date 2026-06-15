import type { Locale } from "./ru";

export const en: Locale = {
  app: {
    name: "v2pn",
    tagline: "Proxy client",
    version: "v0.1.0",
  },

  nav: {
    workspace: "Workspace",
    servers: "Servers",
    routing: "Routing",
    logs: "Logs",
    settings: "Settings",
    subscriptions: "Subscriptions",
    addSubscription: "Add subscription",
  },

  common: {
    cancel: "Cancel",
    confirm: "Confirm",
    delete: "Delete",
    save: "Save",
  },

  sidebar: {
    removeAria: "Remove subscription",
    removeConfirmTitle: "Remove “{name}”?",
    removeConfirmBody:
      "All its servers will disappear from v2pn. Your provider account is unaffected — you can add it again later. This action cannot be undone.",
    removeConfirmCta: "Remove subscription",
  },

  connection: {
    connect: "Connect",
    disconnect: "Disconnect",
    cancel: "Cancel",
    connected: "Connected",
    disconnected: "Disconnected",
    connecting: "Connecting…",
    stopping: "Stopping…",
    failed: "Failed",
    noServerSelected: "No server selected",
    via: "via",
  },

  subscription: {
    title: "Subscription",
    servers: "{count} servers",
    autoUpdate: "auto-update {hours}h",
    refreshing: "refreshing…",
    expires: "expires {date}",
    refresh: "Refresh",
    refreshDisabled: "Pasted subscriptions can't be refreshed — use Import",
    new: "New",
    ping: "Ping",
    pingHint: "Probe latency to every server",
    refreshTipBody:
      "Re-downloads the server list from the same subscription URL. Locally-added servers are kept.",
    newTipBody:
      "Add another subscription or paste a single vless:// / trojan:// link.",
    pingTipBody:
      "Measures the TCP RTT to the server's :443. Does not tell you whether the REALITY tunnel itself works — only a connected server can show that.",

    modeProxyTip: "PROXY mode",
    modeProxyTipBody:
      "v2pn sets a system-wide HTTP/SOCKS proxy. Browsers (Chrome, Edge, Firefox), Discord, Slack, Steam all honour it. Games and the rare apps that ignore Windows proxy settings will go around.",
    modeTunTip: "TUN mode (full system)",
    modeTunTipBody:
      "Creates a virtual network adapter. Every byte the OS sends goes through it — games, torrents, messaging, everything. Requires administrator rights.",
    modeTunNeedsAdmin:
      "v2pn is running without admin rights — TUN switch is disabled.",
    modeLockedTip: "Mode is locked",
    modeLockedTipBody:
      "Can't change PROXY/TUN while a connection is active. Disconnect first.",

    usageTip: "Subscription traffic",
    usageTipBody:
      "How much of your subscription quota you've used. The bar turns yellow at 85% and red at 100%. The cap and expiry are set by your provider.",

    localTrafficLabel: "Local traffic",
    modeLabel: "Mode",
    autoReconnectLabel: "Auto-reconnect",
    autoReconnectOff: "Off",
    autoReconnectOn: "On",
    autoReconnectOffTip: "Auto-reconnect is off",
    autoReconnectOffTipBody:
      "If the tunnel drops — Wi-Fi blip, sleep, network glitch — v2pn does nothing; you stay disconnected until you click Connect. Pick this when you want to start every session by hand.",
    autoReconnectOnTip: "Auto-reconnect is on",
    autoReconnectOnTipBody:
      "If the tunnel fails or the OS goes offline, v2pn waits for the network to return and restarts sing-box at the same server. Backoff 2s → 5s → 10s → 20s → 1m → 2m, with an instant attempt the moment the OS reports network back. A manual Disconnect always trumps auto-reconnect.",
    localTrafficTunnel: "Tunnel",
    localTrafficDirect: "Direct",
    localTrafficTunnelTip: "Tunnel — all traffic via VPN",
    localTrafficTunnelTipBody:
      "Including banks, government services and sites in your country. Maximum anonymisation, but local services see a foreign IP — bank anti-fraud may freeze a transaction, regional streaming refuses to play, government portals ask you to re-verify your login.",
    localTrafficDirectTip: "Direct — local traffic bypasses the VPN",
    localTrafficDirectTipBody:
      "Domains and IPs of your country (SagerNet dataset, auto-updated) go through your regular internet — banks and government work at full local speed with no anti-fraud blocks. Everything else still goes through the VPN. Country list and custom rules live in Settings → Routing.",
  },

  servers: {
    list: "Servers",
    rtt: "RTT",
    selected: "Selected server",
    offline: "offline",
    notMeasured: "—",
  },

  detail: {
    protocol: "Protocol",
    server: "Server",
    port: "Port",
    transport: "Transport",
    security: "Security",
    sni: "SNI",
    utls: "uTLS",
    latency: "Latency",
    switch: "Switch to this server",
    activeBadge: "Active",
  },

  importDialog: {
    title: "Import subscription",
    subtitle: "Paste a subscription URL or a single share link. Format is detected automatically.",
    tabUrl: "URL",
    tabText: "Text",
    urlLabel: "Subscription URL",
    urlPlaceholder: "https://example.com/sub/UUID",
    urlHint:
      "Supports any panel — Marzban, Marzneshin, Remnawave, 3X-UI, x-ui, sing-box.",
    textLabel: "Configuration",
    textPlaceholder:
      "vless://abc@host:443?...\nvmess://...\ntrojan://...\n\n— or —\n\nbase64 blob, sing-box JSON, Clash YAML",
    textHint:
      "Multiple links accepted (one per line). Comments after {hash} are kept as server names.",
    pasteFromClipboard: "Paste from clipboard",
    cancel: "Cancel",
    import: "Import",
    submitHotkey: "⌘↵",
  },

  empty: {
    title: "No subscriptions yet",
    description:
      "Paste a subscription URL or a single share link {kbd} to get started. v2pn never provides servers — you bring your own.",
    cta: "Import subscription",
    hotkey: "⌘N",
  },

  webappFallback: {
    title: "This subscription is webapp-only",
    subtitle:
      "The provider's panel returned an HTML installer page rather than the raw subscription. We tried every common convention — none worked. Two ways forward.",
    optionATitle: "Option A — open in browser, copy a real link",
    optionAStep1: "Open the panel in your default browser.",
    optionAStep2:
      "Right-click the {connect} button → Copy link address. Or open DevTools {f12} → Network → find a request that returns {vless} / base64 / yaml.",
    optionAStep3: "Paste that actual URL via the Import dialog.",
    openInBrowser: "Open {host} in browser",
    optionBTitle: "Option B — paste a single share link",
    optionBSubtitle:
      "Already have {vless} / trojan / hy2 / tuic from another client (Happ, v2rayN, sing-box)? Paste it here.",
    sourcePrefix: "source:",
  },

  settings: {
    title: "Settings",
    subtitle: "Settings persist for the lifetime of this session.",

    sectionMode: "Connection mode",
    sectionModeHint: "How v2pn captures your traffic.",
    modeProxy: "System proxy",
    modeProxyHint:
      "Sets HTTP/SOCKS proxy in Windows. Per-app: most browsers and modern apps obey it.",
    modeTun: "TUN (full system)",
    modeTunHint:
      "Layer-3 virtual adapter via Wintun. Captures every packet from every app. Requires admin privileges.",

    sectionAutoReconnect: "Auto-reconnect",
    sectionAutoReconnectHint:
      "If the tunnel drops — Wi-Fi reconnects, network glitches, sleep wake-up — v2pn brings it back automatically.",
    autoReconnectLabel: "Reconnect automatically",
    autoReconnectDesc:
      "On: when the tunnel fails or the OS goes offline, v2pn waits for the network to return and restarts sing-box at the same server. Backoff 2s → 5s → 10s → 20s → 1m → 2m, with an instant attempt the moment the OS reports network back. Clicking Disconnect turns auto-reconnect off until the next successful connect.",
    autoReconnectStatusIdle: "Active — waiting on events",
    autoReconnectStatusWaitingNetwork: "Waiting for network",
    autoReconnectStatusScheduled: "Attempt #{n} in {sec}s",
    autoReconnectStatusInFlight: "Reconnecting (attempt #{n})…",

    sectionRouting: "Routing",
    sectionRoutingHint:
      "Which traffic goes through the VPN and which goes direct via your regular internet.",
    routingCountries: "Countries (traffic goes direct)",
    routingCountriesHint:
      "Pick the countries where you use local services — banks, gov, shops will skip the VPN. Domain and IP lists are auto-updated from SagerNet's open dataset.",
    routingCustom: "Custom rules",
    routingCustomHint:
      "One per line: example.com (exact), *.banks.com or .banks.com (suffix), 1.2.3.4, 10.0.0.0/8 (IP/CIDR). Lines starting with # are comments.",
    routingDomains: "domains",
    routingSuffixes: "suffixes",
    routingCidrs: "IP/CIDR",
    routingClean: "No changes",
    routingDirty: "Unsaved changes",
    routingReconnect: "Reconnect after Save to apply the new rules",
    routingSaving: "Saving…",

    sectionPorts: "Network ports",
    sectionPortsHint: "Loopback only. Change requires reconnect.",
    portMixed: "Mixed (SOCKS + HTTP)",
    portClashApi: "Clash API",
    portTun: "TUN interface",

    sectionProtocol: "Protocol",
    sectionProtocolHint: "DNS leak protection and address family.",
    protoIpv6: "IPv6 in tunnel",
    protoStrictDns: "Strict DNS",
    enabled: "enabled",
    disabled: "disabled",
    strictDnsOn: "all queries via proxy",
    strictDnsOff: "split",

    sectionLanguage: "Interface language",
    sectionLanguageHint: "Switch any time.",

    sectionAbout: "About",
    aboutVersion: "App version",
    aboutSingbox: "sing-box",
    aboutWintun: "Wintun",
    openLogs: "Open logs folder",
    copyDiagnostics: "Copy diagnostics",

    sectionRepair: "Repair network",
    sectionRepairHint:
      "If your internet broke after using a VPN — the button below resets everything to a clean state.",
    repairDescription:
      "Forcefully cleans up after any VPN client: stops sing-box, disables the system proxy, removes TUN adapters (ours + Hiddify / NekoBox / Karing / v2raya), flushes DNS and ARP caches, re-registers DHCP. No admin required, no reboot.",
    repairAction: "Repair network",
    repairRunning: "Repairing…",
    repairConfirmTitle: "Repair the network?",
    repairConfirmBody:
      "Your active VPN connection will be terminated and direct internet restored. Use this if a different VPN client left your Wi-Fi or browsers broken.",
    repairConfirmCta: "Run repair",
    repairResult: "Done: {ok} of {total} steps succeeded",

    "repair.stopSingbox": "Stop sing-box",
    "repair.clearProxy": "Clear Windows system proxy",
    "repair.wintunCleanup": "Remove stale TUN adapters",
    "repair.flushDns": "Flush DNS cache (ipconfig /flushdns)",
    "repair.renewDhcp": "Re-register DHCP/DNS",
    "repair.arpFlush": "Flush ARP cache",
    "repair.notifyWininet": "Notify Wininet of new settings",
  },

  logs: {
    title: "Logs",
    autoscroll: "autoscroll",
    clear: "clear",
    waiting: "waiting for sing-box output…",
  },

  bar: {
    error: "error",
  },

  themes: {
    light: "Light",
    dark: "Dark",
    toLight: "Switch to light theme",
    toDark: "Switch to dark theme",
    hotkey: "Ctrl+Shift+L",
  },

  comingSoon: "Coming soon",

  admin: {
    required: "Administrator rights required",
    tunNeedsAdmin:
      "TUN mode uses the Wintun virtual network adapter, which Windows only lets administrators configure. Restart v2pn as administrator to enable TUN.",
    restart: "Restart as administrator",
    notNow: "Not now",
    badge: "admin",
  },
};
