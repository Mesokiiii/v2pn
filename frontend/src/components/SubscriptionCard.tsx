import { Component, Show } from "solid-js";
import { RefreshCw, Plus, Activity, Shield } from "lucide-solid";
import type { SubscriptionMeta, ConnectionMode } from "~/lib/api";
import { formatBytes, formatExpire } from "~/lib/format";
import { connectionOptions, setMode, connectionState, probing } from "~/stores/connection";
import { elevationStatus } from "~/stores/elevation";
import { t } from "~/lib/i18n";
import { Tooltip } from "./Tooltip";

interface Props {
  meta: SubscriptionMeta;
  count: number;
  onRefresh: () => void;
  onImport: () => void;
  onPing: () => void;
  onTunRequiresAdmin?: () => void;
  refreshing?: boolean;
  canRefresh?: boolean;
}

export const SubscriptionCard: Component<Props> = (props) => {
  const used = () => (props.meta.upload_bytes ?? 0) + (props.meta.download_bytes ?? 0);
  const pct = () => {
    const t = props.meta.total_bytes;
    return t ? Math.min(100, (used() / t) * 100) : 0;
  };

  const usageTone = () => {
    const p = pct();
    if (p >= 100) return "var(--color-bad)";
    if (p >= 85)  return "var(--color-warn)";
    return "var(--color-fg-1)";
  };

  const isLocked = () => connectionState().state === "connected" || connectionState().state === "starting";

  return (
    <section class="flex shrink-0 flex-col gap-4 px-6 pb-5 pt-5">
      {/* row 1: title + actions */}
      <div class="flex items-start justify-between gap-3">
        <div class="min-w-0 flex-1">
          <h1 class="truncate text-[18px] font-semibold leading-tight tracking-tight text-[var(--color-fg-0)]">
            {props.meta.title ?? t("subscription.title")}
          </h1>
          <p class="mt-1 flex items-center gap-1.5 text-[11.5px] text-[var(--color-fg-2)]">
            <span class="font-mono tabular-nums text-[var(--color-fg-1)]">{props.count}</span>
            <span>{t("servers.list").toLowerCase()}</span>
            <Show when={props.meta.update_interval_hours}>
              {(h) => (
                <>
                  <span class="text-[var(--color-fg-3)]">·</span>
                  <span>{t("subscription.autoUpdate", { hours: h() })}</span>
                </>
              )}
            </Show>
            <Show when={props.refreshing}>
              <span class="text-[var(--color-fg-3)]">·</span>
              <span class="text-[var(--color-fg-1)]">{t("subscription.refreshing")}</span>
            </Show>
          </p>
        </div>

        <div class="flex items-center gap-1">
          <Tooltip
            title={t("subscription.ping")}
            body={t("subscription.pingTipBody")}
          >
            <ActionBtn
              onClick={props.onPing}
              icon={<Activity size={12} />}
              label={t("subscription.ping")}
              spinning={probing()}
              disabled={probing()}
            />
          </Tooltip>
          <Tooltip
            title={t("subscription.refresh")}
            body={
              !props.canRefresh
                ? t("subscription.refreshDisabled")
                : t("subscription.refreshTipBody")
            }
          >
            <ActionBtn
              onClick={props.onRefresh}
              icon={<RefreshCw size={12} />}
              label={t("subscription.refresh")}
              spinning={props.refreshing}
              disabled={!props.canRefresh || props.refreshing}
            />
          </Tooltip>
          <Tooltip
            title={t("subscription.new")}
            body={t("subscription.newTipBody")}
          >
            <ActionBtn
              onClick={props.onImport}
              icon={<Plus size={12} />}
              label={t("subscription.new")}
            />
          </Tooltip>
        </div>
      </div>

      {/* row 2: mode switch + usage */}
      <div class="flex items-center gap-4">
        <ModeSwitch
          mode={connectionOptions()?.mode ?? "proxy"}
          locked={isLocked()}
          onChange={(m) => {
            if (m === "tun" && !elevationStatus().elevated) {
              props.onTunRequiresAdmin?.();
              return;
            }
            void setMode(m);
          }}
          tunNeedsAdmin={!elevationStatus().elevated}
        />

        <Show when={props.meta.total_bytes}>
          <Tooltip
            title={t("subscription.usageTip")}
            body={t("subscription.usageTipBody")}
          >
            <div class="flex flex-1 flex-col gap-1.5">
              <div class="hairline h-[4px] overflow-hidden rounded-[2px] bg-[var(--color-bg-1)]">
                <div
                  class="h-full transition-[width,background-color] duration-500"
                  style={{
                    width: `${pct()}%`,
                    "background-color": usageTone(),
                  }}
                />
              </div>
              <div class="flex justify-between font-mono text-[11px] tabular-nums text-[var(--color-fg-2)]">
                <span>
                  <span class="text-[var(--color-fg-0)]">{formatBytes(used())}</span>
                  <span class="px-1 text-[var(--color-fg-3)]">/</span>
                  {formatBytes(props.meta.total_bytes)}
                </span>
                <Show when={props.meta.expire_at}>
                  {(e) => <span>{t("subscription.expires", { date: formatExpire(e()) })}</span>}
                </Show>
              </div>
            </div>
          </Tooltip>
        </Show>
      </div>
    </section>
  );
};

