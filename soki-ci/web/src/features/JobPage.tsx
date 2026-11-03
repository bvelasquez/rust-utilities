import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ArrowLeft,
  Ban,
  Clock3,
  Copy,
  FolderKanban,
  RotateCcw,
  TerminalSquare,
  Timer,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { Link, useParams } from "react-router-dom";

import { api, logDownloadUrl } from "../api/client";
import type { Job, LogLine } from "../api/types";
import { LogViewer } from "../components/LogViewer";
import { PhaseBadge } from "../components/PhaseBadge";
import { StdinBar } from "../components/StdinBar";
import { useToast } from "../components/Toast";
import { Button, Card, CardHeader, ErrorState, Spinner } from "../components/UI";
import { useJobEvents } from "../hooks/useEvents";
import { useNow } from "../hooks/useNow";
import {
  cn,
  elapsedMs,
  formatBytes,
  formatDateTime,
  formatDuration,
  formatRelative,
  shortId,
} from "../lib/utils";

const MAX_LINES = 10_000;

function mergeLines(previous: LogLine[], incoming: LogLine[]): LogLine[] {
  const bySeq = new Map<number, LogLine>();
  for (const line of previous) bySeq.set(line.seq, line);
  for (const line of incoming) bySeq.set(line.seq, line);
  return [...bySeq.values()].sort((a, b) => a.seq - b.seq).slice(-MAX_LINES);
}

