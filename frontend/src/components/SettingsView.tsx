import { Component, For, Show, createMemo, createSignal } from "solid-js";
import {
  FolderOpen,
  ClipboardCopy,
  Globe,
  Network,
  Shield,
  Wrench,
  CheckCircle2,
  XCircle,
  Loader2,
  Info,
  Split,
  RefreshCw,
} from "lucide-solid";
import { connectionOptions, setMode } from "~/stores/connection";
import {
  autoReconnectEnabled,
  autoReconnectStatus,
  setAutoReconnect,
} from "~/stores/autoreconnect";
import type { ConnectionMode, RepairReport } from "~/lib/api";
import { api } from "~/lib/api";
import { t } from "~/lib/i18n";
import { confirm as confirmDialog } from "./ConfirmDialog";
import { RoutingEditor } from "./RoutingEditor";

export const SettingsView: Component = () => {
  const opts = connectionOptions;

  return (
    <div class="flex flex-1 flex-col overflow-y-auto bg-[var(--color-bg-0)]">
      <header class="border-b border-[var(--color-line)] bg-[var(--color-bg-0)] px-7 pt-6 pb-5">
        <h2 class="text-[18px] font-semibold tracking-tight text-[var(--color-fg-0)]">
          {t("settings.title")}
        </h2>
        <p class="mt-1 text-[12.5px] leading-relaxed text-[var(--color-fg-2)]">
          {t("settings.subtitle")}
        </p>
      </header>

      <div class="settings-timeline mx-auto w-full max-w-[720px] px-7 py-6">
        {/* Mode */}
        <Section
          icon={<Network size={14} />}
          title={t("settings.sectionMode")}
          hint={t("settings.sectionModeHint")}
        >
          <ModeOption
            mode="proxy"
            title={t("settings.modeProxy")}
            subtitle={t("settings.modeProxyHint")}
            active={opts()?.mode === "proxy"}
          />
          <ModeOption
            mode="tun"
            title={t("settings.modeTun")}
            subtitle={t("settings.modeTunHint")}
            active={opts()?.mode === "tun"}
          />
        </Section>

        {/* Auto-reconnect */}
        <Section
          icon={<RefreshCw size={14} />}
          title={t("settings.sectionAutoReconnect")}
          hint={t("settings.sectionAutoReconnectHint")}
        >
          <AutoReconnectCard />
        </Section>

        {/* Routing — split tunnel / per-country bypass / custom rules */}
        <Section
          icon={<Split size={14} />}
          title={t("settings.sectionRouting")}
          hint={t("settings.sectionRoutingHint")}
        >
          <RoutingEditor />
        </Section>

        {/* Network repair */}
        <Section
          icon={<Wrench size={14} />}
          title={t("settings.sectionRepair")}
          hint={t("settings.sectionRepairHint")}
        >
          <RepairCard />
        </Section>

        {/* Ports */}
        <Section
          icon={<Globe size={14} />}
          title={t("settings.sectionPorts")}
          hint={t("settings.sectionPortsHint")}
        >
          <KeyValueGrid>
            <KV label={t("settings.portMixed")} value={`127.0.0.1:${opts()?.mixed_port ?? "—"}`} />
            <KV label={t("settings.portClashApi")} value={`127.0.0.1:${opts()?.clash_api_port ?? "—"}`} />
            <KV label={t("settings.portTun")} value={opts()?.tun_interface_name ?? "—"} />
          </KeyValueGrid>
        </Section>

        {/* Protocol */}
        <Section
          icon={<Shield size={14} />}
          title={t("settings.sectionProtocol")}
          hint={t("settings.sectionProtocolHint")}
        >
          <KeyValueGrid>
            <KV
              label={t("settings.protoIpv6")}
              value={opts()?.ipv6 ? t("settings.enabled") : t("settings.disabled")}
            />
            <KV
              label={t("settings.protoStrictDns")}
              value={
                opts()?.strict_dns
                  ? t("settings.strictDnsOn")
                  : t("settings.strictDnsOff")
              }
            />
          </KeyValueGrid>
        </Section>

        {/* About */}
        <Section
          icon={<Info size={14} />}
          title={t("settings.sectionAbout")}
        >
          <KeyValueGrid>
            <KV label={t("settings.aboutVersion")} value="0.1.0 alpha" />
            <KV label={t("settings.aboutSingbox")} value="1.13.13 / windows-amd64" />
            <KV label={t("settings.aboutWintun")} value="0.14.1" />
          </KeyValueGrid>
          <div class="mt-4 flex flex-wrap items-center gap-2">
            <SecondaryBtn
              icon={<FolderOpen size={12} />}
              onClick={() => void api.openLogsFolder()}
            >
              {t("settings.openLogs")}
            </SecondaryBtn>
            <SecondaryBtn
              icon={<ClipboardCopy size={12} />}
              onClick={async () => {
                try {
                  const d = await api.diagnostics();
                  await navigator.clipboard.writeText(JSON.stringify(d, null, 2));
                } catch (e) {
                  console.error("diagnostics copy failed", e);
                }
              }}
            >
              {t("settings.copyDiagnostics")}
            </SecondaryBtn>
          </div>
        </Section>
      </div>
    </div>
  );
};

