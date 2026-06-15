import { Component, createMemo, Show } from "solid-js";
import { RefreshCw, Plus, Activity, Shield, Globe } from "lucide-solid";
import type { SubscriptionMeta, ConnectionMode } from "~/lib/api";
import { formatBytes, formatExpire } from "~/lib/format";
import {
  connectionOptions,
  setMode,
  setRouting,
  connectionState,
  probing,
} from "~/stores/connection";
import { elevationStatus } from "~/stores/elevation";
import { autoReconnectEnabled, setAutoReconnect } from "~/stores/autoreconnect";
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

/* localStorage key for remembering the user's last non-empty country list,
 * so the quick "Local traffic: Direct" toggle doesn't clobber a multi-country
 * setup configured in Settings → Routing. */
const LAST_BYPASS_KEY = "v2pn:lastBypassCountries";

function rememberBypass(codes: string[]): void {
  if (codes.length === 0) return;
  try {
    localStorage.setItem(LAST_BYPASS_KEY, JSON.stringify(codes));
  } catch {
    /* localStorage may be disabled — toggle still works, just won't restore. */
  }
}
function recallBypass(): string[] {
  try {
    const raw = localStorage.getItem(LAST_BYPASS_KEY);
    if (raw) {
      const parsed = JSON.parse(raw);
      if (Array.isArray(parsed) && parsed.every((x) => typeof x === "string") && parsed.length > 0) {
        return parsed;
      }
    }
  } catch {
    /* fall through to default */
  }
  return ["ru"];
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

  const isLocked = () =>
    connectionState().state === "connected" || connectionState().state === "starting";

  /* Whether the bypass is *active* right now (any country in the list). */
  const bypassActive = createMemo(
    () => (connectionOptions()?.bypass_country_codes ?? []).length > 0,
  );

  /* Toggle handler: persist last non-empty list, swap to []/restored. */
  async function toggleBypass(next: boolean) {
    const opts = connectionOptions();
    const custom = opts?.custom_bypass_rules ?? [];
    if (next) {
      const restored = recallBypass();
      await setRouting(restored, custom);
    } else {
      const current = opts?.bypass_country_codes ?? [];
      rememberBypass(current); // remember what the user had
      await setRouting([], custom);
    }
  }

  return (
    <section class="flex shrink-0 flex-col gap-5 px-6 pb-5 pt-5">
      {/* ------------------------------------------------------------------ */}
      {/*  Row 1 — Identity + actions.                                       */}
      {/*  Title block on the left, action toolbar pinned right.             */}
      {/*  Two visual clusters in the toolbar:                               */}
      {/*    [ Пинг │ Обновить ]   [ + Новая ]                               */}
      {/*  Ping/Refresh act on this subscription, so they share a container; */}
      {/*  "+ Новая" has a different intent (adds a new sub) → separate.     */}
      {/* ------------------------------------------------------------------ */}
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

        <div class="flex shrink-0 items-center gap-1.5">
          {/* Cluster 1 — sub-scoped actions (share container, divided). */}
          <div class="hairline flex h-7 items-center rounded-md bg-[var(--color-bg-1)]">
            <Tooltip
              title={t("subscription.ping")}
              body={t("subscription.pingTipBody")}
            >
              <SegBtn
                onClick={props.onPing}
                icon={<Activity size={12} />}
                label={t("subscription.ping")}
                spinning={probing()}
                disabled={probing()}
              />
            </Tooltip>
            <span aria-hidden="true" class="h-4 w-px bg-[var(--color-line-strong)]" />
            <Tooltip
              title={t("subscription.refresh")}
              body={
                !props.canRefresh
                  ? t("subscription.refreshDisabled")
                  : t("subscription.refreshTipBody")
              }
            >
              <SegBtn
                onClick={props.onRefresh}
                icon={<RefreshCw size={12} />}
                label={t("subscription.refresh")}
                spinning={props.refreshing}
                disabled={!props.canRefresh || props.refreshing}
              />
            </Tooltip>
          </div>

          {/* Cluster 2 — distinct intent, distinct container. */}
          <Tooltip
            title={t("subscription.new")}
            body={t("subscription.newTipBody")}
          >
            <SoloBtn
              onClick={props.onImport}
              icon={<Plus size={12} />}
              label={t("subscription.new")}
            />
          </Tooltip>
        </div>
      </div>

      {/* ------------------------------------------------------------------ */}
      {/*  Row 2 — The two binary controls, each with its own caption.       */}
      {/*  Captions remove all ambiguity about what the switch controls,     */}
      {/*  so the tooltip can stay focused on consequences.                  */}
      {/*    РЕЖИМ                       ЛОКАЛЬНЫЙ ТРАФИК                   */}
      {/*    [PROXY │ TUN]                [ТУННЕЛЬ │ НАПРЯМУЮ]               */}
      {/* ------------------------------------------------------------------ */}
      <div class="flex flex-wrap items-end gap-x-8 gap-y-3">
        <CaptionedControl caption={t("subscription.modeLabel")}>
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
        </CaptionedControl>

        <CaptionedControl caption={t("subscription.localTrafficLabel")}>
          <LocalTrafficSwitch
            direct={bypassActive()}
            onChange={(direct) => void toggleBypass(direct)}
          />
        </CaptionedControl>

        <CaptionedControl caption={t("subscription.autoReconnectLabel")}>
          <AutoReconnectSwitch
            enabled={autoReconnectEnabled()}
            onChange={(v) => setAutoReconnect(v)}
          />
        </CaptionedControl>
      </div>

      {/* ------------------------------------------------------------------ */}
      {/*  Row 3 — Subscription quota.                                       */}
      {/*  Full-width progress + small line with usage and expiry.           */}
      {/*  Hidden when the provider doesn't expose a quota.                  */}
      {/* ------------------------------------------------------------------ */}
      <Show when={props.meta.total_bytes}>
        <Tooltip
          title={t("subscription.usageTip")}
          body={t("subscription.usageTipBody")}
        >
          <div class="flex flex-col gap-2">
            <div class="hairline h-[5px] overflow-hidden rounded-full bg-[var(--color-bg-1)]">
              <div
                class="h-full rounded-full transition-[width,background-color] duration-500"
                style={{
                  width: `${pct()}%`,
                  "background-color": usageTone(),
                }}
              />
            </div>
            <div class="flex items-baseline justify-between font-mono text-[11.5px] tabular-nums text-[var(--color-fg-2)]">
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
    </section>
  );
};

