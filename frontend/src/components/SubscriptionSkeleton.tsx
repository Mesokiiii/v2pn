import { Component, For } from "solid-js";
import { t } from "~/lib/i18n";

/** Placeholder shown while a subscription is being fetched + parsed.
 * Mimics the layout of SubscriptionCard + the rows table so the eventual
 * content reveals into a familiar shape without any visual jump. */
export const SubscriptionSkeleton: Component<{ host?: string | null }> = (props) => (
  <div class="flex h-full flex-col">
    {/* Card */}
    <section class="anim-card-enter flex shrink-0 flex-col gap-4 px-6 pb-5 pt-5">
      <div class="flex items-start justify-between gap-3">
        <div class="min-w-0 flex-1 space-y-2">
          <div class="skeleton h-5 w-[200px]" />
          <div class="skeleton h-3 w-[120px]" />
        </div>
        <div class="flex items-center gap-1">
          <div class="skeleton h-7 w-[68px] rounded-md" />
          <div class="skeleton h-7 w-[78px] rounded-md" />
          <div class="skeleton h-7 w-[64px] rounded-md" />
        </div>
      </div>

      <div class="flex items-center gap-4">
        <div class="skeleton h-7 w-[152px] rounded-md" />
        <div class="flex flex-1 flex-col gap-1.5">
          <div class="skeleton h-[4px] w-full" />
          <div class="flex justify-between">
            <div class="skeleton h-3 w-[100px]" />
            <div class="skeleton h-3 w-[120px]" />
          </div>
        </div>
      </div>
    </section>

    {/* Servers header */}
    <header
      class="anim-card-enter flex items-center justify-between border-t border-[var(--color-line)] px-6 py-2.5"
      style={{ "animation-delay": "60ms" }}
    >
      <div class="flex items-center gap-2">
        <span class="text-[12px] text-[var(--color-fg-1)]">{t("servers.list")}</span>
        <span class="skeleton h-3.5 w-6" />
      </div>
      <span class="tag opacity-50">{t("servers.rtt")}</span>
    </header>

    {/* Skeleton rows */}
    <ul class="flex-1 overflow-y-auto">
      <For each={[0, 1, 2, 3, 4, 5, 6]}>
        {(i) => (
          <li
            class="anim-row-enter border-b border-[var(--color-line)]"
            style={{ "animation-delay": `${120 + i * 28}ms` }}
          >
            <div class="grid w-full grid-cols-[28px_1fr_auto_72px] items-center gap-3 px-6 py-2.5">
              <div class="skeleton h-[18px] w-[18px] rounded-full" />
              <div class="skeleton h-3.5 w-[180px]" style={{ "max-width": `${180 - i * 12}px` }} />
              <div class="flex items-center gap-1.5">
                <div class="skeleton h-4 w-[42px]" />
                <div class="skeleton h-4 w-[34px]" />
                <div class="skeleton h-4 w-[58px]" />
              </div>
              <div class="ml-auto skeleton h-3 w-[44px]" />
            </div>
          </li>
        )}
      </For>
    </ul>

    {/* Status caption */}
    <div class="hairline-t flex h-7 items-center gap-2 px-6 text-[11px] text-[var(--color-fg-2)]">
      <span class="dot" data-state="connecting" />
      <span>{t("subscription.refreshing")}</span>
      <span class="text-[var(--color-fg-3)]">{props.host ?? ""}</span>
    </div>
  </div>
);
