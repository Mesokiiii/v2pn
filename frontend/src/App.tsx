import { Component, createMemo, createSignal, For, Show } from "solid-js";
import { TitleBar } from "./components/TitleBar";
import { Sidebar } from "./components/Sidebar";
import { ConnectionBar } from "./components/ConnectionBar";
import { SubscriptionCard } from "./components/SubscriptionCard";
import { ServerRow } from "./components/ServerRow";
import { ServerDetail } from "./components/ServerDetail";
import { ImportDialog } from "./components/ImportDialog";
import { EmptyState } from "./components/EmptyState";
import { LogsView } from "./components/LogsView";
import { SettingsView } from "./components/SettingsView";
import { WebappFallback } from "./components/WebappFallback";
import { SubscriptionSkeleton } from "./components/SubscriptionSkeleton";
import { ServerContextMenu, type ServerContextMenuTarget } from "./components/ServerContextMenu";
import { AdminPrompt } from "./components/AdminPrompt";
import { initElevation } from "./stores/elevation";
import { attachAutoReconnect } from "./stores/autoreconnect";
import { api, ProxyProfile } from "./lib/api";
import {
  attachConnectionEvents,
  connectAction,
  connectSubscriptionAction,
  connectionState,
  disconnectAction,
  isBusy,
  isConnected,
  probeAll,
  updateActiveServer,
} from "./stores/connection";
import {
  activeSubscription,
  addSubscription,
  removeSubscription,
  setActive,
  updateSubscriptionData,
} from "./stores/subscriptions";
import { t } from "./lib/i18n";

type Section = "servers" | "logs" | "settings";

