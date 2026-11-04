import { useCallback, useSyncExternalStore } from "react";

const STORAGE_KEY = "soki.favorites.v1";

export type FavoriteRef = {
  projectId: string;
  targetId: string;
};

export type FavoriteEntry = FavoriteRef & {
  projectName: string;
  targetLabel: string;
  runnerSummary: string;
};

type Store = {
  /** Stable sort order (favorites first everywhere). */
  order: string[];
  /** projectId:targetId */
  keys: Record<string, FavoriteEntry>;
};

export function favoriteKey(ref: FavoriteRef): string {
  return `${ref.projectId}:${ref.targetId}`;
}

function loadStore(): Store {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return { order: [], keys: {} };
    const parsed = JSON.parse(raw) as Store;
    return {
      order: Array.isArray(parsed.order) ? parsed.order : [],
      keys: parsed.keys && typeof parsed.keys === "object" ? parsed.keys : {},
    };
  } catch {
    return { order: [], keys: {} };
  }
}

function saveStore(store: Store): void {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(store));
}

let store = loadStore();
const listeners = new Set<() => void>();

function emit(): void {
  saveStore(store);
  listeners.forEach((l) => l());
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function getSnapshot(): Store {
  return store;
}

export function useFavoritesStore(): Store {
  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
}

export function isFavorite(ref: FavoriteRef): boolean {
  return Boolean(store.keys[favoriteKey(ref)]);
}

export function toggleFavorite(entry: FavoriteEntry): void {
  const key = favoriteKey(entry);
  const next = { ...store, keys: { ...store.keys }, order: [...store.order] };
  if (next.keys[key]) {
    delete next.keys[key];
    next.order = next.order.filter((k) => k !== key);
  } else {
    next.keys[key] = entry;
    next.order = [...next.order.filter((k) => k !== key), key];
  }
  store = next;
  emit();
}

export function moveFavorite(key: string, direction: -1 | 1): void {
  if (!store.keys[key]) return;
  const order = [...store.order];
  const idx = order.indexOf(key);
  if (idx < 0) return;
  const swap = idx + direction;
  if (swap < 0 || swap >= order.length) return;
  [order[idx], order[swap]] = [order[swap], order[idx]];
  store = { ...store, order };
  emit();
}

export function listFavorites(): FavoriteEntry[] {
  const seen = new Set<string>();
  const out: FavoriteEntry[] = [];
  for (const key of store.order) {
    const entry = store.keys[key];
    if (entry && !seen.has(key)) {
      seen.add(key);
      out.push(entry);
    }
  }
  for (const [key, entry] of Object.entries(store.keys)) {
    if (!seen.has(key)) out.push(entry);
  }
  return out;
}

/** Sort targets with favorites first (global order), then label. */
export function sortTargetsWithFavorites<T extends { id: string; label: string }>(
  projectId: string,
  targets: T[],
): T[] {
  const rank = new Map<string, number>();
  store.order.forEach((key, index) => {
    const prefix = `${projectId}:`;
    if (key.startsWith(prefix)) {
      rank.set(key.slice(prefix.length), index);
    }
  });
  return [...targets].sort((a, b) => {
    const ra = rank.get(a.id);
    const rb = rank.get(b.id);
    if (ra !== undefined && rb !== undefined) return ra - rb;
    if (ra !== undefined) return -1;
    if (rb !== undefined) return 1;
    return a.label.localeCompare(b.label);
  });
}

export function useFavoriteActions() {
  const data = useFavoritesStore();
  return {
    favorites: listFavorites(),
    order: data.order,
    toggleFavorite: useCallback((entry: FavoriteEntry) => toggleFavorite(entry), []),
    moveFavorite: useCallback((key: string, direction: -1 | 1) => moveFavorite(key, direction), []),
    isFavorite: useCallback((ref: FavoriteRef) => isFavorite(ref), [data]),
  };
}
