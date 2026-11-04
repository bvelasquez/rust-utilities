import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Activity,
  CheckCircle2,
  Clock3,
  FolderKanban,
  Gauge,
  ListOrdered,
  Rocket,
  Star,
  TriangleAlert,
} from "lucide-react";
import { Link } from "react-router-dom";

import { api } from "../api/client";
import type { Job, Status } from "../api/types";
import { FavoritesPanel } from "../components/FavoritesPanel";
import { JobTable } from "../components/JobTable";
import { useToast } from "../components/Toast";
import {
  Card,
  CardHeader,
  EmptyState,
  ErrorState,
  Spinner,
  StatCard,
} from "../components/UI";
import { formatDuration, formatPercent, formatRelative, phaseClass, phaseLabel } from "../lib/utils";

export function OverviewPage() {
  const queryClient = useQueryClient();
  const toast = useToast();
  const status = useQuery<Status>({ queryKey: ["status"], queryFn: api.status });
  const projects = useQuery({ queryKey: ["projects"], queryFn: api.projects });
  const jobs = useQuery({
    queryKey: ["jobs", "overview"],
    queryFn: () => api.jobs("?limit=12"),
    refetchInterval: 15_000,
  });

  const cancel = useMutation({
    mutationFn: (job: Job) => api.cancel(job.id),
    onSuccess: async () => {
      toast.push("Cancellation requested", "info");
      await queryClient.invalidateQueries({ queryKey: ["jobs"] });
      await queryClient.invalidateQueries({ queryKey: ["status"] });
      await queryClient.invalidateQueries({ queryKey: ["projects"] });
    },
    onError: (error) => toast.push(error instanceof Error ? error.message : "Cancel failed", "error"),
  });

  if (status.isLoading || jobs.isLoading) return <Spinner />;
  if (status.isError) return <ErrorState error={status.error} />;
  if (jobs.isError) return <ErrorState error={jobs.error} />;

  const data = status.data!;
  const jobData = jobs.data!;
  const active = jobData.jobs.filter((job) => job.phase === "running" || job.phase === "queued");
  const recent = jobData.jobs.filter((job) => job.phase !== "running" && job.phase !== "queued");
  const stats = jobData.stats;

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-xl font-semibold tracking-tight text-zinc-100">Overview</h1>
        <p className="mt-0.5 text-xs text-zinc-500">
          Live deploy control for every project in your registry.
        </p>
      </div>

      {data.config_error && (
        <div className="flex items-start gap-2 rounded-xl border border-rose-400/30 bg-rose-950/30 px-4 py-3 text-xs text-rose-200">
          <TriangleAlert className="mt-0.5 size-4 shrink-0" />
          <span>Config error: {data.config_error}</span>
        </div>
      )}

      <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
        <StatCard
          label="Active"
          value={data.orchestrator.active}
          hint={`${data.orchestrator.running} running · ${data.orchestrator.queued} queued`}
          tone={data.orchestrator.active ? "warn" : "default"}
          icon={<Activity className="size-4 text-cyan-400" />}
        />
        <StatCard
          label="Success rate"
          value={formatPercent(stats.success_rate)}
          hint={`${stats.success} success · ${stats.error} failed`}
          tone={stats.success_rate >= 80 && stats.total > 0 ? "good" : "default"}
          icon={<CheckCircle2 className="size-4 text-emerald-400" />}
        />
        <StatCard
          label="Avg duration"
          value={formatDuration(stats.avg_duration_ms)}
          hint={`${stats.total} jobs in history`}
          icon={<Clock3 className="size-4 text-amber-400" />}
        />
        <StatCard
          label="Parallelism"
          value={`${data.orchestrator.running}/${data.max_parallel}`}
          hint={`${data.orchestrator.available_parallelism} free slots · history ${data.history_limit}`}
          icon={<Gauge className="size-4 text-fuchsia-400" />}
        />
      </div>

      <Card className="min-w-0">
        <CardHeader
          title={
            <span className="inline-flex items-center gap-2">
              <Star className="size-4 text-amber-300" /> Favorite commands
            </span>
          }
          subtitle="Pinned targets — reorder with arrows, run without opening the project"
        />
        <FavoritesPanel compact />
      </Card>

      <div className="grid gap-4 xl:grid-cols-[1.35fr_0.65fr]">
        <Card className="min-w-0">
          <CardHeader
            title={
              <span className="inline-flex items-center gap-2">
                <Rocket className="size-4 text-cyan-300" /> Active deployments
              </span>
            }
            subtitle="Queued and running jobs, newest first"
            action={
              <Link
                to="/queue"
                className="text-xs text-cyan-300 underline-offset-2 hover:underline"
              >
                Queue view
              </Link>
            }
          />
          {active.length === 0 ? (
            <EmptyState
              title="No active deployments"
              description="Run a target from Projects, or trigger a deploy from the API."
            />
          ) : (
            <JobTable jobs={active} onCancel={(job) => cancel.mutate(job)} compact />
          )}
        </Card>

        <Card className="min-w-0">
          <CardHeader
            title={
              <span className="inline-flex items-center gap-2">
                <FolderKanban className="size-4 text-fuchsia-300" /> Projects
              </span>
            }
            subtitle={`${projects.data?.projects.length ?? 0} configured`}
            action={
              <Link
                to="/projects"
                className="text-xs text-fuchsia-300 underline-offset-2 hover:underline"
              >
                All projects
              </Link>
            }
          />
          <div className="divide-y divide-zinc-800/60">
            {(projects.data?.projects ?? []).slice(0, 7).map((project) => {
              const activeTarget = project.targets.find((target) => target.active_job);
              const lastJob = project.targets
                .map((target) => target.last_job)
                .filter((job): job is Job => Boolean(job))
                .sort((a, b) => (a.created_at < b.created_at ? 1 : -1))[0];
              const phase = activeTarget?.active_job?.phase ?? lastJob?.phase;
              return (
                <Link
                  key={project.id}
                  to={`/projects/${project.id}`}
                  className="flex items-center gap-3 px-4 py-3 transition hover:bg-zinc-900/50"
                >
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-sm font-medium text-zinc-200">
                      {project.name}
                    </div>
                    <div className="truncate text-[11px] text-zinc-500">
                      {project.targets.length} targets
                      {project.group ? ` · ${project.group}` : ""}
                      {lastJob ? ` · last ${formatRelative(lastJob.finished_at)}` : ""}
                    </div>
                  </div>
                  {phase ? (
                    <span
                      className={`rounded-full border px-2 py-0.5 text-[10px] font-medium ${phaseClass(phase)}`}
                    >
                      {phaseLabel(phase)}
                    </span>
                  ) : (
                    <span className="text-[10px] text-zinc-600">never run</span>
                  )}
                </Link>
              );
            })}
            {(projects.data?.projects.length ?? 0) === 0 && (
              <EmptyState
                title="No projects"
                description="Add projects to projects.yaml, then reload the config."
              />
            )}
          </div>
        </Card>
      </div>

      <Card className="min-w-0">
        <CardHeader
          title={
            <span className="inline-flex items-center gap-2">
              <ListOrdered className="size-4 text-amber-300" /> Recent deployments
            </span>
          }
          subtitle="Finished jobs from persistent history"
          action={
            <Link
              to="/deployments"
              className="text-xs text-amber-300 underline-offset-2 hover:underline"
            >
              Full history
            </Link>
          }
        />
        {recent.length === 0 ? (
          <EmptyState title="No finished deployments yet" />
        ) : (
          <JobTable jobs={recent} onCancel={(job) => cancel.mutate(job)} />
        )}
      </Card>

      <Card>
        <CardHeader title="System" subtitle="Service, config, and TTS status" />
        <div className="grid gap-4 px-5 py-4 text-xs sm:grid-cols-2 xl:grid-cols-4">
          <div>
            <div className="text-zinc-600">API</div>
            <div className="mt-0.5 font-mono text-zinc-300">{data.bind ?? "unknown"}</div>
          </div>
          <div>
            <div className="text-zinc-600">Config</div>
            <div className="mt-0.5 truncate font-mono text-zinc-300" title={data.config_path}>
              {data.config_path}
            </div>
          </div>
          <div>
            <div className="text-zinc-600">TTS broker</div>
            <div className="mt-0.5 text-zinc-300">
              {data.tts.enabled
                ? data.tts.reachable
                  ? `reachable · ${data.tts.base_url}`
                  : `offline · ${data.tts.base_url}`
                : "disabled"}
            </div>
          </div>
          <div>
            <div className="text-zinc-600">History</div>
            <div className="mt-0.5 text-zinc-300">
              {stats.total} jobs · limit {data.history_limit}
            </div>
          </div>
        </div>
      </Card>
    </div>
  );
}
