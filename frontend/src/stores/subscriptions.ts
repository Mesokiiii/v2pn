/* Subscriptions store — supports an arbitrary number of subscriptions,
 * persists the list (sans ping data) to localStorage so it survives reloads.
 *
 * Each entry has its own remote URL (or `null` for paste-imports), id, and
 * cached payload. The `activeId` decides which one is rendered as "current"
 * in the main view.
 */

import { batch, createSignal } from "solid-js";
import type { ParsedSubscription } from "~/lib/api";

export type StoredSubscription = {
  /** Stable client-generated id (used as the React-style key everywhere). */
  id: string;
  /** Remote URL the data came from, or `null` for pasted text. */
  url: string | null;
  /** Decoded subscription payload — profiles + meta. */
  data: ParsedSubscription;
  /** Unix ms — when it was added or last refreshed. */
  updatedAt: number;
};

const STORAGE_KEY = "v2pn:subscriptions";
const STORAGE_ACTIVE = "v2pn:active-subscription";

function loadInitial(): { items: StoredSubscription[]; activeId: string | null } {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    const items = raw ? (JSON.parse(raw) as StoredSubscription[]) : [];
    const activeId = localStorage.getItem(STORAGE_ACTIVE);
    return {
      items: Array.isArray(items) ? items : [],
      activeId: activeId && items.some((s) => s.id === activeId) ? activeId : items[0]?.id ?? null,
    };
  } catch {
    return { items: [], activeId: null };
  }
}

const initial = loadInitial();
const [items, setItems] = createSignal<StoredSubscription[]>(initial.items);
const [activeId, setActiveIdSignal] = createSignal<string | null>(initial.activeId);

/* ---------- persistence helpers ---------- */
function persist(arr: StoredSubscription[]) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(arr));
  } catch {
    /* quota exceeded — silent */
  }
}
function persistActive(id: string | null) {
  try {
    if (id) localStorage.setItem(STORAGE_ACTIVE, id);
    else localStorage.removeItem(STORAGE_ACTIVE);
  } catch {
    /* */
  }
}

/* ---------- public API ---------- */

export { items as subscriptions, activeId };

export function activeSubscription(): StoredSubscription | null {
  const id = activeId();
  return id ? items().find((s) => s.id === id) ?? null : null;
}

function genId(): string {
  return Math.random().toString(36).slice(2, 10) + Date.now().toString(36);
}

export function addSubscription(
  data: ParsedSubscription,
  url: string | null
): StoredSubscription {
  // Dedupe by URL — refreshes existing entry instead of duplicating.
  const existing = url ? items().find((s) => s.url === url) : null;
  if (existing) {
    const updated: StoredSubscription = { ...existing, data, updatedAt: Date.now() };
    const next = items().map((s) => (s.id === existing.id ? updated : s));
    batch(() => {
      setItems(next);
      setActiveIdSignal(existing.id);
    });
    persist(next);
    persistActive(existing.id);
    return updated;
  }
  const fresh: StoredSubscription = {
    id: genId(),
    url,
    data,
    updatedAt: Date.now(),
  };
  const next = [...items(), fresh];
  batch(() => {
    setItems(next);
    setActiveIdSignal(fresh.id);
  });
  persist(next);
  persistActive(fresh.id);
  return fresh;
}

export function updateSubscriptionData(id: string, data: ParsedSubscription) {
  const next = items().map((s) =>
    s.id === id ? { ...s, data, updatedAt: Date.now() } : s
  );
  setItems(next);
  persist(next);
}

export function removeSubscription(id: string) {
  const next = items().filter((s) => s.id !== id);
  let nextActive = activeId();
  if (nextActive === id) nextActive = next[0]?.id ?? null;
  batch(() => {
    setItems(next);
    setActiveIdSignal(nextActive);
  });
  persist(next);
  persistActive(nextActive);
}

export function setActive(id: string) {
  if (items().some((s) => s.id === id)) {
    setActiveIdSignal(id);
    persistActive(id);
  }
}
