# v2pn

> **Modern, security-first proxy client** for VLESS / REALITY / Hysteria2 / Trojan / Shadowsocks-2022 / VMess / TUIC / AnyTLS / WireGuard / SSH subscriptions.
>
> Rust + Tauri 2 + SolidJS + Tailwind 4. Uses **sing-box** as the protocol core (bundled as a sidecar binary).
>
> Status: **pre-alpha**. Windows-first; macOS / Linux scaffolding is present but not productised.

[![License: GPL v3](https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg)](#license)
![Status](https://img.shields.io/badge/status-pre--alpha-orange)
![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20(WIP)%20%7C%20Linux%20(WIP)-lightgrey)
![Rust](https://img.shields.io/badge/rust-1.91.0-orange?logo=rust)
![Tauri](https://img.shields.io/badge/tauri-2.x-yellow?logo=tauri)

---

## Table of contents

1. [What is v2pn](#what-is-v2pn)
2. [Why another proxy client](#why-another-proxy-client)
3. [Feature matrix](#feature-matrix)
4. [How it works (end-to-end)](#how-it-works-end-to-end)
5. [Architecture](#architecture)
6. [Project layout](#project-layout)
7. [Build & run](#build--run)
8. [Tauri IPC surface](#tauri-ipc-surface)
9. [Configuration & data layout](#configuration--data-layout)
10. [Threat model](#threat-model)
11. [Troubleshooting](#troubleshooting)
12. [FAQ](#faq)
13. [Roadmap](#roadmap)
14. [Contributing](#contributing)
15. [Security disclosure](#security-disclosure)
16. [Glossary](#glossary)
17. [License & acknowledgements](#license)

---

## What is v2pn

v2pn is a desktop GUI in front of [sing-box](https://github.com/SagerNet/sing-box). You point it at a subscription URL, it parses the response into typed proxy profiles, builds a sing-box configuration on the fly, spawns sing-box as a managed sidecar, and routes your traffic through it. When you disconnect — or the app crashes, or the laptop loses power — the system is restored to exactly the state it was in before you connected. No leaked Wintun adapters, no stranded `sing-box.exe`, no stuck system proxy.

Conceptually it is a **safe shell** around a battle-tested protocol engine, with three explicit non-goals:

- It is **not** a VPN protocol. Every protocol comes from sing-box.
- It is **not** a router. There is no DNS hijacking, no kernel module, no driver beyond Wintun.
- It is **not** a subscription marketplace. v2pn doesn't host servers; you bring your own.

It is heavily inspired by [Happ](https://happ.su) and is interoperable with the Happ-style subscription ecosystem (HTML landing pages, Xray-array JSON, base64 URI lists, etc.).

---

## Why another proxy client

Existing GUIs in this space tend to fall into one of two camps:

1. **Quick & dirty** — single-binary tools that shell out to v2ray/sing-box, leak adapters and processes on crash, and treat subscription bytes as trusted JSON.
2. **Heavyweight & opinionated** — full-blown VPN frontends locked to one protocol or one provider.

v2pn aims for a third spot: a **defensive desktop shell** that

- treats every byte of a subscription as hostile until parsed and sanitised,
- treats every system mutation (proxy registry key, TUN adapter, child process) as something that *will* leak unless explicitly accounted for, and
- still looks and feels like a finished consumer app — animations, dark/light theme, EN/RU locales, custom titlebar.

Design priorities, in order:

1. **Don't break the user's machine.** Every system change is RAII-guarded *and* mirrored to disk so the next launch can recover orphan state from a previous crash.
2. **Don't trust the subscription.** Configs are sanitised before sing-box ever sees them. Loopback ports are typed, TUN interface names are validated, addresses and SNIs are bounds-checked.
3. **Look good doing it.** Native Tauri shell, Motion-driven animations, dark/light themes, EN/RU locales.

---

## Feature matrix

### Protocols (via sing-box)

| Protocol     | Variants / notes                                                |
| ------------ | --------------------------------------------------------------- |
| VLESS        | + REALITY, + uTLS, + Vision flow                                |
| VMess        | AEAD (`alterId=0`), legacy `alterId>0` accepted at parse time   |
| Trojan       | TLS, REALITY                                                    |
| Shadowsocks  | classic methods + **SS-2022** (`2022-blake3-aes-128/256-gcm`)   |
| Hysteria2    | password + optional Salamander obfs                             |
| TUIC v5      | uuid + password, configurable congestion control               |
| AnyTLS       | password                                                       |
| WireGuard    | private/peer keys, optional PSK, MTU, multiple local addresses |
| SSH          | password or private-key auth                                    |
| SOCKS / HTTP | with optional username/password                                 |

### Transport layers

`tcp` · `ws` · `grpc` · `httpupgrade` · `xhttp` · `quic`

### Security layers

- **TLS** with ALPN
- **REALITY** (public key + short id + spider X)
- **uTLS fingerprints** (`chrome`, `firefox`, `safari`, `randomized`, …)
- `allow_insecure` is honoured but never the default

### Subscription formats (auto-detected)

The fetcher sniffs the body and dispatches to the right parser:

| Format          | Detection                                                                              |
| --------------- | -------------------------------------------------------------------------------------- |
| URI list        | One or more `vless://` / `vmess://` / `trojan://` / `ss://` / `hy2://` / `tuic://` lines |
| Base64 URI list | Same content, base64-encoded as a single blob                                          |
| sing-box JSON   | Full sing-box config with an `outbounds` array                                         |
| Xray array JSON | Top-level JSON array (Happ / v2rayN dialect)                                           |
| Clash YAML      | Clash / Clash.Meta `proxies:` document                                                 |
| HTML landing    | Remnawave-style page — extracts deep links and probes convention endpoints             |

Subscription metadata (title, traffic counters, expiry, update interval, support URL) is parsed from `subscription-userinfo` / `profile-*` headers when present.

### Connection modes

| Mode             | What it does                                                              | Requires elevation? |
| ---------------- | ------------------------------------------------------------------------- | ------------------- |
| **System proxy** | sing-box exposes a mixed (HTTP + SOCKS) loopback listener; v2pn writes it into the OS proxy registry and restores it on disconnect | no                  |
| **TUN**          | sing-box opens a Wintun adapter for transparent system-wide routing (Windows). UAC is requested if needed                          | yes (Windows)       |

### Reliability layer

- **State guard (RAII + on-disk mirror).** Captures the previous proxy snapshot before applying its own. Restores on `Drop` — even on panic. After a hard kill (BSOD, power loss), the next launch recovers the orphan state file, force-kills the leftover sing-box, and rolls the OS back.
- **Process guard.** Sidecar runs in a Windows **Job Object** with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, so it is reaped automatically when v2pn exits — including hard exits.
- **Watchdog.** Restarts sing-box if it dies unexpectedly while the user is supposed to be connected.
- **Outbound health checks.** Latency probes per server, batched.
- **Suspend / resume awareness.** If you were connected when the laptop went to sleep, v2pn reconnects after wake.
- **Single instance.** Subsequent launches focus the existing window instead of spawning a duplicate (`tauri-plugin-single-instance`).
- **Wintun cleanup.** Stale TUN adapters from prior runs are detected and removed.
- **Pinned binaries.** `scripts/fetch-singbox.ps1` downloads sing-box and wintun and verifies SHA-256 against pinned hashes before placing them.

### UI

- SolidJS + Tailwind 4 (beta) + Motion
- Custom decorated-less titlebar
- Country flags via `flag-icons` + ISO inference from server names
- Subscription cards with traffic / expiry display
- Server list with latency, context menu, ping batches
- Logs view, settings view, import dialog with HTML-fallback flow
- Locales: **English**, **Russian**
- Themes: dark / light

---

## How it works (end-to-end)

A single connect flow, in order:

```
 user pastes URL
       │
       ▼
┌─────────────────────────────────────────────────────────────┐
│ 1. fetch_subscription                                       │
│      reqwest GET with browser-like UA, follows redirects,   │
│      collects body bytes + relevant response headers        │
└─────────────────────────────────────────────────────────────┘
       │
       ▼
┌─────────────────────────────────────────────────────────────┐
│ 2. format::detect                                           │
│      sniffs first 8 KiB → SubscriptionFormat enum:          │
│      UriList | Base64UriList | SingBoxJson | XrayArray |    │
│      ClashYaml | Html | Unknown                             │
└─────────────────────────────────────────────────────────────┘
       │
       ▼
┌─────────────────────────────────────────────────────────────┐
│ 3. parser dispatch                                          │
│      → Vec<ProxyProfile> + SubscriptionMeta                 │
│      Profiles are protocol-neutral; sensitive material      │
│      (UUIDs, passwords, REALITY private keys) lives only    │
│      in memory until persisted.                             │
└─────────────────────────────────────────────────────────────┘
       │
       ▼
┌─────────────────────────────────────────────────────────────┐
│ 4. user picks a server, hits Connect                        │
│      Tauri command `connect` is invoked with the profile    │
│      list + selected id + ConnectionOptions                 │
└─────────────────────────────────────────────────────────────┘
       │
       ▼
┌─────────────────────────────────────────────────────────────┐
│ 5. singbox::config::build                                   │
│      compiles ProxyProfile → sing-box JSON config           │
└─────────────────────────────────────────────────────────────┘
       │
       ▼
┌─────────────────────────────────────────────────────────────┐
│ 6. singbox::sanitize::sanitize_strict                       │
│      walks the JSON tree, rejects fields that wouldn't pass │
│      the spec or would expose the local box (bad addresses, │
│      forbidden inbound types, etc.)                         │
└─────────────────────────────────────────────────────────────┘
       │
       ▼
┌─────────────────────────────────────────────────────────────┐
│ 7. ConnectionGuard::acquire                                 │
│      a) snapshot current OS proxy state                     │
│      b) write state.json mirror to APPDATA                  │
│      c) (proxy mode) apply our loopback proxy to the OS     │
│      d) (tun mode) install Wintun adapter                   │
└─────────────────────────────────────────────────────────────┘
       │
       ▼
┌─────────────────────────────────────────────────────────────┐
│ 8. supervisor::start                                        │
│      spawn sing-box.exe as a child, attach it to a Job      │
│      Object, wire stdout/stderr → tracing → log file +      │
│      Tauri event bridge (live frontend log stream)          │
└─────────────────────────────────────────────────────────────┘
       │
       ▼
┌─────────────────────────────────────────────────────────────┐
│ 9. watchdog                                                 │
│      observes the child; if it dies while we're supposed    │
│      to be Connected, restarts it with backoff              │
└─────────────────────────────────────────────────────────────┘
       │
       ▼
   ConnectionState::Connected
```

Disconnect is the same path in reverse, driven by `Drop` on `ConnectionGuard`. If the process is killed before `Drop` runs, recovery on the next launch picks up `state.json`, force-kills the leftover sidecar PID, and restores the snapshot.

---

## Architecture

```
┌──────────────────────────────────────────────────────────────┐
│ Tauri shell (crates/tauri-app)                               │
│  ├─ main.rs        event loop, lifecycle, shutdown cleanup   │
│  └─ commands.rs    17 IPC commands (subscription, connect,   │
│                     switch_server, diagnostics, elevation…)  │
└────────────────────────────┬─────────────────────────────────┘
                             │ tauri::invoke (typed via lib/api.ts)
┌────────────────────────────▼─────────────────────────────────┐
│ Frontend (frontend/)                                         │
│  SolidJS · Tailwind 4 · Motion · @tauri-apps/api             │
│  stores/ ─ connection, subscriptions, elevation              │
│  components/ ─ Sidebar, ConnectionBar, ServerRow, …          │
└──────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────┐
│ app-core (crates/app-core) — pure logic, no UI deps          │
│                                                              │
│  subscription/   fetch + format-sniff + 7 parsers + meta     │
│  singbox/        config builder + strict sanitiser           │
│  supervisor.rs   sidecar lifecycle, IPC, log capture         │
│  state_guard.rs  RAII proxy guard + on-disk recovery         │
│  process_guard   Job-Object-based child reaping (Windows)    │
│  sys_proxy/      Windows / stub backends                     │
│  watchdog.rs     auto-restart on unexpected death            │
│  outbound_health latency probes                              │
│  port_pick.rs    safe loopback port allocation               │
│  state_validator config preflight                            │
│  wintun_cleanup  stale Wintun adapter sweeper                │
│  power.rs        suspend / resume hooks                      │
│  elevation.rs    UAC integrity check + relaunch              │
│  hwid.rs         stable machine id (for keyring)             │
│  profile.rs      protocol-neutral ProxyProfile model         │
│  types.rs        TunInterfaceName, LoopbackPort newtypes     │
└────────────────────────────┬─────────────────────────────────┘
                             │ spawns + signals
                             ▼
                    ┌──────────────────────┐
                    │ sing-box.exe sidecar │   ← bundled, GPL-3
                    │   wintun.dll         │   ← bundled
                    └──────────────────────┘
```

### Key invariants

- Sensitive material (UUIDs, passwords, REALITY private keys) lives only in the in-memory `ProxyProfile` until persisted; on-disk profiles are encrypted with `age` and the key is stashed in the OS keyring (`keyring` crate).
- Loopback ports are a typed `LoopbackPort`, TUN names a typed `TunInterfaceName` — invalid values are unrepresentable at the public API boundary.
- Every sing-box config goes through `singbox::sanitize::sanitize_strict` before being launched.
- Every connection holds a `ConnectionGuard`. Drop → restore. Crash → recover on next launch.
- The sing-box child is wrapped in a Windows Job Object → it can never outlive the parent.

---

## Project layout

```
v2pn/
├─ Cargo.toml                 workspace manifest
├─ rust-toolchain.toml        pinned to 1.91.0 (cookie 0.18 + rustc 1.92 coherence)
├─ package.json               pnpm workspace root, tauri/dev/build scripts
├─ pnpm-workspace.yaml
│
├─ crates/
│  ├─ app-core/               pure-logic crate (lib `app_core`)
│  │  ├─ src/                 see Architecture above
│  │  └─ tests/
│  │     ├─ smoke.rs
│  │     └─ config_check.rs
│  └─ tauri-app/              Tauri 2 binary `v2pn`
│     ├─ src/main.rs
│     ├─ src/commands.rs
│     ├─ tauri.conf.json
│     ├─ capabilities/
│     ├─ icons/
│     └─ binaries/            (gitignored) sing-box.exe + wintun.dll land here
│
├─ frontend/
│  ├─ index.html
│  ├─ vite.config.ts
│  ├─ tsconfig.json
│  └─ src/
│     ├─ main.tsx, App.tsx, styles.css
│     ├─ components/   (15 components)
│     ├─ stores/       connection, subscriptions, elevation
│     ├─ lib/          api.ts (typed invoke), i18n, format, theme
│     └─ locales/      en.ts, ru.ts
│
└─ scripts/
   ├─ fetch-singbox.ps1            verifies SHA-256, drops binaries into place
   └─ generate-placeholder-icons.ps1
```

---

## Build & run

### Prerequisites

| Tool                  | Version                | Notes                                                   |
| --------------------- | ---------------------- | ------------------------------------------------------- |
| **Rust**              | 1.91.0 (pinned)        | rustup auto-installs from `rust-toolchain.toml`         |
| **Node**              | 18+ (any LTS)          |                                                         |
| **pnpm**              | 10.15.1+               | declared as `packageManager` in `package.json`          |
| **MSVC build tools**  | latest VS 2022 build   | required for the Tauri/Rust Windows toolchain           |
| **Windows SDK**       | 10/11                  | for the `windows` crate features used by app-core       |
| **WebView2 runtime**  | system-installed       | preinstalled on Windows 11; install on Windows 10       |

### One-time: fetch the sidecar

The `sing-box.exe` and `wintun.dll` binaries are **not** committed (sing-box is GPL-3 — fetch it yourself, hash-pinned):

```pwsh
pwsh ./scripts/fetch-singbox.ps1
```

Versions and SHA-256 hashes are baked into the script. The download is verified before being placed into `crates/tauri-app/binaries/`. **Do not bump versions blindly** — re-derive the hashes from the upstream release notes and audit the changelog first.

### Dev

```pwsh
pnpm install
pnpm tauri dev
```

This runs the Vite dev server on `http://localhost:5173` and launches the Tauri shell pointed at it. Hot reload works for the frontend; Rust changes trigger a full rebuild + relaunch.

### Release build

```pwsh
pnpm tauri build
```

Output bundles land under `target/release/bundle/`. The release profile uses `lto = "thin"`, `codegen-units = 1`, `panic = "abort"`, `strip = true`.

### Useful scripts

```pwsh
pnpm fmt                                # cargo fmt --all
pnpm --filter v2pn-frontend typecheck   # tsc --noEmit
cargo test -p app-core                  # unit + integration tests
cargo clippy --all-targets              # lints
```

---

## Tauri IPC surface

The frontend talks to Rust over a thin typed wrapper (`frontend/src/lib/api.ts`). Commands exposed:

| Command                     | Purpose                                                       |
| --------------------------- | ------------------------------------------------------------- |
| `subscription_fetch`        | Download + parse a subscription URL → `ParsedSubscription`    |
| `subscription_parse_text`   | Parse a pasted subscription body                              |
| `subscription_parse_uri`    | Parse a single `vless://…` / `vmess://…` / etc. URI           |
| `connect`                   | Connect using an explicit profile list + selected id          |
| `connect_subscription`      | Convenience: fetch URL then connect                           |
| `switch_server`             | Hot-swap the active outbound without dropping the connection  |
| `disconnect`                | Stop sing-box, release the guard, clear suspend intent        |
| `connection_state`          | Current `Idle / Starting / Connected / Stopping / Failed`     |
| `active_server_id`          | Profile id currently routed                                   |
| `set_connection_mode`       | `proxy` ↔ `tun`                                               |
| `get_connection_options`    | Read `ConnectionOptions`                                      |
| `probe_latency_batch`       | Latency probes for a batch of profiles                        |
| `elevation_status`          | Current process integrity / admin status                      |
| `restart_as_admin`          | Relaunch with UAC                                             |
| `open_logs_folder`          | Reveal log directory in Explorer                              |
| `diagnostics`               | Snapshot of internal state (for the bug-report flow)          |
| `ping`                      | Health check                                                  |

All commands return either a typed payload or `CommandError { message: string }`. The frontend listens for live events from the supervisor (state changes, log lines) over Tauri's event bus.

---

## Configuration & data layout

Per-user state lives under the `directories` crate's project dir:

```
%APPDATA%\v2pn\
├─ profiles.sled\          encrypted (age) sled DB of subscriptions / profiles
├─ logs\                   rotating log files (tracing-appender)
└─ state.json              proxy-snapshot mirror used for crash recovery
```

The `age` encryption key is stored in the **OS keyring** (Windows Credential Manager). The keyring service name is derived from a stable HWID so the entry is distinguishable from other apps on the same machine.

`state.json` schema (current `schema = 1`):

```json
{
  "schema": 1,
  "pid": 12345,
  "child_pid": 12346,
  "started_at": 1700000000,
  "touched_proxy": true,
  "applied_proxy": "127.0.0.1:7890",
  "saved": { "...": "previous OS proxy snapshot" }
}
```

On launch, if `state.json` exists and `pid` does not refer to a live v2pn, recovery runs: kill `child_pid` if alive, restore `saved`, delete `state.json`.

---

## Threat model

What v2pn defends against, and what it explicitly does **not**:

| Concern                                              | Stance                                                                                                                              |
| ---------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| **Hostile subscription server**                      | All bytes are sniffed and parsed defensively; sing-box JSON is run through `sanitize_strict` before launch; no shell-out from URL.  |
| **Crashed v2pn leaks system proxy / TUN / sidecar**  | RAII `ConnectionGuard` + on-disk `state.json`; recovery on next launch.                                                              |
| **Other apps on the box read stored credentials**    | Profiles encrypted at rest with `age`; key in OS keyring under HWID-namespaced service.                                              |
| **Malicious sing-box or wintun replacement**         | `fetch-singbox.ps1` verifies SHA-256 against pinned values before placing the binary.                                                |
| **Privilege escalation by manipulated config**       | TUN names typed (`TunInterfaceName`), loopback ports typed (`LoopbackPort`); no path-traversal-shaped values in commands.            |
| **Network-level attacker on the LAN**                | Out of scope: the protocols themselves (TLS, REALITY, etc.) are responsible. v2pn does not weaken them.                              |
| **Compromised host machine / kernel-level attacker** | Out of scope. A compromised OS can read process memory and OS keyring entries.                                                       |
| **State-level traffic analysis**                     | Out of scope. Use REALITY / uTLS fingerprints; success depends on the upstream protocol stack.                                       |

v2pn is a **client-side guard rail**, not a substitute for protocol-level security.

---

## Troubleshooting

| Symptom                                                          | Likely cause / fix                                                                                                                       |
| ---------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| "sing-box binary not found"                                      | `pwsh ./scripts/fetch-singbox.ps1` was never run, or the SHA mismatched. Rerun and check the script output.                              |
| Connect succeeds but no traffic                                  | OS proxy mode: check that no other app (Fiddler, corporate proxy GPO) is overwriting the registry value. Open Internet Options → LAN.    |
| TUN mode asks for UAC every launch                              | Expected when running as a standard user. If you want it to stick, run v2pn as admin once and use `restart_as_admin` on demand.          |
| After a hard reboot, internet doesn't work until v2pn is opened  | Recovery hasn't run yet. Open v2pn — it will detect the orphan `state.json`, kill the leftover sing-box, and restore the OS proxy.       |
| Wintun adapter from a previous run is still in Network Connections | `wintun_cleanup` removes them on launch; if it didn't, run `pnpm tauri dev` once with logs open and check `wintun_cleanup` traces.       |
| `Failed` state with no clear error                              | Open Logs view (or `%APPDATA%\v2pn\logs`); sing-box stderr is captured verbatim there.                                                    |
| Frontend shows old subscription data after re-import            | Subscriptions are deduped by URL; clear the offending one and re-add.                                                                    |
| Build fails on Windows with `cookie 0.18` coherence error       | You're on rustc 1.92+. Run `rustup show` — the toolchain pin to 1.91.0 should kick in if you're inside the workspace.                    |

---

## FAQ

**Q: Is this a VPN?**
No. It is a proxy client that wraps sing-box. The "VPN" feel comes from TUN mode, where sing-box opens a virtual adapter and the OS routes everything through it — but cryptographically it's whatever the upstream protocol provides (VLESS, Hysteria2, WireGuard, …).

**Q: Why bundle sing-box instead of using its config files directly?**
Because the goal is a *safe* GUI. Letting users hand-craft sing-box JSON is fine for power users; v2pn instead generates that JSON from a typed model that the sanitiser can vet. The model is also protocol-neutral — in principle the same `ProxyProfile` could compile to Xray or another engine.

**Q: Does v2pn phone home?**
No. There are no analytics, no auto-update servers, no telemetry endpoints. The only outbound HTTP is the subscription URL you configure (and DNS for it).

**Q: Why GPL-3?**
sing-box is GPL-3. Bundling it as a sidecar requires v2pn to be GPL-3-or-later compatible.

**Q: Will there be a portable build / no-install variant?**
That's the long-term plan. The current bundle is a Tauri MSI; portable will need an installer-less layout for `state.json` and the sled DB (likely `--portable` switch + co-located data dir).

**Q: Can I use my existing sing-box config?**
Yes — paste it into the import dialog. v2pn detects `outbounds: [...]` and treats the file as a sing-box JSON subscription.

**Q: Why a custom titlebar?**
Tauri windows are decorated by the OS; we want a borderless, custom-painted titlebar so dark mode looks coherent on Windows 10/11. The OS chrome is hidden via `decorations: false` in `tauri.conf.json`.

**Q: macOS / Linux — when?**
The crate boundary is platform-agnostic; `sys_proxy` and `wintun_cleanup` already have stub implementations behind `cfg`. A real macOS sys-proxy backend (using `scutil`/`networksetup`) and Linux backends (NetworkManager / `gsettings`) are on the roadmap.

---

## Roadmap

This is **pre-alpha scaffolding**. Approximate state:

- [x] Subscription fetch / sniff / parse pipeline (7 formats)
- [x] sing-box config build + strict sanitiser
- [x] Supervisor + log capture + watchdog + process guard
- [x] State guard (RAII + on-disk recovery)
- [x] Windows system-proxy backend
- [x] TUN-mode plumbing (Wintun adapter + cleanup)
- [x] UAC elevation flow
- [x] Suspend / resume reconnect
- [x] Frontend MVP: subscriptions, server list, logs, settings, EN / RU
- [ ] Routing rules editor
- [ ] Per-app routing
- [ ] macOS / Linux sys-proxy backends
- [ ] Auto-update (Tauri Updater)
- [ ] Signed Windows installer
- [ ] CI release pipeline (GitHub Actions: build, sign, publish)
- [ ] LICENSE file
- [ ] Screenshots / demo GIF in README

---

## Contributing

Contributions are welcome but the project is in flux — open an issue first for non-trivial changes.

Conventions:

- **Rust**: `cargo fmt` + `cargo clippy --all-targets` must pass. Public API in `app-core` should keep the typed-newtype style (`TunInterfaceName`, `LoopbackPort`, etc.) — don't paper over invariants with raw strings.
- **TS/TSX**: `pnpm --filter v2pn-frontend typecheck` must pass. Keep `lib/api.ts` the single source of truth for the IPC surface.
- **i18n**: every user-visible string lives in `frontend/src/locales/{en,ru}.ts`. Both files must be updated in the same PR.
- **Commits**: imperative mood ("add", "fix"), under 70 chars first line. Reference issue numbers when relevant.
- **Sidecar binaries**: never commit `sing-box.exe` / `wintun.dll`. Update `scripts/fetch-singbox.ps1` if a version bump is needed, and pin a fresh SHA-256.

Test locally before opening a PR:

```pwsh
pnpm fmt
cargo test -p app-core
cargo clippy --all-targets
pnpm --filter v2pn-frontend typecheck
pnpm tauri dev   # smoke-test the actual app
```

---

## Security disclosure

If you find a vulnerability — especially something that lets a hostile subscription escape sanitisation, escalate privileges, or leak credentials from the keyring — please **do not** open a public issue. Email the maintainer directly (see commit history) with reproduction steps. A coordinated disclosure window of 30 days is preferred.

Out-of-scope (won't be treated as security issues):

- Issues that require local admin / root.
- Issues that depend on a custom-patched sing-box.
- Cosmetic UI redress that doesn't lead to credential disclosure or system-state leakage.

---

## Glossary

| Term                  | Meaning                                                                                                  |
| --------------------- | -------------------------------------------------------------------------------------------------------- |
| **sing-box**          | The actual protocol engine. Bundled as a sidecar binary.                                                 |
| **REALITY**           | TLS-handshake-mimicry layer used by VLESS, designed to be indistinguishable from a real TLS server.      |
| **uTLS**              | Library that lets a client mimic a specific browser's TLS ClientHello fingerprint.                       |
| **Sidecar**           | A separate binary spawned and managed by the main app. Here: `sing-box.exe`.                             |
| **TUN mode**          | Routing all OS traffic through a virtual network adapter (Wintun on Windows).                            |
| **Mixed listener**    | A loopback listener that speaks both HTTP CONNECT and SOCKS5 on the same port.                           |
| **RAII guard**        | Object whose `Drop` impl performs cleanup, so a panic still releases the resource.                       |
| **Job Object**        | Windows kernel construct that lets a parent process bind children so they're killed when the parent dies. |
| **`age`**             | Modern file-encryption format used here for the on-disk profile DB.                                      |
| **HWID**              | Hardware-derived stable identifier; used to namespace the keyring entry.                                 |

---

## License

**GPL-3.0-or-later.** Forced by the GPL-3 sing-box sidecar. See `LICENSE` (TODO: add).

Bundled third-party binaries:

- [sing-box](https://github.com/SagerNet/sing-box) — GPL-3.0-or-later
- [Wintun](https://www.wintun.net/) — proprietary, free redistribution

---

## Acknowledgements

- [SagerNet/sing-box](https://github.com/SagerNet/sing-box) — protocol core
- [Happ](https://happ.su) — UX inspiration and subscription-format reference
- [Tauri](https://tauri.app), [SolidJS](https://www.solidjs.com), [Tailwind CSS](https://tailwindcss.com), [Motion](https://motion.dev), [WireGuard](https://www.wireguard.com)