/* ---------------------------------------------------------------------- */
/*  Action buttons.                                                       */
/*  - SegBtn  → sits inside a multi-button rounded container, no own bg.  */
/*  - SoloBtn → free-standing rounded button with its own hairline.       */
/* ---------------------------------------------------------------------- */

/* ---------------------------------------------------------------------- */
/*  CaptionedControl — a tiny uppercase caption above any control.        */
/*  Used to label the two switches (РЕЖИМ / ЛОКАЛЬНЫЙ ТРАФИК) so the     */
/*  per-segment tooltips can stay focused on consequences instead of     */
/*  re-stating "what is this".                                            */
/* ---------------------------------------------------------------------- */
const CaptionedControl: Component<{ caption: string; children: any }> = (p) => (
  <div class="flex flex-col gap-1.5">
    <span class="text-[10px] font-medium uppercase tracking-[0.08em] text-[var(--color-fg-2)]">
      {p.caption}
    </span>
    {p.children}
  </div>
);

const SegBtn: Component<{
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
    class="tactile flex h-full items-center gap-1.5 rounded-md px-2.5 text-[11.5px] text-[var(--color-fg-1)] hover:bg-[var(--color-tint-2)] hover:text-[var(--color-fg-0)] disabled:cursor-not-allowed disabled:opacity-50"
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

const SoloBtn: Component<{
  onClick: () => void;
  icon: any;
  label: string;
}> = (p) => (
  <button
    type="button"
    onClick={p.onClick}
    class="tactile hairline flex h-7 items-center gap-1.5 rounded-md bg-[var(--color-bg-1)] px-2.5 text-[11.5px] text-[var(--color-fg-1)] hover:bg-[var(--color-tint-2)] hover:text-[var(--color-fg-0)]"
  >
    <span class="text-[var(--color-fg-2)]">{p.icon}</span>
    {p.label}
  </button>
);

/* ---------------------------------------------------------------------- */
/*  Generic two-segment switch (the visual primitive for PROXY|TUN and    */
/*  Tunnel|Direct). Sliding indicator + crisp label transitions.          */
/* ---------------------------------------------------------------------- */

interface SegSwitchProps {
  active: 0 | 1;
  locked?: boolean;
  width?: number; // segment width in px (default 80)
  onChange: (idx: 0 | 1) => void;
  segments: [SegSwitchSegment, SegSwitchSegment];
}
interface SegSwitchSegment {
  label: string;
  icon?: any;
  badge?: any;
  tooltip: { title: string; body: string };
  disabled?: boolean;
}

const SegSwitch: Component<SegSwitchProps> = (p) => {
  const w = () => p.width ?? 80;
  return (
    <div
      class="hairline relative flex h-7 shrink-0 rounded-md bg-[var(--color-bg-1)] p-0.5 text-[11.5px] font-medium"
      classList={{ "opacity-60": !!p.locked }}
    >
      <span
        aria-hidden="true"
        class="absolute inset-y-0.5 left-0.5 rounded-[5px] bg-[var(--color-bg-3)] shadow-[0_1px_0_0_rgba(255,255,255,0.06)_inset] transition-transform duration-[260ms]"
        style={{
          width: `${w() - 2}px`,
          transform: p.active === 1 ? `translateX(${w()}px)` : "translateX(0)",
        }}
      />
      {p.segments.map((seg, idx) => (
        <Tooltip title={seg.tooltip.title} body={seg.tooltip.body}>
          <button
            type="button"
            onClick={() => p.onChange(idx as 0 | 1)}
            disabled={p.locked || seg.disabled}
            style={{ width: `${w()}px` }}
            class="relative z-10 flex items-center justify-center gap-1 rounded-[5px] font-mono text-[10.5px] tracking-wider transition-colors duration-200 disabled:cursor-not-allowed"
            classList={{
              "text-[var(--color-fg-0)]": p.active === idx,
              "text-[var(--color-fg-2)] hover:text-[var(--color-fg-1)]":
                p.active !== idx && !p.locked && !seg.disabled,
            }}
          >
            <Show when={seg.icon}>
              <span class="text-[var(--color-fg-2)]">{seg.icon}</span>
            </Show>
            {seg.label}
            <Show when={seg.badge}>
              <span class="text-[var(--color-warn)]">{seg.badge}</span>
            </Show>
          </button>
        </Tooltip>
      ))}
    </div>
  );
};

/* ---------------------------------------------------------------------- */
/*  Concrete switches built on top of <SegSwitch>.                        */
/* ---------------------------------------------------------------------- */

const ModeSwitch: Component<{
  mode: ConnectionMode;
  locked: boolean;
  onChange: (m: ConnectionMode) => void;
  tunNeedsAdmin?: boolean;
}> = (p) => {
  const proxyTip = () =>
    p.locked
      ? { title: t("subscription.modeLockedTip"), body: t("subscription.modeLockedTipBody") }
      : { title: t("subscription.modeProxyTip"), body: t("subscription.modeProxyTipBody") };
  const tunTip = () =>
    p.locked
      ? { title: t("subscription.modeLockedTip"), body: t("subscription.modeLockedTipBody") }
      : {
          title: t("subscription.modeTunTip"),
          body: p.tunNeedsAdmin
            ? t("subscription.modeTunTipBody") + " " + t("subscription.modeTunNeedsAdmin")
            : t("subscription.modeTunTipBody"),
        };

  return (
    <SegSwitch
      active={p.mode === "tun" ? 1 : 0}
      locked={p.locked}
      onChange={(idx) => p.onChange(idx === 1 ? "tun" : "proxy")}
      segments={[
        { label: "PROXY", tooltip: proxyTip() },
        {
          label: "TUN",
          tooltip: tunTip(),
          badge: p.tunNeedsAdmin ? <Shield size={9} /> : null,
        },
      ]}
    />
  );
};

const LocalTrafficSwitch: Component<{
  direct: boolean;
  onChange: (direct: boolean) => void;
}> = (p) => {
  const tunnelTip = {
    title: t("subscription.localTrafficTunnelTip"),
    body: t("subscription.localTrafficTunnelTipBody"),
  };
  const directTip = {
    title: t("subscription.localTrafficDirectTip"),
    body: t("subscription.localTrafficDirectTipBody"),
  };
  return (
    <SegSwitch
      active={p.direct ? 1 : 0}
      width={92}
      onChange={(idx) => p.onChange(idx === 1)}
      segments={[
        {
          label: t("subscription.localTrafficTunnel").toUpperCase(),
          tooltip: tunnelTip,
        },
        {
          label: t("subscription.localTrafficDirect").toUpperCase(),
          tooltip: directTip,
          icon: <Globe size={9} />,
        },
      ]}
    />
  );
};



/* ---------------------------------------------------------------------- */
/*  AutoReconnectSwitch — quick on/off mirror of the Settings toggle.    */
/*  Lives here so users don't have to dive into Settings just to flip    */
/*  it. Per-segment tooltips explain the consequence, not "what is".     */
/* ---------------------------------------------------------------------- */

const AutoReconnectSwitch: Component<{
  enabled: boolean;
  onChange: (next: boolean) => void;
}> = (p) => {
  const offTip = {
    title: t("subscription.autoReconnectOffTip"),
    body: t("subscription.autoReconnectOffTipBody"),
  };
  const onTip = {
    title: t("subscription.autoReconnectOnTip"),
    body: t("subscription.autoReconnectOnTipBody"),
  };
  return (
    <SegSwitch
      active={p.enabled ? 1 : 0}
      width={70}
      onChange={(idx) => p.onChange(idx === 1)}
      segments={[
        { label: t("subscription.autoReconnectOff").toUpperCase(), tooltip: offTip },
        { label: t("subscription.autoReconnectOn").toUpperCase(), tooltip: onTip },
      ]}
    />
  );
};
