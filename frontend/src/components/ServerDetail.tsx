/* Right-hand panel that surfaces the *currently selected* server's
 * config and offers the primary Connect / Disconnect / Switch action.
 *
 * Layout
 * ──────
 *   ┌──────────────────────────────────┐
 *   │  flag  Server name        88 ms  │  ← header
 *   │  • Активен / Выбранный сервер    │
 *   ├──────────────────────────────────┤
 *   │  Protocol     [VLESS]            │
 *   │  Server       153.76.122.202     │  ← field rows (chips for tokens)
 *   │  Port         443                │
 *   │  …                               │
 *   ├──────────────────────────────────┤
 *   │  [   Подключиться (primary)   ]  │  ← sticky action footer
 *   └──────────────────────────────────┘
 *
 * Action button states
 * ────────────────────
 *   - state=connected & this profile is active  → Disconnect (danger)
 *   - state=connected & a different profile     → Switch to this server
 *   - state=starting / stopping                  → spinner + label, disabled
 *   - state=idle / failed                        → Connect (primary)
 */

import { Component, Show } from "solid-js";
import { Power, ArrowRightLeft, Loader2 } from "lucide-solid";
import { Flag } from "./Flag";
import type { ConnectionState, ProxyProfile } from "~/lib/api";
import { activeProfile, pingFor, probing } from "~/stores/connection";
import { t } from "~/lib/i18n";
import { displayName, inferCountryCode } from "~/lib/format";

interface Props {
  profile: ProxyProfile;
  connection: ConnectionState;
  onConnect: (p: ProxyProfile) => void;
  onDisconnect: () => void;
}

export const ServerDetail: Component<Props> = (props) => {
  const cc = () => inferCountryCode(props.profile.name, props.profile.country_code);
  const name = () =>
    displayName(props.profile.name) || `${props.profile.server}:${props.profile.port}`;

  const isActive = () =>
    props.connection.state === "connected" &&
    activeProfile()?.id === props.profile.id;

  const security = () =>
    props.profile.tls.reality
      ? "REALITY"
      : props.profile.tls.enabled
      ? "TLS"
      : "PLAIN";

  const ping = () => pingFor(props.profile.id);

  return (
    <aside class="anim-side-enter flex w-[320px] shrink-0 flex-col border-l border-[var(--color-line)] bg-[var(--color-bg-0)]">
      {/* Header: flag · name · live ping · status. */}
      <header class="border-b border-[var(--color-line)] px-5 py-4">
        <div class="flex items-start gap-3">
          <Flag code={cc()} size={22} class="mt-0.5 shrink-0" />
          <div class="min-w-0 flex-1">
            <div class="truncate text-[14px] font-medium text-[var(--color-fg-0)]">
              {name()}
            </div>
            <div class="mt-1 flex items-center gap-1.5 text-[11px] text-[var(--color-fg-2)]">
              <span
                class="dot"
                data-state={
                  isActive()
                    ? "connected"
                    : props.connection.state === "starting" &&
                      activeProfile()?.id === props.profile.id
                    ? "connecting"
                    : "idle"
                }
                aria-hidden="true"
              />
              <span>
                {isActive() ? t("detail.activeBadge") : t("servers.selected")}
              </span>
            </div>
          </div>

          {/* Live latency chip — same colour scale as ServerRow's RTT. */}
          <PingChip rtt={ping()} probing={probing()} />
        </div>
      </header>

      {/* Field grid. Values are rendered as small monospace chips so the
        * panel reads as a structured config, not as prose. */}
      <dl class="grid grid-cols-[88px_1fr] gap-y-2.5 overflow-y-auto px-5 py-4 text-[12px]">
        <Field label={t("detail.protocol")}>
          <Chip tone="accent">{props.profile.protocol.toUpperCase()}</Chip>
        </Field>
        <Field label={t("detail.server")}>
          <Mono>{props.profile.server}</Mono>
        </Field>
        <Field label={t("detail.port")}>
          <Mono>{props.profile.port}</Mono>
        </Field>
        <Field label={t("detail.transport")}>
          <Chip>{props.profile.transport.type.toUpperCase()}</Chip>
        </Field>
        <Field label={t("detail.security")}>
          <Chip tone={props.profile.tls.reality ? "accent" : "default"}>
            {security()}
          </Chip>
        </Field>
        <Show when={props.profile.tls.server_name}>
          <Field label={t("detail.sni")}>
            <Mono class="truncate">{props.profile.tls.server_name}</Mono>
          </Field>
        </Show>
        <Show when={props.profile.tls.utls_fingerprint}>
          <Field label={t("detail.utls")}>
            <Mono>{props.profile.tls.utls_fingerprint}</Mono>
          </Field>
        </Show>
      </dl>

      {/* Sticky footer: primary action. Pushed to the bottom by mt-auto. */}
      <div class="mt-auto border-t border-[var(--color-line)] px-5 py-3">
        <ActionButton
          connection={props.connection}
          isActive={isActive()}
          onConnect={() => props.onConnect(props.profile)}
          onDisconnect={props.onDisconnect}
        />
      </div>
    </aside>
  );
};

/* ─────────────────────────────────────────────────────────────────────── */

