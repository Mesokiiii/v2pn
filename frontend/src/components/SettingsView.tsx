import { Component, Show } from "solid-js";
import { FolderOpen, ClipboardCopy } from "lucide-solid";
import { connectionOptions, setMode } from "~/stores/connection";
import type { ConnectionMode } from "~/lib/api";
import { api } from "~/lib/api";
import { t } from "~/lib/i18n";

export const SettingsView: Component = () => {
  const opts = connectionOptions;

  return (
    <div class="flex flex-1 flex-col overflow-y-auto">
      <header class="border-b border-[var(--color-line)] px-6 py-4">
        <h2 class="text-[15px] font-semibold tracking-tight text-[var(--color-fg-0)]">
          {t("settings.title")}
        </h2>
        <p class="mt-0.5 text-[12px] text-[var(--color-fg-2)]">{t("settings.subtitle")}</p>
      </header>

      <Section title={t("settings.sectionMode")} hint={t("settings.sectionModeHint")}>
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

      <Section title={t("settings.sectionPorts")} hint={t("settings.sectionPortsHint")}>
        <Row label={t("settings.portMixed")} value={`127.0.0.1:${opts()?.mixed_port ?? "—"}`} />
        <Row label={t("settings.portClashApi")} value={`127.0.0.1:${opts()?.clash_api_port ?? "—"}`} />
        <Row label={t("settings.portTun")} value={opts()?.tun_interface_name ?? "—"} />
      </Section>

      <Section title={t("settings.sectionProtocol")} hint={t("settings.sectionProtocolHint")}>
        <Row
          label={t("settings.protoIpv6")}
          value={opts()?.ipv6 ? t("settings.enabled") : t("settings.disabled")}
        />
        <Row
          label={t("settings.protoStrictDns")}
          value={opts()?.strict_dns ? t("settings.strictDnsOn") : t("settings.strictDnsOff")}
        />
      </Section>

      <Section title={t("settings.sectionAbout")} hint="">
        <Row label={t("settings.aboutVersion")} value="0.1.0 alpha" />
        <Row label={t("settings.aboutSingbox")} value="1.13.13 / windows-amd64" />
        <Row label={t("settings.aboutWintun")} value="0.14.1" />
        <div class="mt-3 flex flex-wrap items-center gap-2">
          <button
            type="button"
            onClick={() => void api.openLogsFolder()}
            class="tactile hairline flex h-7 items-center gap-1.5 rounded-md px-3 text-[12px] text-[var(--color-fg-1)] hover:bg-[var(--color-tint-2)] hover:text-[var(--color-fg-0)]"
          >
            <FolderOpen size={12} />
            {t("settings.openLogs")}
          </button>
          <button
            type="button"
            onClick={async () => {
              try {
                const d = await api.diagnostics();
                await navigator.clipboard.writeText(JSON.stringify(d, null, 2));
              } catch (e) {
                console.error("diagnostics copy failed", e);
              }
            }}
            class="tactile hairline flex h-7 items-center gap-1.5 rounded-md px-3 text-[12px] text-[var(--color-fg-1)] hover:bg-[var(--color-tint-2)] hover:text-[var(--color-fg-0)]"
          >
            <ClipboardCopy size={12} />
            {t("settings.copyDiagnostics")}
          </button>
        </div>
      </Section>
    </div>
  );
};

const Section: Component<{ title: string; hint: string; children: any }> = (props) => (
  <section class="border-b border-[var(--color-line)] px-6 py-5">
    <div class="mb-3">
      <h3 class="text-[12.5px] font-semibold tracking-tight text-[var(--color-fg-0)]">{props.title}</h3>
      <Show when={props.hint}>
        <p class="mt-0.5 text-[11.5px] text-[var(--color-fg-2)]">{props.hint}</p>
      </Show>
    </div>
    {props.children}
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
    class="tactile-row mb-2 flex w-full items-start gap-3 rounded-md border px-3 py-2.5 text-left"
    classList={{
      "border-[color-mix(in_srgb,var(--color-accent)_45%,transparent)] bg-[color-mix(in_srgb,var(--color-accent)_8%,transparent)]":
        props.active,
      "border-[var(--color-line)] hover:bg-[var(--color-tint-1)]": !props.active,
    }}
  >
    <div
      class="mt-0.5 grid h-4 w-4 shrink-0 place-items-center rounded-full border"
      classList={{
        "border-[var(--color-accent)] bg-[var(--color-accent)]": props.active,
        "border-[var(--color-line-strong)]": !props.active,
      }}
    >
      <Show when={props.active}>
        <span class="h-1.5 w-1.5 rounded-full bg-white" />
      </Show>
    </div>
    <div class="min-w-0">
      <div class="text-[12.5px] font-medium text-[var(--color-fg-0)]">{props.title}</div>
      <div class="mt-0.5 text-[11.5px] leading-relaxed text-[var(--color-fg-2)]">
        {props.subtitle}
      </div>
    </div>
  </button>
);

const Row: Component<{ label: string; value: string }> = (p) => (
  <div class="flex items-center justify-between border-b border-[var(--color-line)] py-2 last:border-b-0 text-[12px]">
    <span class="text-[var(--color-fg-1)]">{p.label}</span>
    <span class="font-mono text-[11.5px] tabular-nums text-[var(--color-fg-0)]">{p.value}</span>
  </div>
);
