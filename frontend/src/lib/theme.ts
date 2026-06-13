/* Theme store — single source of truth for the active colour scheme.
 *
 * Persists to localStorage so the choice survives reloads, and falls back to
 * the OS preference (`prefers-color-scheme`) on first launch.
 */
import { createSignal, createEffect } from "solid-js";

export type Theme = "dark" | "light";

const STORAGE_KEY = "v2pn:theme";

function detectInitial(): Theme {
  try {
    const saved = localStorage.getItem(STORAGE_KEY) as Theme | null;
    if (saved === "dark" || saved === "light") return saved;
  } catch {
    /* ignore — private mode etc. */
  }
  if (typeof window !== "undefined" && window.matchMedia) {
    return window.matchMedia("(prefers-color-scheme: light)").matches
      ? "light"
      : "dark";
  }
  return "dark";
}

const [theme, setThemeSignal] = createSignal<Theme>(detectInitial());

// Apply attribute on every change (incl. initial).
createEffect(() => {
  const t = theme();
  if (typeof document !== "undefined") {
    document.documentElement.setAttribute("data-theme", t);
  }
  try {
    localStorage.setItem(STORAGE_KEY, t);
  } catch {
    /* ignore */
  }
});

export { theme };

export function setTheme(t: Theme) {
  setThemeSignal(t);
}

export function toggleTheme() {
  setThemeSignal((t) => (t === "dark" ? "light" : "dark"));
}