/* ──────────────────────────────────────────────────────────────────── */
/*  Auto-reconnect card                                                 */
/* ──────────────────────────────────────────────────────────────────── */

const AutoReconnectCard: Component = () => {
  /* Re-evaluates every second so the "Attempt #N in 12s" countdown
   * actually counts down. Cheap; only one timer for the whole card. */
  const [now, setNow] = createSignal(Date.now());
  let tick: number | null = null;
  const start = () => {
    if (tick != null) return;
    tick = window.setInterval(() => setNow(Date.now()), 1000);
  };
  const stop = () => {
    if (tick != null) {
      clearInterval(tick);
      tick = null;
    }
  };
  /* Only run the ticker when there's something to count down. */
  createMemo(() => {
    const s = autoReconnectStatus();
    if (s.kind === "scheduled") start();
    else stop();
    return null;
  });

  const statusLine = () => {
    const s = autoReconnectStatus();
    switch (s.kind) {
      case "idle":
        return autoReconnectEnabled() ? t("settings.autoReconnectStatusIdle") : "";
      case "waiting-network":
        return t("settings.autoReconnectStatusWaitingNetwork");
      case "scheduled": {
        const sec = Math.max(0, Math.ceil((s.nextAtMs - now()) / 1000));
        return t("settings.autoReconnectStatusScheduled", { n: s.attempt, sec });
      }
      case "in-flight":
        return t("settings.autoReconnectStatusInFlight", { n: s.attempt });
    }
  };

  const statusTone = () => {
    const s = autoReconnectStatus();
    if (!autoReconnectEnabled()) return "muted";
    if (s.kind === "in-flight") return "active";
    if (s.kind === "scheduled" || s.kind === "waiting-network") return "warn";
    return "muted";
  };

  return (
    <div class="hairline rounded-lg bg-[var(--color-bg-1)] p-4">
      <div class="flex items-start justify-between gap-3">
        <div class="min-w-0 flex-1">
          <div class="text-[12.5px] font-medium leading-tight text-[var(--color-fg-0)]">
            {t("settings.autoReconnectLabel")}
          </div>
          <p class="mt-1.5 text-[11.5px] leading-[1.55] text-[var(--color-fg-2)]">
            {t("settings.autoReconnectDesc")}
          </p>
        </div>
        <Switch
          on={autoReconnectEnabled()}
          onChange={(v) => setAutoReconnect(v)}
        />
      </div>

      <Show when={autoReconnectEnabled() && statusLine()}>
        <div class="mt-3 flex items-center gap-2 border-t border-[var(--color-line)] pt-3 text-[11.5px]">
          <span
            class="dot shrink-0"
            data-state={
              statusTone() === "active"
                ? "connecting"
                : statusTone() === "warn"
                ? "connecting"
                : "idle"
            }
          />
          <span
            classList={{
              "text-[var(--color-fg-1)]":
                statusTone() === "active" || statusTone() === "warn",
              "text-[var(--color-fg-2)]": statusTone() === "muted",
            }}
          >
            {statusLine()}
          </span>
        </div>
      </Show>
    </div>
  );
};

