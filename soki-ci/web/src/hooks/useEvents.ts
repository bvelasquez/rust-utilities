import { useEffect, useRef, useState } from "react";

import { sseUrl } from "../api/client";
import type { Job, LogLine } from "../api/types";

export type GlobalEvent =
  | { type: "snapshot"; jobs: Job[]; status: unknown }
  | { type: "job"; job: Job }
  | { type: "removed"; job_id: string }
  | { type: "config"; path: string }
  | { type: "heartbeat"; ts: string };

export function useGlobalEvents(onEvent: (event: GlobalEvent) => void) {
  const callback = useRef(onEvent);
  const [connected, setConnected] = useState(false);
  const [lastEventAt, setLastEventAt] = useState<number | null>(null);

  useEffect(() => {
    callback.current = onEvent;
  }, [onEvent]);

  useEffect(() => {
    const source = new EventSource(sseUrl("/api/events"), { withCredentials: true });
    source.onopen = () => setConnected(true);
    source.onerror = () => setConnected(false);

    const parse = <T,>(event: MessageEvent): T | null => {
      try {
        return JSON.parse(event.data) as T;
      } catch {
        return null;
      }
    };

    const listeners: [string, (event: MessageEvent) => void][] = [
      [
        "snapshot",
        (event) => {
          const data = parse<{ jobs: Job[]; status: unknown }>(event);
          if (data) {
            setLastEventAt(Date.now());
            callback.current({ type: "snapshot", ...data });
          }
        },
      ],
      [
        "job",
        (event) => {
          const data = parse<Job>(event);
          if (data) {
            setLastEventAt(Date.now());
            callback.current({ type: "job", job: data });
          }
        },
      ],
      [
        "removed",
        (event) => {
          const data = parse<{ job_id: string }>(event);
          if (data) {
            setLastEventAt(Date.now());
            callback.current({ type: "removed", ...data });
          }
        },
      ],
      [
        "config",
        (event) => {
          const data = parse<{ path: string }>(event);
          if (data) {
            setLastEventAt(Date.now());
            callback.current({ type: "config", ...data });
          }
        },
      ],
      [
        "heartbeat",
        (event) => {
          const data = parse<{ ts: string }>(event);
          if (data) setLastEventAt(Date.now());
        },
      ],
    ];

    for (const [name, handler] of listeners) {
      source.addEventListener(name, handler as EventListener);
    }

    return () => {
      for (const [name, handler] of listeners) {
        source.removeEventListener(name, handler as EventListener);
      }
      source.close();
    };
  }, []);

  return { connected, lastEventAt };
}

export interface JobEventHandlers {
  onSnapshot?: (data: { job: Job; lines: LogLine[]; source?: string }) => void;
  onLog?: (line: LogLine) => void;
  onResync?: (data: { job: Job | null; lines: LogLine[] }) => void;
  onJob?: (job: Job) => void;
  onEnd?: () => void;
}

export function useJobEvents(jobId: string | null, handlers: JobEventHandlers) {
  const stored = useRef(handlers);
  const [connected, setConnected] = useState(false);

  useEffect(() => {
    stored.current = handlers;
  }, [handlers]);

  useEffect(() => {
    if (!jobId) return;
    const source = new EventSource(sseUrl(`/api/jobs/${jobId}/events`), {
      withCredentials: true,
    });
    source.onopen = () => setConnected(true);
    source.onerror = () => setConnected(false);

    const parse = <T,>(event: MessageEvent): T | null => {
      try {
        return JSON.parse(event.data) as T;
      } catch {
        return null;
      }
    };

    const listeners: [string, (event: MessageEvent) => void][] = [
      [
        "snapshot",
        (event) => {
          const data = parse<{ job: Job; lines: LogLine[]; source?: string }>(event);
          if (data) stored.current.onSnapshot?.(data);
        },
      ],
      [
        "log",
        (event) => {
          const data = parse<LogLine>(event);
          if (data) stored.current.onLog?.(data);
        },
      ],
      [
        "resync",
        (event) => {
          const data = parse<{ job: Job | null; lines: LogLine[] }>(event);
          if (data) stored.current.onResync?.(data);
        },
      ],
      [
        "job",
        (event) => {
          const data = parse<Job>(event);
          if (data) stored.current.onJob?.(data);
        },
      ],
      [
        "removed",
        () => stored.current.onEnd?.(),
      ],
      [
        "end",
        () => stored.current.onEnd?.(),
      ],
    ];

    for (const [name, handler] of listeners) {
      source.addEventListener(name, handler as EventListener);
    }

    return () => {
      for (const [name, handler] of listeners) {
        source.removeEventListener(name, handler as EventListener);
      }
      source.close();
    };
  }, [jobId]);

  return { connected };
}
