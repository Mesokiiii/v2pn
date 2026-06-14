import { Component, For, Show, createMemo, createSignal } from "solid-js";
import { Check, Globe, Plus, X } from "lucide-solid";
import { connectionOptions, setRouting, connectionState } from "~/stores/connection";
import { t } from "~/lib/i18n";

/**
 * Routing editor — multi-country bypass picker + free-form custom rules.
 *
 * The user authors a set of "shortcuts to direct" — domains and IP
 * blocks that skip the VPN tunnel. We surface two equivalent inputs:
 *
 *  - **Country presets** — checked countries get their entire geosite +
 *    geoip rule-set added at config build time. Curated by SagerNet,
 *    auto-refreshed weekly. Covers banks, gov sites, top media,
 *    e-commerce per locale.
 *  - **Custom rules** — line-by-line text input for power users. Same
 *    syntax as the parser (`example.com`, `*.foo.com`, `1.2.3.0/24`,
 *    bare IP). Live preview shows the count by bucket so you see what
 *    will compile before saving.
 *
 * Apply policy: changes are batched, persisted via `setRouting`, then
 * the user is reminded that a reconnect is required for sing-box to
 * pick up the new rule_set definitions.
 */

/** Top countries we surface as one-click presets. Ordered roughly by
 *  how often we expect users to pick them. The full ISO list is
 *  accepted by the backend — this is just the curated visible subset. */
const PRESET_COUNTRIES: Array<{ code: string; name: string; flag: string }> = [
  { code: "ru", name: "Россия", flag: "🇷🇺" },
  { code: "by", name: "Беларусь", flag: "🇧🇾" },
  { code: "kz", name: "Казахстан", flag: "🇰🇿" },
  { code: "ua", name: "Украина", flag: "🇺🇦" },
  { code: "cn", name: "Китай", flag: "🇨🇳" },
  { code: "ir", name: "Иран", flag: "🇮🇷" },
  { code: "tr", name: "Турция", flag: "🇹🇷" },
  { code: "uz", name: "Узбекистан", flag: "🇺🇿" },
  { code: "am", name: "Армения", flag: "🇦🇲" },
  { code: "ge", name: "Грузия", flag: "🇬🇪" },
  { code: "az", name: "Азербайджан", flag: "🇦🇿" },
  { code: "id", name: "Индонезия", flag: "🇮🇩" },
  { code: "in", name: "Индия", flag: "🇮🇳" },
  { code: "br", name: "Бразилия", flag: "🇧🇷" },
];

