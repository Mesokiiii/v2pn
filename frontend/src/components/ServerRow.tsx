import { Component, Show } from "solid-js";
import type { ProxyProfile } from "~/lib/api";
import { displayName, inferCountryCode } from "~/lib/format";
import { pingFor, probing } from "~/stores/connection";
import { t } from "~/lib/i18n";
import { Flag } from "./Flag";

interface Props {
  profile: ProxyProfile;
  selected: boolean;
  onSelect: () => void;
  onActivate?: () => void;
  onContextMenu?: (e: MouseEvent) => void;
  enterDelayMs?: number;
}

export const ServerRow: Component<Props> = (props) => {
  const cc = () => inferCountryCode(props.profile.name, props.profile.country_code);
  const name = () =>
    displayName(props.profile.name) ||
    `${props.profile.server}:${props.profile.port}`;
  const ping = () => pingFor(props.profile.id);

  const pingTone = (ms: number | null | undefined) => {
    if (ms == null) return "text-[var(--color-fg-3)]";
    return ms < 200
      ? "text-[var(--color-good)]"
      : ms < 500
      ? "text-[var(--color-warn)]"
      : "text-[var(--color-bad)]";
  };

  return (
    <li
      class="anim-row-enter border-b border-[var(--color-line)]"
      style={
        props.enterDelayMs != null
          ? { "animation-delay": `${props.enterDelayMs}ms` }
          : undefined
      }
    >
      <button
        type="button"
        onClick={props.onSelect}
        onDblClick={() => props.onActivate?.()}
        onContextMenu={(e) => {
          e.preventDefault();
          props.onSelect();
          props.onContextMenu?.(e);
        }}
        class="tactile-row grid w-full grid-cols-[28px_1fr_auto_72px] items-center gap-3 px-6 py-2 text-left"
        classList={{
          "bg-[var(--color-tint-2)]": props.selected,
          "hover:bg-[var(--color-tint-1)]": !props.selected,
        }}
      >
        <Flag code={cc()} size={18} />

        <span class="truncate text-[12.5px] text-[var(--color-fg-0)]">{name()}</span>

        <span class="flex items-center gap-1.5 text-[10px]">
          <Tag>{props.profile.protocol}</Tag>
          <Tag>{transportLabel(props.profile.transport.type)}</Tag>
          <Show when={props.profile.tls.reality}>
            <Tag tone="accent">REALITY</Tag>
          </Show>
          <Show when={props.profile.tls.enabled && !props.profile.tls.reality}>
            <Tag>TLS</Tag>
          </Show>
        </span>

        <span class={`text-right font-mono text-[11.5px] tabular-nums ${pingTone(ping())}`}>
          <Show when={probing() && ping() === undefined}>
            <span class="opacity-50">…</span>
          </Show>
          <Show when={ping() === null}>
            <span class="text-[var(--color-fg-3)]">{t("servers.offline")}</span>
          </Show>
          <Show when={typeof ping() === "number"}>
            {ping()}
            <span class="text-[var(--color-fg-3)]">ms</span>
          </Show>
          <Show when={!probing() && ping() === undefined}>
            <span class="text-[var(--color-fg-3)]">—</span>
          </Show>
        </span>
      </button>
    </li>
  );
};

const Tag: Component<{ children: any; tone?: "default" | "accent" }> = (p) => (
  <span
    class="rounded border px-1.5 py-px font-mono text-[10px] uppercase tracking-wider"
    classList={{
      "border-[var(--color-line)] text-[var(--color-fg-2)]": p.tone !== "accent",
      "border-[color-mix(in_srgb,var(--color-accent)_40%,transparent)] text-[color-mix(in_srgb,var(--color-accent)_120%,white)]":
        p.tone === "accent",
    }}
  >
    {p.children}
  </span>
);

const transportLabel = (t: string) => {
  const m: Record<string, string> = {
    Tcp: "TCP", tcp: "TCP",
    Ws: "WS",   ws: "WS",
    Grpc: "GRPC", grpc: "GRPC",
    HttpUpgrade: "HTTPUP", httpupgrade: "HTTPUP",
    XHttp: "XHTTP", xhttp: "XHTTP",
    Quic: "QUIC", quic: "QUIC",
  };
  return m[t] ?? t.toUpperCase();
};
