import { Component, onMount, Show, createSignal } from "solid-js";
import { Shield, X } from "lucide-solid";
import { t } from "~/lib/i18n";
import { restartAsAdmin } from "~/stores/elevation";

interface Props {
  open: boolean;
  onClose: () => void;
}

export const AdminPrompt: Component<Props> = (props) => {
  return (
    <Show when={props.open}>
      <Inner onClose={props.onClose} />
    </Show>
  );
};

const Inner: Component<{ onClose: () => void }> = (props) => {
  const [mounted, setMounted] = createSignal(false);
  const [busy, setBusy] = createSignal(false);
  const [err, setErr] = createSignal<string | null>(null);

  onMount(() => requestAnimationFrame(() => setMounted(true)));

  async function go() {
    setBusy(true);
    setErr(null);
    try {
      await restartAsAdmin();
      // Process should exit now; if we still get here, UAC was declined.
      setErr("UAC declined");
    } catch (e: any) {
      setErr(e?.message ?? String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div
      class="modal-overlay fixed inset-0 z-50 grid place-items-center"
      data-open={mounted()}
      onClick={props.onClose}
    >
      <div
        class="modal-card hairline relative w-[480px] max-w-[92vw] overflow-hidden rounded-[10px] bg-[var(--color-bg-1)]"
        data-open={mounted()}
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
      >
        <span class="pointer-events-none absolute inset-x-0 top-0 h-px bg-gradient-to-r from-transparent via-white/15 to-transparent" />

        <header class="flex items-start justify-between gap-3 border-b border-[var(--color-line)] px-5 pb-4 pt-4">
          <div class="flex min-w-0 items-start gap-3">
            <span class="grid h-8 w-8 shrink-0 place-items-center rounded-md border border-[color-mix(in_srgb,var(--color-warn)_45%,transparent)] bg-[color-mix(in_srgb,var(--color-warn)_10%,transparent)] text-[var(--color-warn)]">
              <Shield size={14} />
            </span>
            <div class="min-w-0">
              <h3 class="text-[14px] font-semibold tracking-tight text-[var(--color-fg-0)]">
                {t("admin.required")}
              </h3>
              <p class="mt-1 text-[12px] leading-relaxed text-[var(--color-fg-2)]">
                {t("admin.tunNeedsAdmin")}
              </p>
            </div>
          </div>
          <button
            type="button"
            onClick={props.onClose}
            aria-label={t("admin.notNow")}
            class="tactile-row -mr-1 -mt-1 grid h-7 w-7 shrink-0 place-items-center rounded-md text-[var(--color-fg-2)] hover:bg-[var(--color-tint-2)] hover:text-[var(--color-fg-0)]"
          >
            <X size={14} />
          </button>
        </header>

        <Show when={err()}>
          {(e) => (
            <div class="border-b border-[var(--color-line)] bg-[color-mix(in_srgb,var(--color-bad)_8%,transparent)] px-5 py-2 text-[11.5px] text-[var(--color-bad)]">
              {e()}
            </div>
          )}
        </Show>

        <footer class="flex items-center justify-end gap-2 bg-[var(--color-bg-0)] px-5 py-3">
          <button
            type="button"
            onClick={props.onClose}
            class="tactile hairline h-7 rounded-md px-3 text-[12px] text-[var(--color-fg-1)] hover:bg-[var(--color-tint-2)] hover:text-[var(--color-fg-0)]"
          >
            {t("admin.notNow")}
          </button>
          <button
            type="button"
            onClick={go}
            disabled={busy()}
            class="tactile h-7 rounded-md bg-[var(--color-accent)] px-3.5 text-[12px] font-medium text-white shadow-[0_1px_0_0_rgba(255,255,255,0.12)_inset,0_1px_2px_rgba(0,0,0,0.18)] hover:brightness-110 disabled:opacity-60"
          >
            {busy() ? "…" : t("admin.restart")}
          </button>
        </footer>
      </div>
    </div>
  );
};
