/* Internationalisation core.
 *
 *   import { t, language, setLanguage } from "~/lib/i18n";
 *
 *   <h1>{t("nav.servers")}</h1>
 *   <p>{t("subscription.servers", { count: 12 })}</p>
 *
 * To add a new locale:
 *   1. Drop a file `src/locales/<code>.ts` exporting a `Locale`-shaped const.
 *   2. Register it in `LOCALES` below.
 * The `Locale` type is derived from the Russian dictionary, so any missing
 * key surfaces as a TypeScript error at build time.
 */

import { createSignal, createEffect } from "solid-js";
import { ru } from "~/locales/ru";
import { en } from "~/locales/en";

export const LOCALES = {
  ru: { code: "ru", label: "Русский", nativeLabel: "Русский", flag: "🇷🇺", dict: ru },
  en: { code: "en", label: "English", nativeLabel: "English", flag: "🇬🇧", dict: en },
} as const;

export type LanguageCode = keyof typeof LOCALES;
export const ALL_LANGUAGES: LanguageCode[] = Object.keys(LOCALES) as LanguageCode[];

const STORAGE_KEY = "v2pn:language";
const DEFAULT_LANGUAGE: LanguageCode = "ru";

function detectInitial(): LanguageCode {
  try {
    const saved = localStorage.getItem(STORAGE_KEY) as LanguageCode | null;
    if (saved && saved in LOCALES) return saved;
  } catch {
    /* ignore */
  }
  // Try the OS language; only auto-pick if it matches a supported locale.
  if (typeof navigator !== "undefined" && navigator.language) {
    const guess = navigator.language.slice(0, 2).toLowerCase() as LanguageCode;
    if (guess in LOCALES) return guess;
  }
  return DEFAULT_LANGUAGE;
}

const [language, setLanguageSignal] = createSignal<LanguageCode>(detectInitial());

createEffect(() => {
  const code = language();
  try {
    localStorage.setItem(STORAGE_KEY, code);
  } catch {
    /* ignore */
  }
  if (typeof document !== "undefined") {
    document.documentElement.setAttribute("lang", code);
  }
});

export { language };

export function setLanguage(code: LanguageCode) {
  if (code in LOCALES) setLanguageSignal(code);
}

/* ---------- key resolution ---------- */

type Dict = typeof ru;

/** Build dotted key paths. `Paths<{nav: { servers: string }}>` → `"nav.servers"`. */
type Paths<T, P extends string = ""> = {
  [K in keyof T & string]: T[K] extends string
    ? `${P}${K}`
    : T[K] extends object
    ? Paths<T[K], `${P}${K}.`>
    : never;
}[keyof T & string];

export type TKey = Paths<Dict>;

function lookup(dict: any, key: string): unknown {
  return key.split(".").reduce<any>((acc, part) => (acc == null ? acc : acc[part]), dict);
}

function interpolate(s: string, params?: Record<string, string | number>): string {
  if (!params) return s;
  return s.replace(/\{(\w+)\}/g, (_, name) =>
    params[name] != null ? String(params[name]) : `{${name}}`
  );
}

/** Translate. Falls back to RU if a non-base locale is missing the key. */
export function t(key: TKey, params?: Record<string, string | number>): string {
  const code = language();
  const primary = lookup(LOCALES[code].dict, key);
  if (typeof primary === "string") return interpolate(primary, params);
  const fallback = lookup(ru, key);
  if (typeof fallback === "string") return interpolate(fallback, params);
  return key; // visible breadcrumb if a key is missing everywhere
}