/* A small, reusable on/off toggle. Pure CSS animation; no library. */
const Switch: Component<{ on: boolean; onChange: (v: boolean) => void }> = (p) => (
  <button
    type="button"
    role="switch"
    aria-checked={p.on}
    onClick={() => p.onChange(!p.on)}
    class="tactile relative h-[22px] w-[40px] shrink-0 rounded-full border transition-colors duration-200"
    classList={{
      "border-[color-mix(in_srgb,var(--color-accent)_55%,transparent)] bg-[color-mix(in_srgb,var(--color-accent)_85%,transparent)]":
        p.on,
      "border-[var(--color-line-strong)] bg-[var(--color-bg-2)]": !p.on,
    }}
  >
    <span
      aria-hidden="true"
      class="absolute top-[2px] left-[2px] h-[16px] w-[16px] rounded-full bg-white shadow-[0_1px_2px_rgba(0,0,0,0.25)] transition-transform duration-[220ms]"
      style={{ transform: p.on ? "translateX(18px)" : "translateX(0)" }}
    />
  </button>
);

/* ──────────────────────────────────────────────────────────────────── */
/*  Repair card                                                         */
/* ──────────────────────────────────────────────────────────────────── */

const RepairCard: Component = () => {
  const [running, setRunning] = createSignal(false);
  const [report, setReport] = createSignal<RepairReport | null>(null);

  const runRepair = async () => {
    const ok = await confirmDialog({
      title: t("settings.repairConfirmTitle"),
      body: t("settings.repairConfirmBody"),
      confirmLabel: t("settings.repairConfirmCta"),
      cancelLabel: t("common.cancel"),
    });
    if (!ok) return;
    setRunning(true);
    setReport(null);
    try {
      const r = await api.repairNetwork();
      setReport(r);
    } catch (e) {
      console.error("repair_network failed", e);
    } finally {
      setRunning(false);
    }
  };

  return (
    <div class="hairline rounded-lg bg-[var(--color-bg-1)] p-4">
      <div class="flex items-start justify-between gap-3">
        <div class="min-w-0 flex-1">
          <p class="text-[12.5px] leading-relaxed text-[var(--color-fg-1)]">
            {t("settings.repairDescription")}
          </p>
        </div>
        <button
          type="button"
          onClick={() => void runRepair()}
          disabled={running()}
          class="flex h-8 shrink-0 items-center gap-1.5 rounded-md bg-[var(--color-accent)] px-3 text-[12.5px] font-medium text-white transition-[background,opacity] duration-150 hover:bg-[color-mix(in_srgb,var(--color-accent)_85%,white)] disabled:cursor-not-allowed disabled:opacity-60"
        >
          {running() ? (
            <Loader2 size={13} class="animate-spin" />
          ) : (
            <Wrench size={13} />
          )}
          {running() ? t("settings.repairRunning") : t("settings.repairAction")}
        </button>
      </div>

      <Show when={report()}>
        {(r) => (
          <div class="mt-4 space-y-1.5 border-t border-[var(--color-line)] pt-3">
            <div class="text-[11.5px] font-medium text-[var(--color-fg-1)]">
              {t("settings.repairResult", {
                ok: r().steps.filter((s) => s.ok).length,
                total: r().steps.length,
              })}
            </div>
            <For each={r().steps}>
              {(step) => (
                <div class="flex items-start gap-2 py-1 text-[12px]">
                  <span
                    class="mt-0.5 shrink-0"
                    classList={{
                      "text-[var(--color-good)]": step.ok,
                      "text-[var(--color-bad)]": !step.ok,
                    }}
                  >
                    {step.ok ? <CheckCircle2 size={13} /> : <XCircle size={13} />}
                  </span>
                  <div class="min-w-0 flex-1">
                    <div class="text-[var(--color-fg-1)]">
                      {/* Backend ships a stable label_key; if a translation
                          is missing, fall back to a humanised id. */}
                      {translateOrFallback(step.label_key, step.id)}
                    </div>
                    <Show when={step.detail}>
                      <div class="mt-0.5 break-words font-mono text-[10.5px] leading-snug text-[var(--color-fg-3)]">
                        {step.detail}
                      </div>
                    </Show>
                  </div>
                  <span class="shrink-0 font-mono text-[10.5px] tabular-nums text-[var(--color-fg-3)]">
                    {step.took_ms}ms
                  </span>
                </div>
              )}
            </For>
          </div>
        )}
      </Show>
    </div>
  );
};

