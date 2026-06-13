import { Component, Show, onMount, onCleanup, createSignal } from "solid-js";
import { Power, Sun, Moon, Globe } from "lucide-solid";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { ConnectionState, ProxyProfile } from "~/lib/api";
import { theme, toggleTheme } from "~/lib/theme";
import {
  t,
  language,
  setLanguage,
  LOCALES,
  ALL_LANGUAGES,
  type LanguageCode,
} from "~/lib/i18n";

interface Props {
  connection: ConnectionState;
  selected: ProxyProfile | null;
  onToggle: () => void;
}

export const TitleBar: Component<Props> = (props) => {
  const w = getCurrentWindow();

  onMount(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.shiftKey && (e.key === "L" || e.key === "l")) {
        e.preventDefault();
        toggleTheme();
      }
    };
    window.addEventListener("keydown", onKey);
    onCleanup(() => window.removeEventListener("keydown", onKey));
  });

  const dotState = () => {
    switch (props.connection.state) {
      case "connected": return "connected";
      case "starting":
      case "stopping": return "connecting";
      case "failed": return "error";
      default: return "idle";
    }
  };

  const label = () => {
    switch (props.connection.state) {
      case "connected": return t("connection.connected");
      case "starting":  return t("connection.connecting");
      case "stopping":  return t("connection.stopping");
      case "failed":    return t("connection.failed");
      default:          return t("connection.disconnected");
    }
  };

  const isStarting = () => props.connection.state === "starting";
  const isStopping = () => props.connection.state === "stopping";
  // Pill stays clickable in Starting (=cancel). Disabled only mid-shutdown.
  const pillDisabled = () => !props.selected || isStopping();

  return (
    <header
      data-tauri-drag-region
      class="hairline-b flex h-9 shrink-0 items-center bg-[var(--color-bg-0)] pl-3 pr-0"
    >
      <div data-tauri-drag-region class="flex items-center gap-2">
        <Logo />
        <span class="text-[12px] font-medium tracking-tight text-[var(--color-fg-0)]">v2pn</span>
      </div>

      <div data-tauri-drag-region class="flex flex-1 justify-center">
        <button
          type="button"
          onClick={props.onToggle}
          disabled={pillDisabled()}
          title={
            isStarting()
              ? t("connection.cancel")
              : props.connection.state === "failed"
              ? props.connection.reason
              : undefined
          }
          class="no-drag tactile hairline group flex h-6 items-center gap-2 rounded-full bg-[var(--color-bg-1)] px-2.5 text-[11.5px] text-[var(--color-fg-1)] hover:bg-[var(--color-bg-2)] disabled:cursor-not-allowed disabled:opacity-50"
        >
          <span class="dot" data-state={dotState()} aria-hidden="true" />
          <Show
            when={props.selected}
            fallback={<span class="text-[var(--color-fg-2)]">{t("connection.noServerSelected")}</span>}
          >
            {(p) => (
              <>
                <span class="font-medium text-[var(--color-fg-0)]">{label()}</span>
                <span class="text-[var(--color-fg-2)]">·</span>
                <Show
                  when={isStarting()}
                  fallback={<span class="max-w-[180px] truncate">{p().name}</span>}
                >
                  <span class="font-medium text-[var(--color-warn)]">
                    {t("connection.cancel")}
                  </span>
                </Show>
              </>
            )}
          </Show>
          <Power
            size={11}
            class="ml-0.5 transition-colors duration-150"
            classList={{
              "text-[var(--color-warn)]": isStarting(),
              "text-[var(--color-fg-2)] group-hover:text-[var(--color-fg-0)]": !isStarting(),
            }}
          />
        </button>
      </div>

      <div class="no-drag flex items-center gap-1 pl-1">
        <LanguageToggle />
        <ThemeToggle />
        <span class="mx-1 h-4 w-px bg-[var(--color-line)]" aria-hidden="true" />
        <WinBtn label="Minimize" onClick={() => w.minimize()}>
          <MinimizeIcon />
        </WinBtn>
        <WinBtn label="Maximize" onClick={() => w.toggleMaximize()}>
          <MaximizeIcon />
        </WinBtn>
        <WinBtn label="Close" onClick={() => w.close()} danger>
          <CloseIcon />
        </WinBtn>
      </div>
    </header>
  );
};

/* ---------- pills ---------- */

const ThemeToggle: Component = () => {
  const isDark = () => theme() === "dark";
  return (
    <button
      type="button"
      onClick={toggleTheme}
      aria-label={isDark() ? t("themes.toLight") : t("themes.toDark")}
      title={`${isDark() ? t("themes.light") : t("themes.dark")}  ·  ${t("themes.hotkey")}`}
      class="tactile hairline group flex h-6 items-center gap-1.5 rounded-full bg-[var(--color-bg-1)] px-2 text-[11px] text-[var(--color-fg-1)] hover:bg-[var(--color-bg-2)] hover:text-[var(--color-fg-0)]"
    >
      <Show when={isDark()} fallback={<Moon size={12} />}>
        <Sun size={12} />
      </Show>
      <span class="font-medium">{isDark() ? t("themes.dark") : t("themes.light")}</span>
    </button>
  );
};

