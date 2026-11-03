import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Activity,
  ChevronRight,
  CircleDot,
  FolderKanban,
  LayoutDashboard,
  ListOrdered,
  Menu,
  Rocket,
  Settings2,
  SlidersHorizontal,
  Volume2,
  VolumeX,
  X,
} from "lucide-react";
import { useCallback, useRef, useState } from "react";
import { NavLink, Outlet } from "react-router-dom";

import { api } from "../api/client";
import type { Job, Phase, Status } from "../api/types";
import { useLogout, useSession } from "../hooks/useAuth";
import { useGlobalEvents, type GlobalEvent } from "../hooks/useEvents";
import { cn, formatDuration, shortId } from "../lib/utils";
import { useToast } from "./Toast";
import { Button } from "./UI";

const NAV = [
  { to: "/", label: "Overview", icon: LayoutDashboard, end: true },
  { to: "/projects", label: "Projects", icon: FolderKanban },
  { to: "/deployments", label: "Deployments", icon: Rocket },
  { to: "/queue", label: "Queue", icon: ListOrdered },
  { to: "/config", label: "Config", icon: Settings2 },
  { to: "/settings", label: "Settings", icon: SlidersHorizontal },
];

function activePhase(phase: Phase): boolean {
  return phase === "queued" || phase === "running";
}

