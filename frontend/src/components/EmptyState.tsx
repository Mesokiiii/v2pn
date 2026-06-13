import { Component } from "solid-js";
import { t } from "~/lib/i18n";

interface Props {
  onImport: () => void;
}

export const EmptyState: Component<Props> = (props) => (
  <div class="flex flex-1 items-center justify-center px-8">
    <div class="flex max-w-[420px] flex-col items-start gap-4">
      <div class="hairline grid h-9 w-9 place-items-center rounded-md bg-[var(--color-bg-1)]">
        <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
          <path
            d="M2.5 3.5l5.5 9 5.5-9"
            stroke="currentColor"
            stroke-width="1.6"
            stroke-linecap="round"
            stroke-linejoin="round"
          />
        </svg>
      </div>

      <div>
        <h2 class="text-[15px] font-semibold tracking-tight text-[var(--color-fg-0)]">
          {t("empty.title")}
        </h2>
        <p class="mt-1 text-[12.5px] leading-relaxed text-[var(--color-fg-2)]">
          {t("empty.description", { kbd: "vless://" })}
        </p>
      </div>

      <div class="mt-1 flex items-center gap-2">
        <button
          type="button"
          onClick={props.onImport}
          class="tactile hairline h-7 rounded-md bg-[var(--color-bg-2)] px-3 text-[12px] font-medium text-[var(--color-fg-0)] hover:bg-[var(--color-bg-3)]"
        >
          {t("empty.cta")}
        </button>
        <span class="kbd">{t("empty.hotkey")}</span>
      </div>
    </div>
  </div>
);
