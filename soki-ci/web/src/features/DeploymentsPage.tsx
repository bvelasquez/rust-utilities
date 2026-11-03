import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ChevronLeft,
  ChevronRight,
  Download,
  Filter,
  RotateCcw,
  Search,
  Trash2,
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import { api } from "../api/client";
import type { Job, Phase } from "../api/types";
import { JobTable } from "../components/JobTable";
import { useToast } from "../components/Toast";
import {
  Button,
  Card,
  ErrorState,
  Modal,
  Spinner,
  StatCard,
} from "../components/UI";
import { formatDuration, formatPercent } from "../lib/utils";

const PHASES: { value: "" | Phase; label: string }[] = [
  { value: "", label: "All phases" },
  { value: "queued", label: "Queued" },
  { value: "running", label: "Running" },
  { value: "success", label: "Success" },
  { value: "error", label: "Failed" },
  { value: "cancelled", label: "Cancelled" },
  { value: "interrupted", label: "Interrupted" },
];

export function DeploymentsPage() {
  const queryClient = useQueryClient();
  const toast = useToast();
  const [q, setQ] = useState("");
  const [project, setProject] = useState("");
  const [target, setTarget] = useState("");
  const [phase, setPhase] = useState<"" | Phase>("");
  const [offset, setOffset] = useState(0);
  const [limit, setLimit] = useState(50);
  const [resetOpen, setResetOpen] = useState(false);

  const projects = useQuery({ queryKey: ["projects"], queryFn: api.projects });

  const query = useMemo(() => {
    const params = new URLSearchParams();
    params.set("limit", String(limit));
    params.set("offset", String(offset));
    if (q.trim()) params.set("q", q.trim());
    if (project) params.set("project", project);
    if (target) params.set("target", target);
    if (phase) params.set("phase", phase);
    return params.toString();
  }, [limit, offset, phase, project, q, target]);

  const jobs = useQuery({
    queryKey: ["jobs", "deployments", query],
    queryFn: () => api.jobs(`?${query}`),
    refetchInterval: 15_000,
  });

  useEffect(() => {
    setOffset(0);
  }, [q, project, target, phase, limit]);

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

  const reset = useMutation({
    mutationFn: api.reset,
    onSuccess: async (result) => {
      toast.push(`Removed ${result.removed} finished deployment(s)`, "success");
      setResetOpen(false);
      await queryClient.invalidateQueries({ queryKey: ["jobs"] });
      await queryClient.invalidateQueries({ queryKey: ["status"] });
      await queryClient.invalidateQueries({ queryKey: ["projects"] });
    },
    onError: (error) =>
      toast.push(error instanceof Error ? error.message : "Reset failed", "error"),
  });

  const selectedProject = projects.data?.projects.find((entry) => entry.id === project);
  const exportJson = () => {
    const blob = new Blob([JSON.stringify(jobs.data?.jobs ?? [], null, 2)], {
      type: "application/json",
    });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = "soki-ci-deployments.json";
    anchor.click();
    URL.revokeObjectURL(url);
  };

  if (jobs.isLoading && !jobs.data) return <Spinner />;

  return (
    <div className="space-y-5">
      <div className="flex flex-wrap items-end justify-between gap-3">
        <div>
          <h1 className="text-xl font-semibold tracking-tight text-zinc-100">Deployments</h1>
          <p className="mt-0.5 text-xs text-zinc-500">
            Persistent history with live phase updates.
          </p>
        </div>
        <div className="flex items-center gap-2">
          <Button size="sm" variant="ghost" onClick={exportJson} disabled={!jobs.data?.jobs.length}>
            <Download className="size-3.5" /> Export
          </Button>
          <Button size="sm" variant="danger" onClick={() => setResetOpen(true)}>
            <Trash2 className="size-3.5" /> Clear history
          </Button>
        </div>
      </div>

      {jobs.data && (
        <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
          <StatCard label="Matching jobs" value={jobs.data.total} hint={`${jobs.data.total_all} total`} />
          <StatCard
            label="Success rate"
            value={formatPercent(jobs.data.stats.success_rate)}
            tone={jobs.data.stats.success_rate >= 80 && jobs.data.stats.total > 0 ? "good" : "default"}
          />
          <StatCard label="Avg duration" value={formatDuration(jobs.data.stats.avg_duration_ms)} />
          <StatCard
            label="Failures"
            value={jobs.data.stats.error + jobs.data.stats.interrupted}
            tone={jobs.data.stats.error > 0 ? "bad" : "default"}
          />
        </div>
      )}

      <Card className="min-w-0">
        <div className="flex flex-wrap items-center gap-2 border-b border-zinc-800/80 px-4 py-3">
          <div className="relative min-w-48 flex-1">
            <Search className="pointer-events-none absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-zinc-600" />
            <input
              value={q}
              onChange={(event) => setQ(event.target.value)}
              placeholder="Search project, target, or job id…"
              className="w-full rounded-lg border border-zinc-800 bg-zinc-900 py-1.5 pl-8 pr-3 text-xs text-zinc-200 placeholder:text-zinc-600 focus:border-cyan-400/40 focus:outline-none"
            />
          </div>
          <div className="flex items-center gap-1.5 text-xs text-zinc-600">
            <Filter className="size-3.5" />
            <select
              value={project}
              onChange={(event) => {
                setProject(event.target.value);
                setTarget("");
              }}
              className="rounded-lg border border-zinc-800 bg-zinc-900 px-2 py-1.5 text-xs text-zinc-300 focus:border-cyan-400/40 focus:outline-none"
            >
              <option value="">All projects</option>
              {(projects.data?.projects ?? []).map((entry) => (
                <option key={entry.id} value={entry.id}>
                  {entry.name}
                </option>
              ))}
            </select>
            <select
              value={target}
              onChange={(event) => setTarget(event.target.value)}
              className="rounded-lg border border-zinc-800 bg-zinc-900 px-2 py-1.5 text-xs text-zinc-300 focus:border-cyan-400/40 focus:outline-none"
            >
              <option value="">All targets</option>
              {(selectedProject?.targets ?? []).map((entry) => (
                <option key={entry.id} value={entry.id}>
                  {entry.label}
                </option>
              ))}
            </select>
            <select
              value={phase}
              onChange={(event) => setPhase(event.target.value as "" | Phase)}
              className="rounded-lg border border-zinc-800 bg-zinc-900 px-2 py-1.5 text-xs text-zinc-300 focus:border-cyan-400/40 focus:outline-none"
            >
              {PHASES.map((entry) => (
                <option key={entry.value} value={entry.value}>
                  {entry.label}
                </option>
              ))}
            </select>
            <select
              value={limit}
              onChange={(event) => setLimit(Number(event.target.value))}
              className="rounded-lg border border-zinc-800 bg-zinc-900 px-2 py-1.5 text-xs text-zinc-300 focus:border-cyan-400/40 focus:outline-none"
            >
              {[25, 50, 100, 250].map((value) => (
                <option key={value} value={value}>
                  {value} / page
                </option>
              ))}
            </select>
          </div>
        </div>

        {jobs.isError ? (
          <div className="p-4">
            <ErrorState error={jobs.error} />
          </div>
        ) : jobs.data?.jobs.length === 0 ? (
          <div className="px-4 py-10 text-center text-xs text-zinc-500">
            No deployments match these filters.
          </div>
        ) : (
          <JobTable jobs={jobs.data?.jobs ?? []} onCancel={(job) => cancel.mutate(job)} />
        )}

        <div className="flex flex-wrap items-center justify-between gap-2 border-t border-zinc-800/80 px-4 py-3 text-xs text-zinc-500">
          <div>
            {jobs.data
              ? `${jobs.data.offset + 1}–${Math.min(
                  jobs.data.offset + jobs.data.count,
                  jobs.data.total,
                )} of ${jobs.data.total}`
              : ""}
          </div>
          <div className="flex items-center gap-1">
            <Button
              size="sm"
              variant="ghost"
              disabled={offset === 0}
              onClick={() => setOffset(Math.max(0, offset - limit))}
            >
              <ChevronLeft className="size-3.5" /> Previous
            </Button>
            <Button
              size="sm"
              variant="ghost"
              disabled={!jobs.data?.has_more}
              onClick={() => setOffset(offset + limit)}
            >
              Next <ChevronRight className="size-3.5" />
            </Button>
          </div>
        </div>
      </Card>

      <Modal open={resetOpen} title="Clear finished deployments?" onClose={() => setResetOpen(false)}>
        <p className="text-sm text-zinc-400">
          This removes finished job records and their log files. Active and queued deployments are
          kept.
        </p>
        <div className="mt-4 flex justify-end gap-2">
          <Button variant="ghost" onClick={() => setResetOpen(false)}>
            Cancel
          </Button>
          <Button variant="danger" loading={reset.isPending} onClick={() => reset.mutate()}>
            <RotateCcw className="size-4" /> Clear history
          </Button>
        </div>
      </Modal>
    </div>
  );
}