export function AppLayout() {
  const queryClient = useQueryClient();
  const toast = useToast();
  const session = useSession();
  const logout = useLogout();
  const [mobileOpen, setMobileOpen] = useState(false);
  const previousPhases = useRef<Map<string, Phase>>(new Map());

  const status = useQuery<Status>({
    queryKey: ["status"],
    queryFn: api.status,
    refetchInterval: 15_000,
    retry: 1,
  });

  const onEvent = useCallback(
    (event: GlobalEvent) => {
      if (event.type === "snapshot") {
        void queryClient.invalidateQueries({ queryKey: ["status"] });
        void queryClient.invalidateQueries({ queryKey: ["jobs"] });
        void queryClient.invalidateQueries({ queryKey: ["projects"] });
        return;
      }
      if (event.type === "config") {
        void queryClient.invalidateQueries({ queryKey: ["config"] });
        void queryClient.invalidateQueries({ queryKey: ["projects"] });
        void queryClient.invalidateQueries({ queryKey: ["status"] });
        return;
      }
      if (event.type === "removed") {
        void queryClient.invalidateQueries({ queryKey: ["jobs"] });
        void queryClient.invalidateQueries({ queryKey: ["projects"] });
        void queryClient.invalidateQueries({ queryKey: ["status"] });
        return;
      }
      if (event.type === "job") {
        const job: Job = event.job;
        const previous = previousPhases.current.get(job.id);
        previousPhases.current.set(job.id, job.phase);
        if (previous && activePhase(previous) && !activePhase(job.phase)) {
          const succeeded = job.phase === "success";
          toast.push(
            `${job.project_name} / ${job.target_label} ${
              succeeded ? "succeeded" : `finished: ${job.phase}`
            }${job.duration_ms ? ` (${formatDuration(job.duration_ms)})` : ""}`,
            succeeded ? "success" : job.phase === "error" ? "error" : "info",
          );
          if ("Notification" in window && Notification.permission === "granted") {
            new Notification(
              `${job.project_name} / ${job.target_label} ${succeeded ? "succeeded" : job.phase}`,
              { body: `${shortId(job.id)} · ${formatDuration(job.duration_ms)}` },
            );
          }
        }
        void queryClient.invalidateQueries({ queryKey: ["jobs"] });
        void queryClient.invalidateQueries({ queryKey: ["projects"] });
        void queryClient.invalidateQueries({ queryKey: ["status"] });
        void queryClient.invalidateQueries({ queryKey: ["job", job.id] });
      }
    },
    [queryClient, toast],
  );

  const events = useGlobalEvents(onEvent);
  const data = status.data;

  return (
    <div className="flex min-h-screen bg-[#090c12] text-zinc-200">
      <aside
        className={cn(
          "fixed inset-y-0 left-0 z-40 flex w-64 flex-col border-r border-zinc-800/80 bg-[#0b0f17]/95 backdrop-blur transition-transform lg:static lg:translate-x-0",
          mobileOpen ? "translate-x-0" : "-translate-x-full",
        )}
      >
        <div className="flex items-center justify-between border-b border-zinc-800/80 px-4 py-4">
          <NavLink to="/" className="flex items-center gap-2.5" onClick={() => setMobileOpen(false)}>
            <div className="grid size-8 place-items-center rounded-lg border border-cyan-400/30 bg-cyan-400/10">
              <Activity className="size-4 text-cyan-300" />
            </div>
            <div>
              <div className="text-sm font-semibold tracking-tight text-zinc-100">soki-ci</div>
              <div className="text-[10px] text-zinc-500">deploy control panel</div>
            </div>
          </NavLink>
          <button
            type="button"
            className="text-zinc-500 lg:hidden"
            onClick={() => setMobileOpen(false)}
            aria-label="Close navigation"
          >
            <X className="size-5" />
          </button>
        </div>

        <nav className="flex-1 space-y-1 overflow-y-auto p-3">
          {NAV.map((item) => (
            <NavLink
              key={item.to}
              to={item.to}
              end={item.end}
              onClick={() => setMobileOpen(false)}
              className={({ isActive }) =>
                cn(
                  "group flex items-center gap-3 rounded-lg px-3 py-2 text-sm transition",
                  isActive
                    ? "bg-cyan-400/10 text-cyan-100 ring-1 ring-inset ring-cyan-400/20"
                    : "text-zinc-400 hover:bg-zinc-800/60 hover:text-zinc-100",
                )
              }
            >
              {({ isActive }) => (
                <>
                  <item.icon className={cn("size-4", isActive && "text-cyan-300")} />
                  <span className="flex-1">{item.label}</span>
                  {item.to === "/deployments" && data?.orchestrator.active ? (
                    <span className="rounded-full bg-cyan-400/15 px-1.5 py-0.5 text-[10px] font-semibold text-cyan-200">
                      {data.orchestrator.active}
                    </span>
                  ) : null}
                  {isActive && <ChevronRight className="size-3.5 text-cyan-400/60" />}
                </>
              )}
            </NavLink>
          ))}
        </nav>

        <div className="border-t border-zinc-800/80 p-3 text-[10px] text-zinc-600">
          <div className="flex items-center justify-between gap-2">
            <span className="inline-flex items-center gap-1.5">
              <CircleDot
                className={cn("size-3", events.connected ? "text-emerald-400" : "text-amber-400")}
              />
              {events.connected ? "live" : "reconnecting"}
            </span>
            <span>{data?.version ? `v${data.version}` : ""}</span>
          </div>
          <div className="mt-1 truncate font-mono">{data?.bind ?? "api"}</div>
        </div>
      </aside>

      {mobileOpen && (
        <div
          className="fixed inset-0 z-30 bg-black/60 lg:hidden"
          onClick={() => setMobileOpen(false)}
        />
      )}

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="sticky top-0 z-20 flex flex-wrap items-center gap-3 border-b border-zinc-800/80 bg-[#090c12]/90 px-4 py-3 backdrop-blur lg:px-6">
          <button
            type="button"
            className="rounded-lg border border-zinc-800 p-1.5 text-zinc-400 lg:hidden"
            onClick={() => setMobileOpen(true)}
            aria-label="Open navigation"
          >
            <Menu className="size-4" />
          </button>
          <div className="hidden items-center gap-2 text-xs text-zinc-500 sm:flex">
            <span className={cn("size-2 rounded-full", events.connected ? "bg-emerald-400" : "bg-amber-400")} />
            {events.connected ? "Live updates" : "Reconnecting…"}
          </div>
          <div className="flex flex-wrap items-center gap-2 text-xs">
            <span className="rounded-full border border-zinc-800 bg-zinc-900 px-2.5 py-1 text-zinc-400">
              {data?.orchestrator.active ?? 0} active · {data?.orchestrator.queued ?? 0} queued
            </span>
            <span className="hidden rounded-full border border-zinc-800 bg-zinc-900 px-2.5 py-1 text-zinc-400 md:inline-flex">
              {data?.orchestrator.running ?? 0}/{data?.max_parallel ?? "—"} parallel
            </span>
            <span
              className={cn(
                "inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1",
                !data?.tts.enabled
                  ? "border-zinc-800 bg-zinc-900 text-zinc-500"
                  : data.tts.reachable
                    ? "border-emerald-400/30 bg-emerald-400/10 text-emerald-300"
                    : "border-amber-400/30 bg-amber-400/10 text-amber-300",
              )}
            >
              {data?.tts.enabled ? <Volume2 className="size-3" /> : <VolumeX className="size-3" />}
              TTS {data?.tts.enabled ? (data.tts.reachable ? "on" : "offline") : "off"}
            </span>
          </div>
          <div className="ml-auto flex items-center gap-2">
            <span className="hidden max-w-56 truncate font-mono text-[10px] text-zinc-600 md:block">
              {data?.config_path}
            </span>
            {session.data?.auth_required && (
              <Button
                size="sm"
                variant="ghost"
                onClick={() => logout.mutate()}
                loading={logout.isPending}
              >
                Sign out
              </Button>
            )}
          </div>
        </header>
        <main className="min-w-0 flex-1 px-4 py-5 lg:px-6 lg:py-6">
          <Outlet />
        </main>
      </div>
    </div>
  );
}

export function useJobLookups() {
  return useQuery({
    queryKey: ["jobs", "lookup"],
    queryFn: () => api.jobs("?limit=1000"),
    staleTime: 5_000,
  });
}