/* @refresh reload */
import { render } from "solid-js/web";

// Fonts (bundled, no external CDN — desktop apps shouldn't depend on Google).
import "@fontsource-variable/geist";
import "@fontsource-variable/geist-mono";

// Country flags as CSS classes (`fi fi-us fis`).
import "flag-icons/css/flag-icons.min.css";

import "./lib/theme";
import "./lib/i18n";
import "./styles.css";
import App from "./App";

/* Disable WebView2's default right-click menu and accidental reload. */
function isEditable(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  if (!el) return false;
  const tag = el.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || el.isContentEditable === true;
}
window.addEventListener("contextmenu", (e) => {
  if (!isEditable(e.target)) e.preventDefault();
});
window.addEventListener("keydown", (e) => {
  if (e.key === "F5") { e.preventDefault(); return; }
  if ((e.ctrlKey || e.metaKey) && (e.key === "r" || e.key === "R")) e.preventDefault();
});

const root = document.getElementById("root");
if (!root) throw new Error("root element not found");
render(() => <App />, root);