const Field: Component<{ label: string; children: any }> = (p) => (
  <>
    <dt class="self-center text-[var(--color-fg-2)]">{p.label}</dt>
    <dd class="min-w-0 self-center text-[var(--color-fg-0)]">{p.children}</dd>
  </>
);

const Chip: Component<{ children: any; tone?: "default" | "accent" }> = (p) => (
  <span
    class="inline-flex items-center rounded border px-1.5 py-px font-mono text-[10.5px] uppercase tracking-wider"
    classList={{
      "border-[var(--color-line)] bg-[var(--color-bg-1)] text-[var(--color-fg-1)]":
        p.tone !== "accent",
      "border-[color-mix(in_srgb,var(--color-accent)_40%,transparent)] bg-[color-mix(in_srgb,var(--color-accent)_12%,transparent)] text-[color-mix(in_srgb,var(--color-accent)_120%,white)]":
        p.tone === "accent",
    }}
  >
    {p.children}
  </span>
);

const Mono: Component<{ children: any; class?: string }> = (p) => (
  <span class={`font-mono text-[11.5px] tabular-nums text-[var(--color-fg-0)] ${p.class ?? ""}`}>
    {p.children}
  </span>
);

const PingChip: Component<{ rtt: number | null | undefined; probing: boolean }> = (p) => {
  const tone = () => {
    const v = p.rtt;
    if (typeof v !== "number") return "muted";
    if (v < 100) return "good";
    if (v < 250) return "ok";
    if (v < 500) return "warn";
    return "bad";
  };

  return (
    <span
      class="shrink-0 rounded-md border px-1.5 py-1 font-mono text-[11px] tabular-nums leading-none"
      classList={{
        "border-[var(--color-line)] text-[var(--color-fg-3)]": tone() === "muted",
        "border-[color-mix(in_srgb,var(--color-good)_30%,transparent)] text-[var(--color-good)]":
          tone() === "good",
        "border-[var(--color-line)] text-[var(--color-fg-1)]": tone() === "ok",
        "border-[color-mix(in_srgb,var(--color-warn)_30%,transparent)] text-[var(--color-warn)]":
          tone() === "warn",
        "border-[color-mix(in_srgb,var(--color-bad)_30%,transparent)] text-[var(--color-bad)]":
          tone() === "bad",
      }}
    >
      <Show when={p.probing && p.rtt === undefined}>…</Show>
      <Show when={p.rtt === null}>{t("servers.offline")}</Show>
      <Show when={typeof p.rtt === "number"}>
        {p.rtt}<span class="text-[var(--color-fg-3)]">ms</span>
      </Show>
      <Show when={!p.probing && p.rtt === undefined}>{t("servers.notMeasured")}</Show>
    </span>
  );
};

/* ─────────────────────────────────────────────────────────────────────── */

const ActionButton: Component<{
  connection: ConnectionState;
  isActive: boolean;
  onConnect: () => void;
  onDisconnect: () => void;
}> = (props) => {
  const state = () => props.connection.state;
  const isConnected = () => state() === "connected";
  const isBusy = () => state() === "starting" || state() === "stopping";

  /* Decide variant + label + click handler from the current state. */
  const action = () => {
    if (isBusy()) {
      return {
        variant: "busy" as const,
        label: state() === "starting" ? t("connection.connecting") : t("connection.stopping"),
        icon: <Loader2 size={14} class="animate-spin" />,
        onClick: () => {},
        disabled: true,
      };
    }
    if (props.isActive) {
      return {
        variant: "danger" as const,
        label: t("connection.disconnect"),
        icon: <Power size={14} />,
        onClick: props.onDisconnect,
        disabled: false,
      };
    }
    if (isConnected()) {
      // Connected, but to a different server — offer hot-swap.
      return {
        variant: "secondary" as const,
        label: t("detail.switch"),
        icon: <ArrowRightLeft size={14} />,
        onClick: props.onConnect,
        disabled: false,
      };
    }
    return {
      variant: "primary" as const,
      label: t("connection.connect"),
      icon: <Power size={14} />,
      onClick: props.onConnect,
      disabled: false,
    };
  };

  return (
    <button
      type="button"
      onClick={action().onClick}
      disabled={action().disabled}
      class="flex h-10 w-full items-center justify-center gap-2 rounded-md text-[13px] font-medium tracking-[-0.005em] disabled:cursor-not-allowed"
      classList={{
        "btn-onyx": action().variant === "primary",
        "tactile border border-[var(--color-line)] bg-[var(--color-bg-1)] text-[var(--color-fg-0)] transition-colors hover:bg-[var(--color-tint-2)]":
          action().variant === "secondary",
        "tactile border border-[color-mix(in_srgb,var(--color-bad)_40%,transparent)] bg-[color-mix(in_srgb,var(--color-bad)_10%,transparent)] text-[var(--color-bad)] transition-colors hover:bg-[color-mix(in_srgb,var(--color-bad)_18%,transparent)]":
          action().variant === "danger",
        "cursor-wait border border-[var(--color-line)] bg-[var(--color-bg-1)] text-[var(--color-fg-2)]":
          action().variant === "busy",
      }}
    >
      {action().icon}
      <span>{action().label}</span>
    </button>
  );
};