/* Translate `repair.flushDns` → user-visible label. We fall back to the
 * id when the key is missing so a backend-only step doesn't show as a
 * raw key in the UI. */
function translateOrFallback(key: string, id: string): string {
  // `t()` returns the key itself when nothing matches, so we can detect
  // missing translations by string equality.
  const v = t(key as any);
  if (v && v !== key) return v;
  return id.replace(/_/g, " ");
}

/* ──────────────────────────────────────────────────────────────────── */
/*  Layout primitives                                                   */
/* ──────────────────────────────────────────────────────────────────── */

const Section: Component<{
  icon: any;
  title: string;
  hint?: string;
  children: any;
}> = (props) => (
  <section class="relative mb-7">
    <div class="mb-3 flex items-start gap-2.5">
      {/* z-10 + solid bg-1 → the timeline line passing through this
          column is visually clipped behind the icon, leaving a clean
          "node on a thread" effect rather than a line crossing it. */}
      <div class="relative z-10 mt-0.5 grid h-7 w-7 shrink-0 place-items-center rounded-md bg-[var(--color-bg-1)] text-[var(--color-fg-1)] hairline">
        {props.icon}
      </div>
      <div class="min-w-0">
        <h3 class="text-[13.5px] font-semibold tracking-tight text-[var(--color-fg-0)]">
          {props.title}
        </h3>
        <Show when={props.hint}>
          <p class="mt-0.5 text-[11.5px] leading-relaxed text-[var(--color-fg-2)]">
            {props.hint}
          </p>
        </Show>
      </div>
    </div>
    <div class="relative z-10 pl-[38px]">{props.children}</div>
  </section>
);

const ModeOption: Component<{
  mode: ConnectionMode;
  title: string;
  subtitle: string;
  active: boolean;
}> = (props) => (
  <button
    type="button"
    onClick={() => void setMode(props.mode)}
    class="tactile-row mb-2 flex w-full items-start gap-3 rounded-md border px-3.5 py-3 text-left transition-colors"
    classList={{
      "border-[color-mix(in_srgb,var(--color-accent)_45%,transparent)] bg-[color-mix(in_srgb,var(--color-accent)_8%,transparent)]":
        props.active,
      "border-[var(--color-line)] hover:bg-[var(--color-tint-1)]": !props.active,
    }}
  >
    <div
      class="mt-0.5 grid h-4 w-4 shrink-0 place-items-center rounded-full border transition-colors"
      classList={{
        "border-[var(--color-accent)] bg-[var(--color-accent)]": props.active,
        "border-[var(--color-line-strong)]": !props.active,
      }}
    >
      <Show when={props.active}>
        <span class="h-1.5 w-1.5 rounded-full bg-white" />
      </Show>
    </div>
    <div class="min-w-0 flex-1">
      <div class="text-[12.5px] font-medium leading-tight text-[var(--color-fg-0)]">
        {props.title}
      </div>
      <div class="mt-1 text-[11.5px] leading-[1.55] text-[var(--color-fg-2)]">
        {props.subtitle}
      </div>
    </div>
  </button>
);

const KeyValueGrid: Component<{ children: any }> = (p) => (
  <div class="hairline divide-y divide-[var(--color-line)] overflow-hidden rounded-lg bg-[var(--color-bg-1)]">
    {p.children}
  </div>
);

const KV: Component<{ label: string; value: string }> = (p) => (
  <div class="flex items-center justify-between px-3.5 py-2.5 text-[12px]">
    <span class="text-[var(--color-fg-1)]">{p.label}</span>
    <span class="font-mono text-[11.5px] tabular-nums text-[var(--color-fg-0)]">{p.value}</span>
  </div>
);

const SecondaryBtn: Component<{
  icon: any;
  onClick: () => void;
  children: any;
}> = (p) => (
  <button
    type="button"
    onClick={p.onClick}
    class="tactile hairline flex h-7 items-center gap-1.5 rounded-md bg-[var(--color-bg-1)] px-3 text-[12px] text-[var(--color-fg-1)] transition-colors hover:bg-[var(--color-tint-2)] hover:text-[var(--color-fg-0)]"
  >
    {p.icon}
    {p.children}
  </button>
);

