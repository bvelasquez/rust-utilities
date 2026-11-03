import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Activity, Gauge, Lock, ListOrdered, Rocket } from "lucide-react";
import { Link } from "react-router-dom";

import { api } from "../api/client";
import type { Job } from "../api/types";
import { JobTable } from "../components/JobTable";
import { useToast } from "../components/Toast";
import { Card, CardHeader, EmptyState, ErrorState, Spinner, StatCard } from "../components/UI";

export function QueuePage() {
  const queryClient = useQueryClient();
  const toast = useToast();
  const status = useQuery({ queryKey: ["status"], queryFn: api.status, refetchInterval: 10_000 });
  const jobs = useQuery({
    queryKey: ["jobs", "queue"],
    queryFn: () => api.jobs("?limit=1000"),
    refetchInterval: 10_000,
  });

  const cancel = useMutation({
    mutationFn: (job: Job) => api.cancel(job.id),
    onSuccess: async () => {
      toast.push("Cancellation requested", "info");
      await queryClient.invalidateQueries({ queryKey: ["jobs"] });
      await queryClient.invalidateQueries({ queryKey: ["status"] });
      await queryClient.invalidateQueries({ queryKey: ["projects"] });
    },
    onError: (error) =>
      toast.push(error instanceof Error ? error.message : "Cancel failed", "error"),
  });

  if (status.isLoading || jobs.isLoading) return <Spinner />;
  if (status.isError) return <ErrorState error={status.error} />;
  if (jobs.isError) return <ErrorState error={jobs.error} />;

  const data = status.data!;
  const running = (jobs.data?.jobs ?? []).filter((job) => job.phase === "running");
  const queued = (jobs.data?.jobs ?? []).filter((job) => job.phase === "queued");
  const utilization =
    data.max_parallel > 0 ? Math.round((data.orchestrator.running / data.max_parallel) * 100) : 0;

  return (
    <div className="space-y-5">
      <div>
        <h1 className="text-xl font-semibold tracking-tight text-zinc-100">Queue</h1>
        <p className="mt-0.5 text-xs text-zinc-500">
          Parallel slots and per-project locks. Targets of one project cannot deploy concurrently.
        </p>
      </div>

      <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
        <StatCard
          label="Running"
          value={running.length}
          hint={`${data.orchestrator.available_parallelism} slots free`}
          tone={running.length ? "warn" : "default"}
          icon={<Activity className="size-4 text-cyan-400" />}
        />
        <StatCard
          label="Queued"
          value={queued.length}
          hint="waiting for a slot"
          icon={<ListOrdered className="size-4 text-amber-400" />}
        />
        <StatCard
          label="Project locks"
          value={data.orchestrator.project_locks.length}
          hint="one active deploy per project"
          icon={<Lock className="size-4 text-fuchsia-400" />}
        />
        <StatCard
          label="Utilization"
          value={`${utilization}%`}
          hint={`${data.orchestrator.running} of ${data.max_parallel} slots`}
          tone={utilization >= 90 ? "warn" : "default"}
          icon={<Gauge className="size-4 text-emerald-400" />}
        />
      </div>

      <Card>
        <CardHeader title="Parallel slots" subtitle={`max_parallel = ${data.max_parallel}`} />
        <div className="px-5 py-4">
          <div className="flex gap-1.5">
            {Array.from({ length: Math.max(1, data.max_parallel) }).map((_, index) => (
              <div
                key={index}
                className={`h-2 flex-1 rounded-full ${
                  index < data.orchestrator.running
                    ? "bg-cyan-400"
                    : index < data.orchestrator.running + data.orchestrator.available_parallelism
                      ? "bg-zinc-700"
                      : "bg-zinc-800"
                }`}
              />
            ))}
          </div>
          <div className="mt-2 text-[11px] text-zinc-500">
            {data.orchestrator.running} running · {data.orchestrator.available_parallelism} available ·{" "}
            {data.orchestrator.queued} queued
          </div>
        </div>
      </Card>

      <div className="grid gap-4 xl:grid-cols-2">
        <Card className="min-w-0">
          <CardHeader title="Running" subtitle="Currently executing deploy processes" />
          {running.length === 0 ? (
            <EmptyState title="Nothing running" description="Start a deploy from the Projects page." />
          ) : (
            <JobTable jobs={running} onCancel={(job) => cancel.mutate(job)} compact />
          )}
        </Card>
        <Card className="min-w-0">
          <CardHeader title="Queued" subtitle="Waiting for a parallel slot or project lock" />
          {queued.length === 0 ? (
            <EmptyState title="Queue is empty" />
          ) : (
            <JobTable jobs={queued} onCancel={(job) => cancel.mutate(job)} compact />
          )}
        </Card>
      </div>

      <Card>
        <CardHeader title="Project locks" subtitle="Project-level serialization prevents shared-tree races" />
        {data.orchestrator.project_locks.length === 0 ? (
          <EmptyState title="No project locks" description="All projects are idle." />
        ) : (
          <div className="divide-y divide-zinc-800/60">
            {data.orchestrator.project_locks.map((lock) => {
              const job = [...running, ...queued].find((entry) => entry.id === lock.job_id);
              return (
                <div key={lock.project_id} className="flex items-center justify-between gap-3 px-4 py-3 text-xs">
                  <div className="flex min-w-0 items-center gap-2">
                    <Lock className="size-3.5 text-fuchsia-300" />
                    <span className="font-mono text-zinc-300">{lock.project_id}</span>
                    <span className="text-zinc-600">→</span>
                    <span className="truncate text-zinc-400">{lock.target_id}</span>
                  </div>
                  {job ? (
                    <Link
                      to={`/deployments/${job.id}`}
                      className="inline-flex items-center gap-1 text-cyan-300 hover:underline"
                    >
                      <Rocket className="size-3.5" /> open job
                    </Link>
                  ) : (
                    <span className="text-zinc-600">{lock.job_id.slice(0, 8)}</span>
                  )}
                </div>
              );
            })}
          </div>
        )}
      </Card>
    </div>
  );
}