/* ----- Action button. Hover/focus tooltip is provided by an outer
 *       <Tooltip> wrapper at the call site, so we don't pass a `title`
 *       through here — the native title attribute would race the React-
 *       style portal popover and visually flash twice on slow hover.   */
const ActionBtn: Component<{
  onClick: () => void;
  icon: any;
  label: string;
  spinning?: boolean;
  disabled?: boolean;
}> = (p) => (
  <button
    type="button"
    onClick={p.onClick}
    disabled={p.disabled}
    class="tactile hairline flex h-7 items-center gap-1.5 rounded-md px-2.5 text-[11.5px] text-[var(--color-fg-1)] hover:bg-[var(--color-tint-2)] hover:text-[var(--color-fg-0)] disabled:cursor-not-allowed disabled:opacity-50"
  >
    <span
      class="text-[var(--color-fg-2)]"
      classList={{ "animate-spin": !!p.spinning }}
    >
      {p.icon}
    </span>
    {p.label}
  </button>
);

/* ----- Proxy / TUN segmented control ---- */
const ModeSwitch: Component<{
  mode: ConnectionMode;
  locked: boolean;
  onChange: (m: ConnectionMode) => void;
  tunNeedsAdmin?: boolean;
}> = (p) => (
  <div
    class="hairline relative flex h-7 shrink-0 rounded-md bg-[var(--color-bg-1)] p-0.5 text-[11.5px] font-medium"
    classList={{ "opacity-60": p.locked }}
  >
    <span
      aria-hidden="true"
      class="absolute inset-y-0.5 left-0.5 w-[calc(50%-2px)] rounded-[5px] bg-[var(--color-bg-3)] shadow-[0_1px_0_0_rgba(255,255,255,0.06)_inset] transition-transform duration-[260ms]"
      style={{
        transform: p.mode === "tun" ? "translateX(100%)" : "translateX(0)",
      }}
    />
    <Tooltip
      title={
        p.locked
          ? t("subscription.modeLockedTip")
          : t("subscription.modeProxyTip")
      }
      body={
        p.locked
          ? t("subscription.modeLockedTipBody")
          : t("subscription.modeProxyTipBody")
      }
    >
      <ModeOption
        active={p.mode === "proxy"}
        disabled={p.locked}
        onClick={() => p.onChange("proxy")}
        label="PROXY"
      />
    </Tooltip>
    <Tooltip
      title={
        p.locked
          ? t("subscription.modeLockedTip")
          : t("subscription.modeTunTip")
      }
      body={
        p.locked
          ? t("subscription.modeLockedTipBody")
          : p.tunNeedsAdmin
          ? t("subscription.modeTunTipBody") +
            " " +
            t("subscription.modeTunNeedsAdmin")
          : t("subscription.modeTunTipBody")
      }
    >
      <ModeOption
        active={p.mode === "tun"}
        disabled={p.locked}
        onClick={() => p.onChange("tun")}
        label="TUN"
        badge={p.tunNeedsAdmin ? <Shield size={9} /> : null}
      />
    </Tooltip>
  </div>
);

const ModeOption: Component<{
  active: boolean;
  disabled: boolean;
  onClick: () => void;
  label: string;
  badge?: any;
}> = (p) => (
  <button
    type="button"
    onClick={p.onClick}
    disabled={p.disabled}
    class="relative z-10 flex w-[80px] items-center justify-center gap-1 rounded-[5px] font-mono text-[10.5px] tracking-wider transition-colors duration-200 disabled:cursor-not-allowed"
    classList={{
      "text-[var(--color-fg-0)]": p.active,
      "text-[var(--color-fg-2)] hover:text-[var(--color-fg-1)]": !p.active && !p.disabled,
    }}
  >
    {p.label}
    <Show when={p.badge}>
      <span class="text-[var(--color-warn)]">{p.badge}</span>
    </Show>
  </button>
);