const App: Component = () => {
  attachConnectionEvents();
  initElevation();
  attachAutoReconnect();

  const [section, setSection] = createSignal<Section>("servers");
  const [selected, setSelected] = createSignal<ProxyProfile | null>(null);
  const [importOpen, setImportOpen] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [fallback, setFallback] = createSignal<{ url: string; message: string } | null>(null);
  const [refreshing, setRefreshing] = createSignal(false);
  const [importing, setImporting] = createSignal<{ host: string | null } | null>(null);
  const [ctxMenu, setCtxMenu] = createSignal<ServerContextMenuTarget | null>(null);
  const [adminPrompt, setAdminPrompt] = createSignal(false);

  // Active subscription's profiles, reactively kept in sync.
  const profiles = createMemo(() => activeSubscription()?.data.profiles ?? []);

  // Auto-select the first profile when the active subscription changes.
  createMemo(() => {
    const list = profiles();
    if (list.length === 0) {
      setSelected(null);
      return;
    }
    const cur = selected();
    if (!cur || !list.some((p) => p.id === cur.id)) {
      setSelected(list[0]!);
    }
  });

  async function loadFromUrl(url: string, opts: { isRefresh?: boolean } = {}) {
    setError(null);
    setFallback(null);
    if (opts.isRefresh) setRefreshing(true);
    else {
      setSection("servers");
      let host: string | null = null;
      try { host = new URL(url).host; } catch { /* */ }
      setImporting({ host });
    }
    try {
      const res = await api.fetchSubscription(url);
      const stored = addSubscription(res, url);
      setActive(stored.id);
      void probeAll(res.profiles);
    } catch (e) {
      const msg = asError(e);
      if (
        msg.includes("HTML landing page") ||
        msg.includes("HTML page instead") ||
        msg.includes("recursed too deep")
      ) {
        setFallback({ url, message: msg });
      } else {
        setError(msg);
      }
    } finally {
      if (opts.isRefresh) setRefreshing(false);
      else setImporting(null);
    }
  }

  async function loadFromText(text: string) {
    setError(null);
    setFallback(null);
    setSection("servers");
    setImporting({ host: null });
    try {
      const res = await api.parseText(text);
      const stored = addSubscription(res, null);
      setActive(stored.id);
      void probeAll(res.profiles);
    } catch (e) {
      setError(asError(e));
    } finally {
      setImporting(null);
    }
  }

  async function refreshActive() {
    const sub = activeSubscription();
    if (!sub?.url) return;
    setRefreshing(true);
    try {
      const res = await api.fetchSubscription(sub.url);
      updateSubscriptionData(sub.id, res);
      void probeAll(res.profiles);
    } catch (e) {
      setError(asError(e));
    } finally {
      setRefreshing(false);
    }
  }

  async function toggleConnection() {
    setError(null);
    try {
      if (isConnected() || isBusy()) {
        await disconnectAction();
      } else if (selected()) {
        const sub = activeSubscription();
        if (sub && sub.data.profiles.some((p) => p.id === selected()!.id)) {
          await connectSubscriptionAction(sub.data.profiles, selected()!.id);
        } else {
          await connectAction(selected()!);
        }
      }
    } catch (e) {
      setError(asError(e));
    }
  }

  /** Activate a specific profile.
   *
   * Fast-path: if sing-box is already running with the *same* subscription,
   * we just hot-swap the selector via clash_api — no engine restart, TUN
   * adapter stays up, system proxy stays applied. ~10ms.
   *
   * Slow-path: full reconnect (stop sing-box, release guard, start fresh).
   * Used when there's no active connection or the user crossed subscription
   * boundaries. */
  async function connectTo(profile: ProxyProfile) {
    setError(null);
    setSelected(profile);
    const sub = activeSubscription();
    if (!sub) return;

    // Spam protection: while sing-box is mid-transition we ignore extra
    // clicks. The pill in TitleBar acts as Cancel during Starting.
    if (
      connectionState().state === "starting" ||
      connectionState().state === "stopping"
    ) {
      return;
    }

    try {
      // Fast-path: hot-switch within the same subscription.
      if (
        connectionState().state === "connected" &&
        sub.data.profiles.some((p) => p.id === profile.id)
      ) {
        try {
          console.info("[v2pn] switch_server →", profile.id, profile.name);
          await api.switchServer(profile.id);
          updateActiveServer(profile.id);
          console.info("[v2pn] switch_server ok");
          return;
        } catch (e) {
          console.warn("[v2pn] switch_server failed:", e);
          // Fall through — fast path failed, do a full reconnect below.
        }
      } else {
        console.info(
          "[v2pn] full reconnect: state=",
          connectionState().state,
          "in-sub:",
          sub.data.profiles.some((p) => p.id === profile.id)
        );
      }

      // Slow-path: full reconnect.
      if (isConnected() || isBusy()) {
        await disconnectAction();
      }
      await connectSubscriptionAction(sub.data.profiles, profile.id);
    } catch (e) {
      setError(asError(e));
    }
  }

  return (
    <div class="flex h-full w-full flex-col bg-[var(--color-bg-0)]">
      <TitleBar
        connection={connectionState()}
        selected={selected()}
        onToggle={toggleConnection}
      />

      <div class="flex flex-1 overflow-hidden">
        <Sidebar
          active={section()}
          onSelect={setSection}
          onAddSubscription={() => setImportOpen(true)}
          onSelectSubscription={(id) => {
            setActive(id);
            setSection("servers");
          }}
          onRemoveSubscription={removeSubscription}
        />

        <main class="flex flex-1 flex-col overflow-hidden">
          <Show when={section() === "servers"}>
            <Show
              when={!importing()}
              fallback={<SubscriptionSkeleton host={importing()?.host ?? null} />}
            >
              <Show
                when={activeSubscription()}
                fallback={<EmptyState onImport={() => setImportOpen(true)} />}
              >
              {(sub) => (
                <div class="flex h-full flex-col">
                  <div class="anim-card-enter">
                    <SubscriptionCard
                      meta={sub().data.meta}
                      count={sub().data.profiles.length}
                      onRefresh={refreshActive}
                      onImport={() => setImportOpen(true)}
                      onPing={() => void probeAll(sub().data.profiles)}
                      onTunRequiresAdmin={() => setAdminPrompt(true)}
                      refreshing={refreshing()}
                      canRefresh={sub().url != null}
                    />
                  </div>

                  <header
                    class="anim-card-enter flex items-center justify-between border-t border-[var(--color-line)] px-6 py-2.5"
                    style={{ "animation-delay": "60ms" }}
                  >
                    <div class="flex items-center gap-2">
                      <span class="text-[12px] text-[var(--color-fg-1)]">{t("servers.list")}</span>
                      <span class="tag">{profiles().length}</span>
                    </div>
                    <span class="tag">{t("servers.rtt")}</span>
                  </header>

                  <ul class="flex-1 overflow-y-auto">
                    <For each={profiles()}>
                      {(p, i) => (
                        <ServerRow
                          profile={p}
                          selected={selected()?.id === p.id}
                          onSelect={() => setSelected(p)}
                          onActivate={() => void connectTo(p)}
                          onContextMenu={(e) =>
                            setCtxMenu({ profile: p, x: e.clientX, y: e.clientY })
                          }
                          enterDelayMs={120 + i() * 28}
                        />
                      )}
                    </For>
                  </ul>
                </div>
              )}
            </Show>
            </Show>
          </Show>

          <Show when={section() === "logs"}>
            <LogsView />
          </Show>
          <Show when={section() === "settings"}>
            <SettingsView />
          </Show>
        </main>

        <Show when={selected() && section() === "servers"}>
          <ServerDetail
            profile={selected()!}
            connection={connectionState()}
            onConnect={(p) => void connectTo(p)}
            onDisconnect={() => void disconnectAction()}
          />
        </Show>
      </div>

      <ConnectionBar
        connection={connectionState()}
        selected={selected()}
        error={error()}
      />

      <ImportDialog
        open={importOpen()}
        onClose={() => setImportOpen(false)}
        onSubmitUrl={(u) => {
          setImportOpen(false);
          void loadFromUrl(u);
        }}
        onSubmitText={(t) => {
          setImportOpen(false);
          void loadFromText(t);
        }}
      />

      <WebappFallback
        info={fallback()}
        onClose={() => setFallback(null)}
        onPaste={(t) => {
          setFallback(null);
          void loadFromText(t);
        }}
      />

      <ServerContextMenu
        target={ctxMenu()}
        isConnectedTo={
          !!ctxMenu() &&
          isConnected() &&
          selected()?.id === ctxMenu()!.profile.id
        }
        onConnect={(p) => void connectTo(p)}
        onDisconnect={() => void disconnectAction()}
        onClose={() => setCtxMenu(null)}
      />

      <AdminPrompt
        open={adminPrompt()}
        onClose={() => setAdminPrompt(false)}
      />
    </div>
  );
};

function asError(e: unknown): string {
  if (typeof e === "string") return e;
  if (e && typeof e === "object" && "message" in e) {
    return String((e as { message: unknown }).message);
  }
  return String(e);
}

export default App;