/** Compact language picker. Click opens a popover with the full list. */
const LanguageToggle: Component = () => {
  const [open, setOpen] = createSignal(false);
  const meta = () => LOCALES[language()];

  onMount(() => {
    const close = (e: MouseEvent) => {
      const el = e.target as HTMLElement | null;
      if (el && !el.closest("[data-language-toggle]")) setOpen(false);
    };
    window.addEventListener("click", close);
    onCleanup(() => window.removeEventListener("click", close));
  });

  return (
    <div data-language-toggle class="relative">
      <button
        type="button"
        onClick={(e) => {
          e.stopPropagation();
          setOpen((v) => !v);
        }}
        aria-label={t("settings.sectionLanguage")}
        title={t("settings.sectionLanguage")}
        class="tactile hairline group flex h-6 items-center gap-1.5 rounded-full bg-[var(--color-bg-1)] px-2 text-[11px] text-[var(--color-fg-1)] hover:bg-[var(--color-bg-2)] hover:text-[var(--color-fg-0)]"
      >
        <Globe
          size={11}
          class="text-[var(--color-fg-2)] transition-colors duration-150 group-hover:text-[var(--color-fg-0)]"
        />
        <span class="font-mono text-[10.5px] uppercase tracking-wider">{meta().code}</span>
      </button>

      <Show when={open()}>
        <div
          class="absolute right-0 top-full z-50 mt-1 w-[180px] origin-top-right overflow-hidden rounded-md border border-[var(--color-line)] bg-[var(--color-bg-2)] shadow-[0_8px_24px_-12px_rgba(0,0,0,0.4)]"
          onClick={(e) => e.stopPropagation()}
        >
          {ALL_LANGUAGES.map((code) => {
            const item = LOCALES[code];
            const active = language() === code;
            return (
              <button
                type="button"
                onClick={() => {
                  setLanguage(code as LanguageCode);
                  setOpen(false);
                }}
                class="tactile-row flex w-full items-center gap-2.5 px-3 py-2 text-left text-[12px]"
                classList={{
                  "bg-[var(--color-tint-2)] text-[var(--color-fg-0)]": active,
                  "text-[var(--color-fg-1)] hover:bg-[var(--color-tint-1)] hover:text-[var(--color-fg-0)]":
                    !active,
                }}
              >
                <span class="text-[14px] leading-none">{item.flag}</span>
                <span class="flex-1 font-medium">{item.nativeLabel}</span>
                <Show when={active}>
                  <span class="dot" data-state="connected" aria-hidden="true" />
                </Show>
              </button>
            );
          })}
        </div>
      </Show>
    </div>
  );
};

/* ---------- window controls ---------- */

const WinBtn: Component<{
  label: string; onClick: () => void; danger?: boolean; children: any;
}> = (props) => (
  <button
    type="button"
    aria-label={props.label}
    title={props.label}
    onClick={props.onClick}
    class="tactile-row grid h-9 w-[38px] place-items-center text-[var(--color-fg-1)]"
    classList={{
      "hover:bg-[var(--color-tint-2)] hover:text-[var(--color-fg-0)]": !props.danger,
      "hover:bg-[var(--color-bad)] hover:text-white": props.danger,
    }}
  >
    {props.children}
  </button>
);

const STROKE = "currentColor";
const MinimizeIcon: Component = () => (
  <svg width="10" height="10" viewBox="0 0 10 10" fill="none" aria-hidden="true">
    <line x1="1.5" y1="5" x2="8.5" y2="5" stroke={STROKE} stroke-width="1" stroke-linecap="round" />
  </svg>
);
const MaximizeIcon: Component = () => (
  <svg width="10" height="10" viewBox="0 0 10 10" fill="none" aria-hidden="true">
    <rect x="1.5" y="1.5" width="7" height="7" stroke={STROKE} stroke-width="1" rx="0.5" />
  </svg>
);
const CloseIcon: Component = () => (
  <svg width="10" height="10" viewBox="0 0 10 10" fill="none" aria-hidden="true">
    <path d="M1.8 1.8l6.4 6.4M8.2 1.8l-6.4 6.4" stroke={STROKE} stroke-width="1" stroke-linecap="round" />
  </svg>
);

const Logo: Component = () => (
  <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true">
    <path
      d="M2.5 3.5l5.5 9 5.5-9"
      stroke="currentColor"
      stroke-width="1.6"
      stroke-linecap="round"
      stroke-linejoin="round"
    />
  </svg>
);
