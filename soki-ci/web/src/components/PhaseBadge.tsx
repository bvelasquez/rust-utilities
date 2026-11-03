import { useEffect, useState } from "react";

import type { Job, Phase } from "../api/types";
import { cn, elapsedMs, formatDuration, phaseClass, phaseLabel } from "../lib/utils";

export function PhaseBadge({
  phase,
  cancelRequested,
  className,
}: {
  phase: Phase;
  cancelRequested?: boolean;
  className?: string;
}) {
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1.5 rounded-full border px-2 py-0.5 text-[11px] font-medium",
        phaseClass(phase),
        className,
      )}
    >
      <span
        className={cn(
          "size-1.5 rounded-full bg-current",
          phase === "running" && "animate-pulse",
        )}
      />
      {cancelRequested ? "Cancelling" : phaseLabel(phase)}
    </span>
  );
}

export function LiveDuration({ job }: { job: Job }) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!job.can_cancel) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [job.can_cancel, job.id]);
  return <>{formatDuration(elapsedMs(job, now))}</>;
}
