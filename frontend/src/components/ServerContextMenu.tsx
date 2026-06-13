import { Component, onCleanup, onMount, Show } from "solid-js";
import { Power, Copy, Info } from "lucide-solid";
import type { ProxyProfile } from "~/lib/api";
import { t } from "~/lib/i18n";

export interface ServerContextMenuTarget {
  profile: ProxyProfile;
  x: number;
  y: number;
}

interface Props {
  target: ServerContextMenuTarget | null;
  isConnectedTo: boolean;
  onConnect: (p: ProxyProfile) => void;
  onDisconnect: () => void;
  onClose: () => void;
}

export const ServerContextMenu: Component<Props> = (props) => {
  return (
    <Show when={props.target}>
      {(target) => <Inner {...props} target={target()} />}
    </Show>
  );
};

const Inner: Component<{
  target: ServerContextMenuTarget;
  isConnectedTo: boolean;
  onConnect: (p: ProxyProfile) => void;
  onDisconnect: () => void;
  onClose: () => void;
}> = (props) => {
  let ref: HTMLDivElement | undefined;

  onMount(() => {
    const onClick = (e: MouseEvent) => {
      if (ref && !ref.contains(e.target as Node)) props.onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") props.onClose();
    };
    // Defer attach by one frame so the *opening* click doesn't immediately close us.
    const id = window.setTimeout(() => {
      window.addEventListener("mousedown", onClick);
      window.addEventListener("keydown", onKey);
    }, 0);
    onCleanup(() => {
      window.clearTimeout(id);
      window.removeEventListener("mousedown", onClick);
      window.removeEventListener("keydown", onKey);
    });
  });

  // Clamp to viewport so we never spawn off-screen.
  const W = 220;
  const H = 160;
  const left = () => Math.min(props.target.x, window.innerWidth - W - 8);
  const top = () => Math.min(props.target.y, window.innerHeight - H - 8);

  function copy(text: string) {
    navigator.clipboard?.writeText(text).catch(() => {});
    props.onClose();
  }

  function buildShareLink(p: ProxyProfile): string | null {
    // Best-effort. Full URI generation lives in Rust later; this covers
    // the common case so the user can paste into Happ / v2rayN immediately.
    if (p.protocol === "vless") {
      const settings = p.settings as { kind?: string; uuid?: string; flow?: string | null };
      const tls = p.tls;
      const params = new URLSearchParams();
      params.set("type", p.transport.type?.toString().toLowerCase() ?? "tcp");
      params.set(
        "security",
        tls.reality ? "reality" : tls.enabled ? "tls" : "none"
      );
      if (tls.server_name) params.set("sni", tls.server_name);
      if (tls.utls_fingerprint) params.set("fp", tls.utls_fingerprint);
      if (tls.reality?.public_key) params.set("pbk", tls.reality.public_key);
      if (tls.reality?.short_id) params.set("sid", tls.reality.short_id);
      if (settings.flow) params.set("flow", settings.flow);
      const frag = encodeURIComponent(p.name);
      return `vless://${settings.uuid}@${p.server}:${p.port}?${params.toString()}#${frag}`;
    }
    return null;
  }

  return (
    <div
      ref={ref}
      class="fixed z-[60] w-[220px] overflow-hidden rounded-md border border-[var(--color-line)] bg-[var(--color-bg-2)] shadow-[0_12px_32px_-12px_rgba(0,0,0,0.45)]"
      style={{ left: `${left()}px`, top: `${top()}px` }}
      role="menu"
    >
      <div class="border-b border-[var(--color-line)] px-3 py-2">
        <div class="truncate text-[12.5px] font-medium text-[var(--color-fg-0)]">
          {props.target.profile.name}
        </div>
        <div class="mt-0.5 truncate font-mono text-[10.5px] text-[var(--color-fg-3)]">
          {props.target.profile.server}:{props.target.profile.port}
        </div>
      </div>

      <div class="py-1">
        <Show
          when={!props.isConnectedTo}
          fallback={
            <Item
              onClick={() => {
                props.onDisconnect();
                props.onClose();
              }}
              icon={<Power size={12} />}
              label={t("connection.disconnect")}
              tone="danger"
            />
          }
        >
          <Item
            onClick={() => {
              props.onConnect(props.target.profile);
              props.onClose();
            }}
            icon={<Power size={12} />}
            label={t("connection.connect")}
            tone="accent"
          />
        </Show>

        <hr class="my-1 border-t border-[var(--color-line)]" />

        <Item
          onClick={() => copy(`${props.target.profile.server}:${props.target.profile.port}`)}
          icon={<Copy size={12} />}
          label={`${t("detail.server")}:${t("detail.port")}`}
        />
        <Show when={buildShareLink(props.target.profile)}>
          {(link) => (
            <Item
              onClick={() => copy(link())}
              icon={<Copy size={12} />}
              label="vless://…"
            />
          )}
        </Show>

        <hr class="my-1 border-t border-[var(--color-line)]" />

        <Item
          onClick={props.onClose}
          icon={<Info size={12} />}
          label={t("servers.selected")}
          subtle
        />
      </div>
    </div>
  );
};

const Item: Component<{
  onClick: () => void;
  icon: any;
  label: string;
  tone?: "default" | "accent" | "danger";
  subtle?: boolean;
}> = (p) => (
  <button
    type="button"
    onClick={p.onClick}
    class="tactile-row flex w-full items-center gap-2 px-3 py-1.5 text-left text-[12px]"
    classList={{
      "text-[var(--color-fg-1)] hover:bg-[var(--color-tint-2)] hover:text-[var(--color-fg-0)]":
        !p.tone || p.tone === "default" || p.subtle,
      "text-[var(--color-accent)] hover:bg-[color-mix(in_srgb,var(--color-accent)_10%,transparent)]":
        p.tone === "accent",
      "text-[var(--color-bad)] hover:bg-[color-mix(in_srgb,var(--color-bad)_10%,transparent)]":
        p.tone === "danger",
    }}
  >
    <span
      class="grid h-3.5 w-3.5 place-items-center"
      classList={{ "opacity-60": p.subtle }}
    >
      {p.icon}
    </span>
    <span class="truncate">{p.label}</span>
  </button>
);
