import { Ban, ChevronRight, Clock3 } from "lucide-react";
import { Link } from "react-router-dom";

import type { Job } from "../api/types";
import { formatRelative, formatTime, shortId } from "../lib/utils";
import { LiveDuration, PhaseBadge } from "./PhaseBadge";
import { Button } from "./UI";

export function JobTable({
  jobs,
  onCancel,
  compact = false,
}: {
  jobs: Job[];
  onCancel?: (job: Job) => void;
  compact?: boolean;
}) {
  return (
    <div className="overflow-x-auto">
      <table className="w-full min-w-[720px] text-left text-xs">
        <thead className="border-b border-zinc-800 text-[10px] uppercase tracking-wider text-zinc-600">
          <tr>
            <th className="px-4 py-2.5 font-medium">Status</th>
            <th className="px-4 py-2.5 font-medium">Deployment</th>
            <th className="px-4 py-2.5 font-medium">Duration</th>
            <th className="px-4 py-2.5 font-medium">Started</th>
            {!compact && <th className="px-4 py-2.5 font-medium">Source</th>}
            <th className="px-4 py-2.5 font-medium">Job</th>
            <th className="px-4 py-2.5" />
          </tr>
        </thead>
        <tbody className="divide-y divide-zinc-800/60">
          {jobs.map((job) => (
            <tr key={job.id} className="group transition hover:bg-zinc-900/50">
              <td className="px-4 py-3 align-middle">
                <PhaseBadge phase={job.phase} cancelRequested={job.cancel_requested} />
              </td>
              <td className="px-4 py-3 align-middle">
                <Link to={`/deployments/${job.id}`} className="block min-w-0">
                  <div className="truncate font-medium text-zinc-200 group-hover:text-cyan-200">
                    {job.project_name}
                  </div>
                  <div className="truncate text-[11px] text-zinc-500">
                    {job.target_label}{" "}
                    <span className="font-mono text-zinc-600">({job.target_id})</span>
                  </div>
                </Link>
              </td>
              <td className="whitespace-nowrap px-4 py-3 align-middle text-zinc-400">
                <span className="inline-flex items-center gap-1.5">
                  <Clock3 className="size-3 text-zinc-600" />
                  <LiveDuration job={job} />
                </span>
              </td>
              <td className="whitespace-nowrap px-4 py-3 align-middle text-zinc-500">
                <div>{formatRelative(job.started_at ?? job.created_at)}</div>
                {!compact && (
                  <div className="text-[10px] text-zinc-700">
                    {formatTime(job.started_at ?? job.created_at)}
                  </div>
                )}
              </td>
              {!compact && (
                <td className="px-4 py-3 align-middle">
                  <span className="rounded border border-zinc-800 bg-zinc-900 px-1.5 py-0.5 text-[10px] text-zinc-500">
                    {job.source}
                  </span>
                </td>
              )}
              <td className="px-4 py-3 align-middle font-mono text-[10px] text-zinc-600">
                {shortId(job.id)}
              </td>
              <td className="px-4 py-3 align-middle">
                <div className="flex items-center justify-end gap-1">
                  {job.can_cancel && onCancel && (
                    <Button
                      size="sm"
                      variant="danger"
                      onClick={(event) => {
                        event.preventDefault();
                        onCancel(job);
                      }}
                      title="Cancel this deployment"
                    >
                      <Ban className="size-3.5" />
                    </Button>
                  )}
                  <Link
                    to={`/deployments/${job.id}`}
                    className="rounded-lg p-1.5 text-zinc-500 transition hover:bg-zinc-800 hover:text-zinc-200"
                    aria-label="Open deployment"
                  >
                    <ChevronRight className="size-4" />
                  </Link>
                </div>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
