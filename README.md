# v2pn

A modern, secure, beautifully animated proxy client for VLESS / REALITY / Hysteria2 / Trojan / SS-2022 / VMess / TUIC / AnyTLS / WireGuard / SSH subscriptions.

Inspired by Happ, built in Rust + Tauri 2 + SolidJS + Tailwind 4.
Uses **sing-box** as the protocol core (sidecar binary).

## Status

Pre-alpha. Scaffolding stage.

## Architecture

```
Tauri shell (Rust)
├─ Frontend: SolidJS + Tailwind 4 + Motion (WebView)
├─ app-core (crate)
│   ├─ subscription parser (base64 / sing-box JSON / Clash YAML / vless:// URI)
│   ├─ profile store (sled + age + OS keyring)
│   ├─ sing-box config builder + sanitizer
│   ├─ supervisor (sidecar lifecycle)
│   └─ probe (latency, health)
└─ sing-box.exe (bundled sidecar, GPL-3)
```

## Build

```pwsh
pnpm install
pnpm tauri dev
```

## License

GPL-3.0-or-later (forced by sing-box GPL).
