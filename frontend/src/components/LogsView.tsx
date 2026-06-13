import { Component, For, Show, createEffect, createSignal } from "solid-js";
import { Trash2 } from "lucide-solid";
import { clearLogs, logs } from "~/stores/connection";
import { t } from "~/lib/i18n";

export const LogsView: Component = () => {
  let scrollEl: HTMLDivElement | undefined;
  const [autoscroll, setAutoscroll] = createSignal(true);

  createEffect(() => {
    // re-run when log count changes
    void logs().length;
    if (autoscroll() && scrollEl) {
      requestAnimationFrame(() => {
        scrollEl!.scrollTop = scrollEl!.scrollHeight;
      });
    }
  });

  return (
    <div class="flex h-full flex-col">
      <header class="flex h-10 shrink-0 items-center justify-between border-b border-[var(--color-line)] px-6">
        <div class="flex items-center gap-3">
          <span class="text-[13px] font-medium text-[var(--color-fg-0)]">{t("logs.title")}</span>
          <span class="tag">{logs().length}</span>
        </div>
        <div class="flex items-center gap-3 text-[11.5px]">
          <label class="flex items-center gap-1.5 text-[var(--color-fg-2)]">
            <input
              type="checkbox"
              class="h-3 w-3 accent-[var(--color-accent)]"
              checked={autoscroll()}
              onChange={(e) => setAutoscroll(e.currentTarget.checked)}
            />
            {t("logs.autoscroll")}
          </label>
          <button
            type="button"
            onClick={clearLogs}
            class="tactile-row flex items-center gap-1.5 rounded-md px-2 py-1 text-[var(--color-fg-2)] hover:bg-[var(--color-tint-2)] hover:text-[var(--color-fg-0)]"
          >
            <Trash2 size={12} />
            {t("logs.clear")}
          </button>
        </div>
      </header>

      <div ref={scrollEl} class="flex-1 overflow-y-auto px-6 py-3 font-mono text-[11.5px] leading-[1.55]">
        <Show
          when={logs().length > 0}
          fallback={
            <div class="grid h-full place-items-center text-[12px] text-[var(--color-fg-3)]">
              {t("logs.waiting")}
            </div>
          }
        >
          <For each={logs()}>
            {(l) => (
              <div
                class="whitespace-pre-wrap"
                classList={{
                  "text-[var(--color-fg-0)]": l.stream === "stdout",
                  "text-[var(--color-bad)]": l.stream === "stderr",
                }}
              >
                <span
                  class="mr-2 select-none text-[10.5px] uppercase tracking-wider"
                  classList={{
                    "text-[var(--color-fg-3)]": l.stream === "stdout",
                    "text-[color-mix(in_srgb,var(--color-bad)_70%,transparent)]":
                      l.stream === "stderr",
                  }}
                >
                  {l.stream === "stdout" ? "out" : "err"}
                </span>
                {l.text}
              </div>
            )}
          </For>
        </Show>
      </div>
    </div>
  );
};
