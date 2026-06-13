import { Component, createSignal, onMount, Show } from "solid-js";
import { ExternalLink, X, ClipboardPaste } from "lucide-solid";
import { open as openUrl } from "@tauri-apps/plugin-shell";
import { readText } from "@tauri-apps/plugin-clipboard-manager";

interface Props {
  info: { url: string; message: string } | null;
  onClose: () => void;
  onPaste: (text: string) => void;
}

export const WebappFallback: Component<Props> = (props) => {
  return (
    <Show when={props.info}>
      {(i) => <Inner info={i()} onClose={props.onClose} onPaste={props.onPaste} />}
    </Show>
  );
};

const Inner: Component<{
  info: { url: string; message: string };
  onClose: () => void;
  onPaste: (text: string) => void;
}> = (props) => {
  const [paste, setPaste] = createSignal("");
  const [mounted, setMounted] = createSignal(false);

  onMount(() => requestAnimationFrame(() => setMounted(true)));

  async function pasteFromClipboard() {
    try {
      const t = await readText();
      if (t) setPaste(t);
    } catch {
      /* empty */
    }
  }

  async function openInBrowser() {
    try {
      await openUrl(props.info.url);
    } catch (e) {
      console.error("openUrl failed", e);
    }
  }

  return (
    <div
      class="modal-overlay fixed inset-0 z-50 grid place-items-center"
      data-open={mounted()}
      onClick={props.onClose}
    >
      <div
        class="modal-card hairline relative w-[640px] max-w-[92vw] overflow-hidden rounded-[10px] bg-[var(--color-bg-1)]"
        data-open={mounted()}
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
      >
        <span class="pointer-events-none absolute inset-x-0 top-0 h-px bg-gradient-to-r from-transparent via-white/15 to-transparent" />

        <header class="flex items-start justify-between gap-3 border-b border-[var(--color-line)] px-5 pb-4 pt-4">
          <div class="min-w-0">
            <h3 class="text-[14px] font-semibold tracking-tight text-[var(--color-fg-0)]">
              This subscription is webapp-only
            </h3>
            <p class="mt-0.5 text-[12px] leading-relaxed text-[var(--color-fg-2)]">
              The provider's panel returned an HTML installer page rather than the
              raw subscription. We tried every common convention — none worked
              from this network. Two ways forward below.
            </p>
          </div>
          <button
            type="button"
            onClick={props.onClose}
            aria-label="Close"
            class="tactile-row -mr-1 -mt-1 grid h-7 w-7 shrink-0 place-items-center rounded-md text-[var(--color-fg-2)] hover:bg-[var(--color-tint-2)] hover:text-[var(--color-fg-0)]"
          >
            <X size={14} />
          </button>
        </header>

        <section class="border-b border-[var(--color-line)] px-5 py-4">
          <h4 class="text-[12.5px] font-semibold text-[var(--color-fg-0)]">
            Option A — open in browser, copy a real link
          </h4>
          <ol class="mt-2 list-decimal space-y-1 pl-5 text-[12px] text-[var(--color-fg-1)]">
            <li>Open the panel in your default browser.</li>
            <li>
              Right-click the <span class="kbd">Connect</span> /{" "}
              <span class="kbd">Подключить</span> button → <em>Copy link address</em>.
              Or open DevTools <span class="kbd">F12</span> → Network → find a
              request that returns <code class="font-mono">vless://…</code> /
              base64 / yaml.
            </li>
            <li>Paste that <em>actual</em> URL via the Import dialog.</li>
          </ol>
          <button
            type="button"
            onClick={openInBrowser}
            class="tactile mt-3 inline-flex h-7 items-center gap-1.5 rounded-md bg-[var(--color-bg-2)] px-3 text-[12px] font-medium text-[var(--color-fg-0)] hover:bg-[var(--color-bg-3)]"
          >
            <ExternalLink size={12} />
            Open {hostOf(props.info.url)} in browser
          </button>
        </section>

        <section class="px-5 py-4">
          <h4 class="text-[12.5px] font-semibold text-[var(--color-fg-0)]">
            Option B — paste a single share link
          </h4>
          <p class="mt-1 text-[11.5px] text-[var(--color-fg-2)]">
            Already have <code class="font-mono">vless://…</code> / trojan / hy2 /
            tuic from another client (Happ, v2rayN, sing-box)? Paste it here.
          </p>
          <textarea
            rows={4}
            spellcheck={false}
            placeholder={"vless://...\ntrojan://...\nhy2://..."}
            value={paste()}
            onInput={(e) => setPaste(e.currentTarget.value)}
            class="hairline mt-2 w-full resize-y rounded-md bg-[var(--color-bg-0)] px-3 py-2 font-mono text-[12px] leading-relaxed outline-none transition-colors duration-150 placeholder:text-[var(--color-fg-3)] focus:border-[color-mix(in_srgb,var(--color-accent)_55%,transparent)]"
          />
          <div class="mt-2 flex items-center justify-between">
            <button
              type="button"
              onClick={pasteFromClipboard}
              class="tactile-row flex items-center gap-1.5 rounded-md px-2 py-1 text-[11.5px] text-[var(--color-fg-2)] hover:bg-[var(--color-tint-2)] hover:text-[var(--color-fg-0)]"
            >
              <ClipboardPaste size={12} />
              Paste from clipboard
            </button>
            <button
              type="button"
              onClick={() => paste().trim() && props.onPaste(paste())}
              disabled={!paste().trim()}
              class="tactile h-7 rounded-md bg-[var(--color-accent)] px-3.5 text-[12px] font-medium text-white disabled:opacity-50 hover:brightness-110"
            >
              Import
            </button>
          </div>
        </section>

        <footer class="border-t border-[var(--color-line)] bg-[var(--color-bg-0)] px-5 py-2.5 text-[10.5px] font-mono text-[var(--color-fg-3)]">
          source: {props.info.url}
        </footer>
      </div>
    </div>
  );
};

function hostOf(u: string): string {
  try {
    return new URL(u).host;
  } catch {
    return u;
  }
}
