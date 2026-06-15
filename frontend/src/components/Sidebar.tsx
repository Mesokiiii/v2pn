import { Component, For, Show } from "solid-js";
import { Globe, Terminal, Settings, Plus, X } from "lucide-solid";
import { t } from "~/lib/i18n";
import { activeId, subscriptions } from "~/stores/subscriptions";
import { confirm as confirmDialog } from "./ConfirmDialog";

type Section = "servers" | "logs" | "settings";

interface Props {
  active: Section;
  onSelect: (s: Section) => void;
  onAddSubscription: () => void;
  onSelectSubscription: (id: string) => void;
  onRemoveSubscription: (id: string) => void;
}

export const Sidebar: Component<Props> = (props) => {
  return (
    <nav class="flex w-[220px] shrink-0 flex-col border-r border-[var(--color-line)] bg-[var(--color-bg-0)]">
      <div class="flex-1 overflow-y-auto px-2 py-4">
        <NavGroup label={t("nav.workspace")}>
          <NavItem
            active={props.active === "servers"}
            onClick={() => props.onSelect("servers")}
            icon={<Globe size={13} />}
            label={t("nav.servers")}
          />
          <NavItem
            active={props.active === "logs"}
            onClick={() => props.onSelect("logs")}
            icon={<Terminal size={13} />}
            label={t("nav.logs")}
          />
        </NavGroup>

        <NavGroup
          label={t("nav.subscriptions")}
          action={
            <button
              type="button"
              onClick={props.onAddSubscription}
              class="tactile grid h-5 w-5 place-items-center rounded text-[var(--color-fg-2)] hover:bg-[var(--color-tint-2)] hover:text-[var(--color-fg-0)]"
              aria-label={t("nav.addSubscription")}
            >
              <Plus size={12} />
            </button>
          }
        >
          <Show
            when={subscriptions().length > 0}
            fallback={
              <button
                type="button"
                onClick={props.onAddSubscription}
                class="tactile-row block w-full rounded-md px-2 py-1.5 text-left text-[12px] text-[var(--color-fg-2)] hover:bg-[var(--color-tint-2)] hover:text-[var(--color-fg-1)]"
              >
                {t("nav.addSubscription")}
              </button>
            }
          >
            <For each={subscriptions()}>
              {(s) => (
                <SubscriptionItem
                  active={s.id === activeId() && props.active === "servers"}
                  title={s.data.meta.title ?? t("subscription.title")}
                  count={s.data.profiles.length}
                  onSelect={() => props.onSelectSubscription(s.id)}
                  onRemove={() => props.onRemoveSubscription(s.id)}
                />
              )}
            </For>
          </Show>
        </NavGroup>
      </div>

      <div class="border-t border-[var(--color-line)] px-2 py-2">
        <NavItem
          active={props.active === "settings"}
          onClick={() => props.onSelect("settings")}
          icon={<Settings size={13} />}
          label={t("nav.settings")}
        />
      </div>
    </nav>
  );
};

const NavGroup: Component<{ label: string; action?: any; children: any }> = (props) => (
  <div class="mb-4">
    <div class="mb-1 flex items-center justify-between px-2">
      <span class="text-[10.5px] font-medium uppercase tracking-wider text-[var(--color-fg-2)]">
        {props.label}
      </span>
      {props.action}
    </div>
    {props.children}
  </div>
);

const NavItem: Component<{
  active: boolean;
  onClick: () => void;
  icon: any;
  label: string;
}> = (props) => (
  <button
    type="button"
    onClick={props.onClick}
    class="tactile-row flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-[12.5px]"
    classList={{
      "bg-[var(--color-tint-3)] text-[var(--color-fg-0)]": props.active,
      "text-[var(--color-fg-1)] hover:bg-[var(--color-tint-1)] hover:text-[var(--color-fg-0)]":
        !props.active,
    }}
  >
    <span
      class="grid h-4 w-4 place-items-center text-[var(--color-fg-2)]"
      classList={{ "text-[var(--color-fg-0)]": props.active }}
    >
      {props.icon}
    </span>
    <span class="truncate">{props.label}</span>
  </button>
);

/** One subscription entry: dot + title + count, with hover-only remove (×). */
const SubscriptionItem: Component<{
  active: boolean;
  title: string;
  count: number;
  onSelect: () => void;
  onRemove: () => void;
}> = (props) => (
  <div class="group/sub relative">
    <button
      type="button"
      onClick={props.onSelect}
      class="tactile-row flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-[12.5px]"
      classList={{
        "bg-[var(--color-tint-3)] text-[var(--color-fg-0)]": props.active,
        "text-[var(--color-fg-1)] hover:bg-[var(--color-tint-1)] hover:text-[var(--color-fg-0)]":
          !props.active,
      }}
    >
      <span
        class="dot shrink-0"
        data-state={props.active ? "connected" : "idle"}
        aria-hidden="true"
      />
      <span class="min-w-0 flex-1 truncate">{props.title}</span>
      <span class="shrink-0 font-mono text-[10.5px] tabular-nums text-[var(--color-fg-3)] group-hover/sub:opacity-0">
        {props.count}
      </span>
    </button>

    <button
      type="button"
      onClick={async (e) => {
        e.stopPropagation();
        const ok = await confirmDialog({
          title: t("sidebar.removeConfirmTitle", { name: props.title }),
          body: t("sidebar.removeConfirmBody"),
          confirmLabel: t("sidebar.removeConfirmCta"),
          cancelLabel: t("common.cancel"),
          destructive: true,
        });
        if (ok) props.onRemove();
      }}
      class="tactile absolute right-1.5 top-1/2 grid h-5 w-5 -translate-y-1/2 place-items-center rounded text-[var(--color-fg-3)] opacity-0 transition-opacity duration-150 hover:bg-[var(--color-tint-3)] hover:text-[var(--color-bad)] group-hover/sub:opacity-100"
      aria-label={t("sidebar.removeAria")}
    >
      <X size={11} />
    </button>
  </div>
);