export function JobPage() {
  const { jobId = "" } = useParams();
  const queryClient = useQueryClient();
  const toast = useToast();
  const now = useNow(1000);
  const [liveJob, setLiveJob] = useState<Job | null>(null);
  const [lines, setLines] = useState<LogLine[]>([]);
  const seeded = useRef(false);

  useEffect(() => {
    setLines([]);
    setLiveJob(null);
    seeded.current = false;
  }, [jobId]);

  const jobQuery = useQuery({
    queryKey: ["job", jobId],
    queryFn: () => api.job(jobId),
    enabled: Boolean(jobId),
    refetchInterval: 10_000,
  });

  const logQuery = useQuery({
    queryKey: ["job", jobId, "logs"],
    queryFn: () => api.logs(jobId, 2_000),
    enabled: Boolean(jobId),
    refetchOnWindowFocus: false,
  });

  useEffect(() => {
    if (!seeded.current && logQuery.data) {
      seeded.current = true;
      setLines((current) => mergeLines(current, logQuery.data.lines));
    }
  }, [logQuery.data]);

  useEffect(() => {
    if (jobQuery.data?.job) {
      setLiveJob((current) => (current?.phase === jobQuery.data.job.phase ? current : jobQuery.data.job));
    }
  }, [jobQuery.data?.job]);

  const handlers = useMemo(
    () => ({
      onSnapshot: (data: { job: Job; lines: LogLine[] }) => {
        setLiveJob(data.job);
        setLines((current) => mergeLines(current, data.lines));
      },
      onLog: (line: LogLine) => {
        setLines((current) => mergeLines(current, [line]));
      },
      onResync: (data: { job: Job | null; lines: LogLine[] }) => {
        if (data.job) setLiveJob(data.job);
        setLines((current) => mergeLines(current, data.lines));
      },
      onJob: (job: Job) => {
        setLiveJob(job);
        if (job.phase === "success" || job.phase === "error" || job.phase === "cancelled" || job.phase === "interrupted") {
          void queryClient.invalidateQueries({ queryKey: ["jobs"] });
          void queryClient.invalidateQueries({ queryKey: ["projects"] });
        }
      },
    }),
    [queryClient],
  );

  const events = useJobEvents(jobId, handlers);

  const cancel = useMutation({
    mutationFn: () => api.cancel(jobId),
    onSuccess: () => toast.push("Cancellation requested", "info"),
    onError: (error) =>
      toast.push(error instanceof Error ? error.message : "Cancel failed", "error"),
  });

  const retry = useMutation({
    mutationFn: () => api.retry(jobId),
    onSuccess: async (result) => {
      toast.push(`Queued retry as ${shortId(result.job_id)}`, "success");
      await queryClient.invalidateQueries({ queryKey: ["jobs"] });
      await queryClient.invalidateQueries({ queryKey: ["projects"] });
      await queryClient.invalidateQueries({ queryKey: ["status"] });
    },
    onError: (error) =>
      toast.push(error instanceof Error ? error.message : "Retry failed", "error"),
  });

  const sendInput = useCallback(
    async (text: string) => {
      try {
        await api.sendInput(jobId, text);
        toast.push(text ? `Sent: ${text}` : "Sent empty line", "success");
      } catch (error) {
        toast.push(error instanceof Error ? error.message : "Input failed", "error");
      }
    },
    [jobId, toast],
  );

  if (jobQuery.isLoading) return <Spinner />;
  if (jobQuery.isError) return <ErrorState error={jobQuery.error} />;
  const job = liveJob ?? jobQuery.data?.job;
  if (!job) return <ErrorState error={new Error("Job not found")} />;

  const duration = elapsedMs(job, now);
  const terminal = !job.can_cancel;
  const copyId = async () => {
    await navigator.clipboard.writeText(job.id);
    toast.push("Job id copied", "success");
  };

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <Link
            to="/deployments"
            className="mb-2 inline-flex items-center gap-1 text-xs text-zinc-500 transition hover:text-zinc-300"
          >
            <ArrowLeft className="size-3.5" /> Deployments
          </Link>
          <div className="flex flex-wrap items-center gap-2">
            <h1 className="text-xl font-semibold tracking-tight text-zinc-100">
              {job.project_name} / {job.target_label}
            </h1>
            <PhaseBadge phase={job.phase} cancelRequested={job.cancel_requested} />
            {job.interrupted && (
              <span className="rounded-full border border-orange-400/30 bg-orange-400/10 px-2 py-0.5 text-[10px] font-medium text-orange-300">
                interrupted by restart
              </span>
            )}
          </div>
          <div className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-zinc-500">
            <span className="font-mono">{job.project_id}</span>
            <span className="font-mono">/{job.target_id}</span>
            <span>·</span>
            <span>{job.source}</span>
            <span>·</span>
            <span className="font-mono">{shortId(job.id)}</span>
            <span className={cn("inline-flex items-center gap-1", events.connected ? "text-emerald-400" : "text-amber-400")}>
              <span className="size-1.5 rounded-full bg-current" />
              {events.connected ? "live" : "polling"}
            </span>
          </div>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button size="sm" variant="ghost" onClick={() => void copyId()}>
            <Copy className="size-3.5" /> Copy id
          </Button>
          <a
            href={logDownloadUrl(job.id)}
            className="inline-flex items-center gap-1.5 rounded-lg border border-zinc-700 bg-zinc-900 px-2.5 py-1.5 text-xs font-medium text-zinc-100 transition hover:border-zinc-500 hover:bg-zinc-800"
          >
            <TerminalSquare className="size-3.5" /> Download log
          </a>
          {job.can_cancel && (
            <Button
              size="sm"
              variant="danger"
              loading={cancel.isPending}
              onClick={() => cancel.mutate()}
            >
              <Ban className="size-3.5" /> Cancel
            </Button>
          )}
          {terminal && (
            <Button
              size="sm"
              variant="primary"
              loading={retry.isPending}
              onClick={() => retry.mutate()}
            >
              <RotateCcw className="size-3.5" /> Retry
            </Button>
          )}
        </div>
      </div>

      <div className="grid gap-4 xl:grid-cols-[1fr_320px]">
        <div className="flex h-[70vh] min-h-[24rem] min-w-0 flex-col gap-0">
          <LogViewer job={job} lines={lines} />
          {job.can_input && <StdinBar onSubmit={sendInput} disabled={job.cancel_requested} />}
        </div>

        <div className="space-y-3">
          <Card>
            <CardHeader title="Run details" subtitle={`Job ${job.id}`} />
            <dl className="divide-y divide-zinc-800/60 text-xs">
              <Detail label="Phase" value={<PhaseBadge phase={job.phase} cancelRequested={job.cancel_requested} />} />
              <Detail
                label="Duration"
                value={
                  <span className="inline-flex items-center gap-1">
                    <Clock3 className="size-3 text-zinc-600" /> {formatDuration(duration)}
                  </span>
                }
              />
              <Detail label="Exit code" value={job.exit_code === null ? "—" : String(job.exit_code)} />
              <Detail label="Runner" value={<span className="font-mono">{job.runner}</span>} />
              <Detail label="Working dir" value={<span className="break-all font-mono">{job.cwd}</span>} />
              <Detail label="Created" value={formatDateTime(job.created_at)} />
              <Detail label="Started" value={formatDateTime(job.started_at)} />
              <Detail label="Finished" value={formatDateTime(job.finished_at)} />
              <Detail label="Log lines" value={String(lines.length)} />
              <Detail label="Log size" value={formatBytes(job.log_size_bytes)} />
              <Detail label="Log path" value={<span className="break-all font-mono">{job.log_path}</span>} />
            </dl>
          </Card>

          <Card>
            <CardHeader title="Timeline" subtitle="Queue, run, and terminal state" />
            <div className="space-y-3 px-4 py-4 text-xs">
              <TimelineItem
                label="Queued"
                time={job.created_at}
                active={job.phase === "queued"}
                done={Boolean(job.started_at) || terminal}
              />
              <TimelineItem
                label="Running"
                time={job.started_at}
                active={job.phase === "running"}
                done={Boolean(job.finished_at)}
              />
              <TimelineItem
                label={job.phase === "success" ? "Succeeded" : job.phase === "error" ? "Failed" : "Finished"}
                time={job.finished_at}
                active={false}
                done={terminal}
              />
            </div>
          </Card>

          <Card>
            <CardHeader title="Project" />
            <div className="px-4 py-3 text-xs">
              <Link
                to={`/projects/${job.project_id}`}
                className="inline-flex items-center gap-1.5 text-cyan-300 hover:underline"
              >
                <FolderKanban className="size-3.5" /> Open project
              </Link>
              <div className="mt-2 text-zinc-500">
                {job.project_name} / {job.target_label}
              </div>
            </div>
          </Card>
        </div>
      </div>
    </div>
  );
}

function Detail({ label, value }: { label: string; value: ReactNode }) {
  return (
    <div className="flex items-start justify-between gap-3 px-4 py-2.5">
      <dt className="shrink-0 text-zinc-600">{label}</dt>
      <dd className="min-w-0 text-right text-zinc-300">{value}</dd>
    </div>
  );
}

function TimelineItem({
  label,
  time,
  active,
  done,
}: {
  label: string;
  time: string | null;
  active: boolean;
  done: boolean;
}) {
  return (
    <div className="flex items-start gap-2">
      <span
        className={cn(
          "mt-1 size-2 shrink-0 rounded-full",
          active && "animate-pulse bg-cyan-400",
          done && !active && "bg-emerald-400",
          !done && !active && "border border-zinc-700 bg-zinc-900",
        )}
      />
      <div className="min-w-0 flex-1">
        <div className={cn("font-medium", active ? "text-cyan-200" : "text-zinc-300")}>{label}</div>
        <div className="text-[10px] text-zinc-600">
          {time ? formatDateTime(time) : active ? "in progress" : "pending"}
        </div>
      </div>
      {time && <Timer className="mt-0.5 size-3 text-zinc-700" />}
    </div>
  );
}
