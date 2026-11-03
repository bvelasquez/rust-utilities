import {
  ArrowDownToLine,
  ArrowDown,
  Copy,
  Download,
  Pause,
  Play,
  Search,
  WrapText,
} from "lucide-react";
import {
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";

import type { Job, LogLine } from "../api/types";
import { logDownloadUrl } from "../api/client";
import { classForLogStream, cn, formatTime, stripAnsi } from "../lib/utils";
import { Button } from "./UI";

type StreamFilter = "all" | "stdout" | "stderr" | "system";

function highlight(text: string, query: string): ReactNode {
  if (!query.trim()) return text;
  const escaped = query.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const parts = text.split(new RegExp(`(${escaped})`, "gi"));
  if (parts.length === 1) return text;
  return parts.map((part, index) =>
    part.toLowerCase() === query.toLowerCase() ? (
      <mark key={index} className="rounded bg-amber-300/30 px-0.5 text-amber-100">
        {part}
      </mark>
    ) : (
      <span key={index}>{part}</span>
    ),
  );
}

export function LogViewer({
  job,
  lines,
  onFollowChange,
}: {
  job: Job | null;
  lines: LogLine[];
  onFollowChange?: (following: boolean) => void;
}) {
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<StreamFilter>("all");
  const [wrap, setWrap] = useState(true);
  const [follow, setFollow] = useState(true);
  const [copied, setCopied] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const contentRef = useRef<HTMLDivElement>(null);
  const followRef = useRef(follow);

  const filtered = useMemo(
    () =>
      lines.filter((line) => {
        if (filter === "all") return true;
        if (filter === "system") return line.stream === "system" || line.stream === "stdin";
        return line.stream === filter;
      }),
    [filter, lines],
  );

  const matches = useMemo(() => {
    if (!query.trim()) return 0;
    const needle = query.toLowerCase();
    return filtered.reduce(
      (total, line) =>
        total + (line.line.toLowerCase().split(needle).length - 1),
      0,
    );
  }, [filtered, query]);

  useEffect(() => {
    followRef.current = follow;
  }, [follow]);

  // Follow the tail even when rows use content-visibility. The layout effect
  // covers the common case; the ResizeObserver corrects after the browser has
  // measured newly inserted rows.
  useLayoutEffect(() => {
    if (!follow) return;
    const element = containerRef.current;
    if (!element) return;
    element.scrollTop = element.scrollHeight;
  }, [follow, filtered.length]);

  useEffect(() => {
    const element = containerRef.current;
    const content = contentRef.current;
    if (!element || !content) return;
    const observer = new ResizeObserver(() => {
      if (followRef.current) element.scrollTop = element.scrollHeight;
    });
    observer.observe(content);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    onFollowChange?.(follow);
  }, [follow, onFollowChange]);

  const copyAll = async () => {
    const text = filtered.map((line) => line.line).join("\n");
    await navigator.clipboard.writeText(text);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  };

  return (
    <div className="relative flex h-full min-h-0 flex-1 flex-col overflow-hidden rounded-2xl border border-zinc-800 bg-[#070a10]">
      <div className="flex flex-wrap items-center gap-2 border-b border-zinc-800 bg-zinc-950/80 px-3 py-2">
        <div className="relative min-w-52 flex-1">
          <Search className="pointer-events-none absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-zinc-600" />
          <input
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Search log…"
            className="w-full rounded-lg border border-zinc-800 bg-zinc-900/80 py-1.5 pl-8 pr-3 font-mono text-xs text-zinc-200 placeholder:text-zinc-600 focus:border-cyan-400/40 focus:outline-none"
          />
        </div>
        {query.trim() && (
          <span className="text-[11px] text-zinc-500">
            {matches} match{matches === 1 ? "" : "es"}
          </span>
        )}
        <select
          value={filter}
          onChange={(event) => setFilter(event.target.value as StreamFilter)}
          className="rounded-lg border border-zinc-800 bg-zinc-900 px-2 py-1.5 text-xs text-zinc-300 focus:border-cyan-400/40 focus:outline-none"
        >
          <option value="all">All output</option>
          <option value="stdout">stdout</option>
          <option value="stderr">stderr</option>
          <option value="system">system</option>
        </select>
        <Button
          size="sm"
          variant={follow ? "primary" : "secondary"}
          onClick={() => setFollow((current) => !current)}
          title={follow ? "Pause tail" : "Follow tail"}
        >
          {follow ? <Pause className="size-3.5" /> : <Play className="size-3.5" />}
          {follow ? "Following" : "Paused"}
        </Button>
        <Button
          size="sm"
          variant={wrap ? "primary" : "secondary"}
          onClick={() => setWrap((current) => !current)}
          title="Toggle wrapping"
        >
          <WrapText className="size-3.5" />
        </Button>
        <Button size="sm" variant="ghost" onClick={() => void copyAll()} title="Copy visible log">
          <Copy className="size-3.5" />
          {copied ? "Copied" : "Copy"}
        </Button>
        {job && (
          <a
            href={logDownloadUrl(job.id)}
            className="inline-flex items-center gap-1.5 rounded-lg border border-zinc-700 bg-zinc-900 px-2.5 py-1.5 text-xs font-medium text-zinc-100 transition hover:border-zinc-500 hover:bg-zinc-800"
            title="Download raw log"
          >
            <Download className="size-3.5" />
            Download
          </a>
        )}
      </div>

      <div
        ref={containerRef}
        data-testid="log-scroll"
        onWheel={(event) => {
          if (event.deltaY < 0 && follow) setFollow(false);
        }}
        onTouchMove={() => {
          if (follow) setFollow(false);
        }}
        onScroll={(event) => {
          const element = event.currentTarget;
          const distance = element.scrollHeight - element.scrollTop - element.clientHeight;
          if (distance < 48 && !follow) setFollow(true);
        }}
        className="min-h-0 flex-1 overflow-auto font-mono text-[11.5px] leading-5"
      >
        <div ref={contentRef} data-testid="log-content">
          {filtered.length === 0 ? (
            <div className="flex h-full items-center justify-center px-6 text-center text-xs text-zinc-600">
              {lines.length === 0
                ? "No output yet. Lines will appear here as the deploy runs."
                : "No lines match the current filter."}
            </div>
          ) : (
            <div className="min-w-full py-1">
            {filtered.map((line) => (
              <div
                key={line.seq}
                className="group flex gap-2 px-3 hover:bg-white/[0.03]"
                style={{ contentVisibility: "auto", containIntrinsicSize: "20px" }}
              >
                <span className="w-8 shrink-0 select-none text-right text-[10px] text-zinc-700">
                  {line.seq}
                </span>
                <span className="hidden w-[68px] shrink-0 select-none text-[10px] text-zinc-700 sm:inline">
                  {formatTime(line.ts)}
                </span>
                <span
                  className={cn(
                    "w-8 shrink-0 select-none text-[9px] font-semibold uppercase",
                    classForLogStream(line.stream),
                  )}
                >
                  {line.stream === "stdout"
                    ? "out"
                    : line.stream === "stderr"
                      ? "err"
                      : line.stream === "stdin"
                        ? "in"
                        : "sys"}
                </span>
                <span
                  className={cn(
                    "min-w-0 flex-1",
                    wrap ? "whitespace-pre-wrap break-words" : "whitespace-pre",
                    classForLogStream(line.stream),
                  )}
                >
                  {highlight(stripAnsi(line.line), query)}
                </span>
              </div>
              ))}
            </div>
          )}
        </div>
      </div>

      {!follow && filtered.length > 0 && (
        <button
          type="button"
          onClick={() => setFollow(true)}
          className="absolute bottom-6 right-6 inline-flex items-center gap-1.5 rounded-full border border-cyan-400/40 bg-cyan-400/15 px-3 py-1.5 text-xs font-medium text-cyan-100 shadow-lg backdrop-blur transition hover:bg-cyan-400/25"
        >
          <ArrowDown className="size-3.5" />
          Jump to tail
        </button>
      )}
      {follow && (
        <div className="pointer-events-none flex items-center justify-end border-t border-zinc-800/60 px-3 py-1 text-[10px] text-zinc-600">
          <ArrowDownToLine className="mr-1 size-3" /> live tail
        </div>
      )}
    </div>
  );
}
