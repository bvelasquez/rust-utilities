import { AlertTriangle, CheckCircle2, Info, X } from "lucide-react";
import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useState,
  type ReactNode,
} from "react";

import { cn } from "../lib/utils";

type Tone = "info" | "success" | "error";

interface ToastItem {
  id: number;
  message: string;
  tone: Tone;
}

interface ToastContextValue {
  push: (message: string, tone?: Tone) => void;
}

const ToastContext = createContext<ToastContextValue | null>(null);

export function ToastProvider({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<ToastItem[]>([]);

  const push = useCallback((message: string, tone: Tone = "info") => {
    const id = Date.now() + Math.random();
    setItems((current) => [...current, { id, message, tone }]);
    window.setTimeout(() => {
      setItems((current) => current.filter((item) => item.id !== id));
    }, 5000);
  }, []);

  const value = useMemo(() => ({ push }), [push]);

  return (
    <ToastContext.Provider value={value}>
      {children}
      <div className="pointer-events-none fixed bottom-4 right-4 z-[60] flex w-full max-w-sm flex-col gap-2">
        {items.map((item) => (
          <div
            key={item.id}
            className={cn(
              "pointer-events-auto flex items-start gap-3 rounded-xl border px-4 py-3 text-sm shadow-2xl backdrop-blur",
              item.tone === "success" &&
                "border-emerald-400/30 bg-emerald-950/90 text-emerald-100",
              item.tone === "error" && "border-rose-400/30 bg-rose-950/90 text-rose-100",
              item.tone === "info" && "border-zinc-700 bg-zinc-900/95 text-zinc-100",
            )}
          >
            {item.tone === "success" ? (
              <CheckCircle2 className="mt-0.5 size-4 shrink-0" />
            ) : item.tone === "error" ? (
              <AlertTriangle className="mt-0.5 size-4 shrink-0" />
            ) : (
              <Info className="mt-0.5 size-4 shrink-0" />
            )}
            <span className="min-w-0 flex-1 break-words">{item.message}</span>
            <button
              type="button"
              onClick={() =>
                setItems((current) => current.filter((entry) => entry.id !== item.id))
              }
              className="opacity-60 transition hover:opacity-100"
              aria-label="Dismiss"
            >
              <X className="size-4" />
            </button>
          </div>
        ))}
      </div>
    </ToastContext.Provider>
  );
}

export function useToast(): ToastContextValue {
  const context = useContext(ToastContext);
  if (!context) throw new Error("useToast must be used inside ToastProvider");
  return context;
}
