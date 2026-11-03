import { clsx, type ClassValue } from "clsx";

import type { Job, Phase } from "../api/types";

export function cn(...values: ClassValue[]): string {
  return clsx(values);
}

export function isTerminal(phase: Phase): boolean {
  return (
    phase === "success" ||
    phase === "error" ||
    phase === "cancelled" ||
    phase === "interrupted"
  );
}

export function phaseLabel(phase: Phase): string {
  switch (phase) {
    case "queued":
      return "Queued";
    case "running":
      return "Running";
    case "success":
      return "Success";
    case "error":
      return "Failed";
    case "cancelled":
      return "Cancelled";
    case "interrupted":
      return "Interrupted";
  }
}

export function phaseClass(phase: Phase): string {
  switch (phase) {
    case "queued":
      return "border-amber-400/30 bg-amber-400/10 text-amber-300";
    case "running":
      return "border-cyan-400/30 bg-cyan-400/10 text-cyan-300";
    case "success":
      return "border-emerald-400/30 bg-emerald-400/10 text-emerald-300";
    case "error":
      return "border-rose-400/30 bg-rose-400/10 text-rose-300";
    case "cancelled":
      return "border-zinc-500/30 bg-zinc-500/10 text-zinc-400";
    case "interrupted":
      return "border-orange-400/30 bg-orange-400/10 text-orange-300";
  }
}

export function formatDuration(ms: number | null | undefined): string {
  if (ms === null || ms === undefined || Number.isNaN(ms)) return "—";
  if (ms < 1000) return `${Math.max(0, Math.round(ms))}ms`;
  const seconds = Math.floor(ms / 1000);
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  const rest = seconds % 60;
  if (minutes < 60) return `${minutes}m ${rest}s`;
  const hours = Math.floor(minutes / 60);
  return `${hours}h ${minutes % 60}m`;
}

export function elapsedMs(job: Job, now = Date.now()): number | null {
  if (!job.started_at) return job.duration_ms;
  if (job.duration_ms !== null) return job.duration_ms;
  return Math.max(0, now - new Date(job.started_at).getTime());
}

export function formatRelative(iso: string | null | undefined): string {
  if (!iso) return "—";
  const then = new Date(iso).getTime();
  if (Number.isNaN(then)) return "—";
  const delta = Date.now() - then;
  const seconds = Math.round(delta / 1000);
  if (seconds < 5) return "just now";
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.round(hours / 24);
  if (days < 30) return `${days}d ago`;
  return new Date(iso).toLocaleDateString();
}

export function formatDateTime(iso: string | null | undefined): string {
  if (!iso) return "—";
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return "—";
  return date.toLocaleString();
}

export function formatTime(iso: string | null | undefined): string {
  if (!iso) return "—";
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return "—";
  return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit", hour12: false });
}

export function shortId(id: string): string {
  return id.slice(0, 8);
}

export function formatPercent(value: number): string {
  if (!Number.isFinite(value)) return "—";
  return `${value.toFixed(0)}%`;
}

export function formatBytes(bytes: number): string {
  if (!bytes) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  let value = bytes;
  let index = 0;
  while (value >= 1024 && index < units.length - 1) {
    value /= 1024;
    index += 1;
  }
  return `${value.toFixed(value >= 10 ? 0 : 1)} ${units[index]}`;
}

export function classForLogStream(stream: string): string {
  switch (stream) {
    case "stderr":
      return "text-amber-300";
    case "stdin":
      return "text-fuchsia-300";
    case "system":
      return "text-cyan-300";
    default:
      return "text-zinc-200";
  }
}

export function stripAnsi(value: string): string {
  // Strip CSI/OSC escape sequences for display; keep the raw line in the log file.
  return value
    .replace(/\u001b\][^\u0007]*(?:\u0007|\u001b\\)/g, "")
    .replace(/\u001b\[[0-?]*[ -/]*[@-~]/g, "")
    .replace(/\u001b[@-_]/g, "");
}