export const RoutingEditor: Component = () => {
  const opts = connectionOptions;

  /* Local working copies of the editable state. We commit on Save so
     the user gets a discrete "applied" cue rather than rules
     mutating per-keystroke. */
  const [countries, setCountries] = createSignal<string[]>([]);
  const [customText, setCustomText] = createSignal<string>("");
  const [pristine, setPristine] = createSignal(true);
  const [saving, setSaving] = createSignal(false);

  /* Pull initial state from the backend exactly once per opts() change.
     `pristine` resets so a backend-side change (e.g. resuming after
     suspend rolled the options) doesn't masquerade as user edit. */
  let lastSeenOptsRev = "";
  const syncFromBackend = () => {
    const o = opts();
    if (!o) return;
    const rev = JSON.stringify([o.bypass_country_codes, o.custom_bypass_rules]);
    if (rev === lastSeenOptsRev) return;
    lastSeenOptsRev = rev;
    setCountries(o.bypass_country_codes ?? []);
    setCustomText((o.custom_bypass_rules ?? []).join("\n"));
    setPristine(true);
  };
  // Reactive pull — Solid re-runs createMemo on every signal change.
  createMemo(syncFromBackend);

  const customLines = createMemo(() =>
    customText()
      .split("\n")
      .map((s) => s.trim())
      .filter((s) => s.length > 0 && !s.startsWith("#")),
  );

  /** Live preview of what custom rules compile into. Keeps the user
   *  honest before they hit Save. Mirrors `parse_custom_bypass_rules`
   *  in the Rust side so the preview is faithful. */
  const customSummary = createMemo(() => {
    let domains = 0;
    let suffixes = 0;
    let cidrs = 0;
    for (const line of customLines()) {
      if (/^([0-9]{1,3}\.){3}[0-9]{1,3}(\/\d{1,2})?$/.test(line)) {
        cidrs++;
      } else if (/^[0-9a-fA-F:]+(\/\d{1,3})?$/.test(line) && line.includes(":")) {
        cidrs++;
      } else if (line.startsWith("*.") || line.startsWith(".")) {
        suffixes++;
      } else {
        domains++;
      }
    }
    return { domains, suffixes, cidrs };
  });

  const toggleCountry = (code: string) => {
    setPristine(false);
    setCountries((prev) =>
      prev.includes(code) ? prev.filter((c) => c !== code) : [...prev, code],
    );
  };

  const onCustomEdit = (text: string) => {
    setPristine(false);
    setCustomText(text);
  };

  const apply = async () => {
    setSaving(true);
    try {
      await setRouting(countries(), customLines());
      setPristine(true);
    } finally {
      setSaving(false);
    }
  };

  const isConnected = () => connectionState().state === "connected";

  return (
    <div class="space-y-4">
      {/* Country presets */}
      <div>
        <div class="mb-2 flex items-center gap-1.5 text-[11.5px] font-medium uppercase tracking-wider text-[var(--color-fg-2)]">
          <Globe size={11} />
          {t("settings.routingCountries")}
        </div>
        <div class="hairline rounded-lg bg-[var(--color-bg-1)] p-2">
          <div class="flex flex-wrap gap-1.5">
            <For each={PRESET_COUNTRIES}>
              {(c) => (
                <CountryChip
                  code={c.code}
                  name={c.name}
                  flag={c.flag}
                  active={countries().includes(c.code)}
                  onToggle={() => toggleCountry(c.code)}
                />
              )}
            </For>
          </div>
        </div>
        <p class="mt-1.5 text-[11px] leading-snug text-[var(--color-fg-3)]">
          {t("settings.routingCountriesHint")}
        </p>
      </div>

      {/* Custom rules */}
      <div>
        <div class="mb-2 flex items-center gap-1.5 text-[11.5px] font-medium uppercase tracking-wider text-[var(--color-fg-2)]">
          <Plus size={11} />
          {t("settings.routingCustom")}
        </div>
        <textarea
          value={customText()}
          onInput={(e) => onCustomEdit(e.currentTarget.value)}
          rows={5}
          spellcheck={false}
          placeholder={"example.com\n*.bank.example\n10.0.0.0/8\n# комментарий"}
          class="hairline w-full resize-y rounded-lg bg-[var(--color-bg-1)] p-3 font-mono text-[11.5px] leading-relaxed text-[var(--color-fg-0)] placeholder:text-[var(--color-fg-3)] focus:border-[color-mix(in_srgb,var(--color-accent)_55%,transparent)] focus:outline-none"
        />
        <Show when={customLines().length > 0}>
          <div class="mt-1.5 flex flex-wrap items-center gap-2 font-mono text-[10.5px] tabular-nums text-[var(--color-fg-2)]">
            <SummaryPill label={t("settings.routingDomains")} count={customSummary().domains} />
            <SummaryPill label={t("settings.routingSuffixes")} count={customSummary().suffixes} />
            <SummaryPill label={t("settings.routingCidrs")} count={customSummary().cidrs} />
          </div>
        </Show>
        <p class="mt-1.5 text-[11px] leading-snug text-[var(--color-fg-3)]">
          {t("settings.routingCustomHint")}
        </p>
      </div>

      {/* Save bar — sticks to the bottom of the section. */}
      <div class="flex items-center justify-between gap-3 border-t border-[var(--color-line)] pt-4">
        <Show
          when={isConnected() && !pristine()}
          fallback={
            <span class="text-[11.5px] text-[var(--color-fg-3)]">
              {pristine()
                ? t("settings.routingClean")
                : t("settings.routingDirty")}
            </span>
          }
        >
          <span class="text-[11.5px] text-[var(--color-warn)]">
            {t("settings.routingReconnect")}
          </span>
        </Show>
        <button
          type="button"
          onClick={() => void apply()}
          disabled={pristine() || saving()}
          class="flex h-8 items-center gap-1.5 rounded-md bg-[var(--color-accent)] px-3 text-[12.5px] font-medium text-white transition-[background,opacity] duration-150 hover:bg-[color-mix(in_srgb,var(--color-accent)_85%,white)] disabled:cursor-not-allowed disabled:opacity-50"
        >
          <Check size={13} />
          {saving() ? t("settings.routingSaving") : t("common.save")}
        </button>
      </div>
    </div>
  );
};

const CountryChip: Component<{
  code: string;
  name: string;
  flag: string;
  active: boolean;
  onToggle: () => void;
}> = (p) => (
  <button
    type="button"
    onClick={p.onToggle}
    class="tactile-row group/chip flex h-7 items-center gap-1.5 rounded-md px-2 text-[11.5px] transition-colors"
    classList={{
      "bg-[color-mix(in_srgb,var(--color-accent)_15%,transparent)] text-[var(--color-fg-0)] ring-1 ring-inset ring-[color-mix(in_srgb,var(--color-accent)_45%,transparent)]":
        p.active,
      "bg-[var(--color-bg-2)] text-[var(--color-fg-1)] hover:bg-[var(--color-tint-2)]":
        !p.active,
    }}
  >
    <span class="text-[14px] leading-none">{p.flag}</span>
    <span class="font-medium">{p.name}</span>
    <span class="font-mono text-[10px] uppercase text-[var(--color-fg-3)]">
      {p.code}
    </span>
    <Show when={p.active}>
      <X size={10} class="text-[var(--color-fg-2)] opacity-0 group-hover/chip:opacity-100" />
    </Show>
  </button>
);

const SummaryPill: Component<{ label: string; count: number }> = (p) => (
  <Show
    when={p.count > 0}
    fallback={
      <span class="rounded bg-[var(--color-bg-1)] px-1.5 py-0.5 text-[var(--color-fg-3)]">
        {p.label}: 0
      </span>
    }
  >
    <span class="rounded bg-[color-mix(in_srgb,var(--color-good)_18%,transparent)] px-1.5 py-0.5 text-[var(--color-good)]">
      {p.label}: {p.count}
    </span>
  </Show>
);
