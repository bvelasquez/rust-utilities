import type {
  ConfigResponse,
  Job,
  JobsResponse,
  LogLine,
  Project,
  SessionResponse,
  Status,
  TtsStatus,
} from "./types";

const API_BASE_KEY = "soki.apiBase";
const TOKEN_KEY = "soki.token";
const CSRF_KEY = "soki.csrf";

export class ApiError extends Error {
  status: number;
  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}

export function getApiBase(): string {
  const stored = localStorage.getItem(API_BASE_KEY);
  if (stored !== null) return stored.replace(/\/$/, "");
  return (import.meta.env.VITE_API_BASE ?? "").replace(/\/$/, "");
}

export function setApiBase(base: string): void {
  const normalized = base.trim().replace(/\/$/, "");
  if (normalized) localStorage.setItem(API_BASE_KEY, normalized);
  else localStorage.removeItem(API_BASE_KEY);
}

export function apiUrl(path: string): string {
  return `${getApiBase()}${path}`;
}

export function getStoredToken(): string | null {
  return sessionStorage.getItem(TOKEN_KEY);
}

export function getCsrfToken(): string | null {
  return sessionStorage.getItem(CSRF_KEY);
}

export function setAuthToken(token: string | null): void {
  if (token) sessionStorage.setItem(TOKEN_KEY, token);
  else sessionStorage.removeItem(TOKEN_KEY);
}

export function setCsrfToken(token: string | null): void {
  if (token) sessionStorage.setItem(CSRF_KEY, token);
  else sessionStorage.removeItem(CSRF_KEY);
}

export function clearAuth(): void {
  sessionStorage.removeItem(TOKEN_KEY);
  sessionStorage.removeItem(CSRF_KEY);
}

async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  const headers = new Headers(init.headers);
  if (init.body && !headers.has("content-type")) {
    headers.set("content-type", "application/json");
  }
  const token = getStoredToken();
  if (token) headers.set("authorization", `Bearer ${token}`);
  const method = (init.method ?? "GET").toUpperCase();
  const csrf = getCsrfToken();
  if (csrf && method !== "GET" && method !== "HEAD") {
    headers.set("x-soki-csrf", csrf);
  }
  const response = await fetch(apiUrl(path), {
    ...init,
    headers,
    credentials: "include",
  });
  if (!response.ok) {
    let message = `${response.status} ${response.statusText}`;
    try {
      const body = await response.json();
      if (body?.error) message = body.error;
    } catch {
      // keep the HTTP status text
    }
    if (response.status === 401) {
      window.dispatchEvent(new CustomEvent("soki:auth-required"));
    }
    throw new ApiError(response.status, message);
  }
  if (response.status === 204) return undefined as T;
  const text = await response.text();
  return text ? (JSON.parse(text) as T) : (undefined as T);
}

export const api = {
  session: () => request<SessionResponse>("/api/session"),
  login: async (token: string) => {
    const result = await request<{ authenticated: boolean; csrf_token: string }>(
      "/api/session",
      { method: "POST", body: JSON.stringify({ token }) },
    );
    setAuthToken(token);
    setCsrfToken(result.csrf_token || null);
    return result;
  },
  logout: async () => {
    await request<void>("/api/session", { method: "DELETE" });
    clearAuth();
  },
  status: () => request<Status>("/api/status"),
  projects: () => request<{ projects: Project[] }>("/api/projects"),
  project: (id: string) => request<Project>(`/api/projects/${encodeURIComponent(id)}`),
  jobs: (query: string) => request<JobsResponse>(`/api/jobs${query}`),
  job: (id: string) => request<{ job: Job }>(`/api/jobs/${id}`),
  logs: (id: string, tail = 1000) =>
    request<{ job_id: string; lines: LogLine[]; count: number }>(
      `/api/jobs/${id}/logs?tail=${tail}`,
    ),
  trigger: (projectId: string, targetId: string) =>
    request<{
      job_id: string;
      project_id: string;
      build_id: string;
      status: string;
    }>(
      `/api/projects/${encodeURIComponent(projectId)}/builds/${encodeURIComponent(targetId)}`,
      { method: "POST" },
    ),
  cancel: (id: string) =>
    request<{ ok: boolean; job_id: string; status: string }>(
      `/api/jobs/${id}/cancel`,
      { method: "POST" },
    ),
  retry: (id: string) =>
    request<{ ok: boolean; job_id: string; retried_from: string }>(
      `/api/jobs/${id}/retry`,
      { method: "POST" },
    ),
  sendInput: (id: string, text: string) =>
    request<{ ok: boolean; job_id: string }>(`/api/jobs/${id}/input`, {
      method: "POST",
      body: JSON.stringify({ text }),
    }),
  reset: () => request<{ removed: number }>("/api/jobs/reset", { method: "POST" }),
  config: (raw = false) =>
    request<ConfigResponse>(`/api/config${raw ? "?raw=true" : ""}`),
  validateConfig: () => request<Record<string, unknown>>("/api/config/validate"),
  reloadConfig: () => request<ConfigResponse>("/api/config/reload", { method: "POST" }),
  saveConfig: (yaml: string) =>
    request<{ ok: boolean; config?: ConfigResponse; issues?: unknown }>("/api/config", {
      method: "PUT",
      body: JSON.stringify({ yaml }),
    }),
  tts: () => request<TtsStatus>("/api/tts"),
  testTts: (text?: string) =>
    request<{ ok: boolean; spoken: string }>("/api/tts/test", {
      method: "POST",
      body: JSON.stringify({ text: text ?? null }),
    }),
};

export function sseUrl(path: string): string {
  return apiUrl(path);
}

export function logDownloadUrl(id: string): string {
  return apiUrl(`/api/jobs/${id}/logs?download=true`);
}
