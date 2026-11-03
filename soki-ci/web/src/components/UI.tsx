import { Loader2, TriangleAlert } from "lucide-react";
import type { ButtonHTMLAttributes, ReactNode } from "react";

import { cn } from "../lib/utils";

type ButtonVariant = "primary" | "secondary" | "ghost" | "danger" | "warning";
type ButtonSize = "sm" | "md";

export function Button({
  className,
  variant = "secondary",
  size = "md",
  loading,
  children,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: ButtonVariant;
  size?: ButtonSize;
  loading?: boolean;
}) {
  return (
    <button
      className={cn(
        "inline-flex items-center justify-center gap-2 rounded-lg border font-medium transition focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-cyan-400 disabled:cursor-not-allowed disabled:opacity-50",
        size === "sm" ? "px-2.5 py-1.5 text-xs" : "px-3.5 py-2 text-sm",
        variant === "primary" &&
          "border-cyan-400/40 bg-cyan-400/15 text-cyan-100 hover:bg-cyan-400/25",
        variant === "secondary" &&
          "border-zinc-700 bg-zinc-900 text-zinc-100 hover:border-zinc-500 hover:bg-zinc-800",
        variant === "ghost" &&
          "border-transparent bg-transparent text-zinc-300 hover:bg-zinc-800 hover:text-zinc-100",
        variant === "danger" &&
          "border-rose-400/40 bg-rose-400/10 text-rose-200 hover:bg-rose-400/20",
        variant === "warning" &&
          "border-amber-400/40 bg-amber-400/10 text-amber-200 hover:bg-amber-400/20",
        className,
      )}
      disabled={loading || props.disabled}
      {...props}
    >
      {loading && <Loader2 className="size-4 animate-spin" />}
      {children}
    </button>
  );
}

export function Card({
  children,
  className,
}: {
  children: ReactNode;
  className?: string;
}) {
  return (
    <div
      className={cn(
        "rounded-2xl border border-zinc-800 bg-zinc-950/60 shadow-[0_1px_0_0_rgba(255,255,255,0.03)_inset]",
        className,
      )}
    >
      {children}
    </div>
  );
}

export function CardHeader({
  title,
  subtitle,
  action,
}: {
  title: ReactNode;
  subtitle?: ReactNode;
  action?: ReactNode;
}) {
  return (
    <div className="flex flex-wrap items-start justify-between gap-3 border-b border-zinc-800/80 px-5 py-4">
      <div className="min-w-0">
        <div className="text-sm font-semibold tracking-tight text-zinc-100">{title}</div>
        {subtitle && <div className="mt-0.5 text-xs text-zinc-500">{subtitle}</div>}
      </div>
      {action}
    </div>
  );
}

export function StatCard({
  label,
  value,
  hint,
  tone = "default",
  icon,
}: {
  label: string;
  value: ReactNode;
  hint?: ReactNode;
  tone?: "default" | "good" | "bad" | "warn";
  icon?: ReactNode;
}) {
  return (
    <Card className="p-4">
      <div className="flex items-center justify-between gap-2 text-xs font-medium uppercase tracking-wider text-zinc-500">
        <span>{label}</span>
        {icon}
      </div>
      <div
        className={cn(
          "mt-2 text-2xl font-semibold tracking-tight",
          tone === "default" && "text-zinc-100",
          tone === "good" && "text-emerald-300",
          tone === "bad" && "text-rose-300",
          tone === "warn" && "text-amber-300",
        )}
      >
        {value}
      </div>
      {hint && <div className="mt-1 text-xs text-zinc-500">{hint}</div>}
    </Card>
  );
}

export function Spinner({ label = "Loading…" }: { label?: string }) {
  return (
    <div className="flex items-center justify-center gap-2 py-12 text-sm text-zinc-500">
      <Loader2 className="size-4 animate-spin" />
      {label}
    </div>
  );
}

export function ErrorState({ error }: { error: unknown }) {
  return (
    <div className="flex items-start gap-3 rounded-xl border border-rose-400/30 bg-rose-950/30 px-4 py-3 text-sm text-rose-200">
      <TriangleAlert className="mt-0.5 size-4 shrink-0" />
      <div>{error instanceof Error ? error.message : String(error)}</div>
    </div>
  );
}

export function EmptyState({
  title,
  description,
  action,
}: {
  title: string;
  description?: string;
  action?: ReactNode;
}) {
  return (
    <div className="flex flex-col items-center justify-center gap-2 px-6 py-12 text-center">
      <div className="text-sm font-medium text-zinc-300">{title}</div>
      {description && <div className="max-w-md text-xs text-zinc-500">{description}</div>}
      {action && <div className="mt-2">{action}</div>}
    </div>
  );
}

export function Modal({
  open,
  title,
  children,
  onClose,
}: {
  open: boolean;
  title: string;
  children: ReactNode;
  onClose: () => void;
}) {
  if (!open) return null;
  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4 backdrop-blur-sm"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label={title}
        className="w-full max-w-lg rounded-2xl border border-zinc-700 bg-zinc-950 p-5 shadow-2xl"
      >
        <div className="text-base font-semibold text-zinc-100">{title}</div>
        <div className="mt-3">{children}</div>
      </div>
    </div>
  );
}

export function KeyHint({ children }: { children: ReactNode }) {
  return (
    <kbd className="rounded border border-zinc-700 bg-zinc-900 px-1.5 py-0.5 font-mono text-[10px] text-zinc-400">
      {children}
    </kbd>
  );
}
