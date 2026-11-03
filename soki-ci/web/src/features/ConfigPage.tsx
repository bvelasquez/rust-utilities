import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  AlertTriangle,
  CheckCircle2,
  FileCode2,
  RefreshCw,
  Save,
  ShieldAlert,
  TerminalSquare,
} from "lucide-react";
import { useEffect, useState } from "react";

import { api } from "../api/client";
import type { ConfigResponse, Status } from "../api/types";
import { useToast } from "../components/Toast";
import {
  Button,
  Card,
  CardHeader,
  ErrorState,
  Spinner,
  StatCard,
} from "../components/UI";
import { formatDuration, formatPercent } from "../lib/utils";

export function ConfigPage() {
  const queryClient = useQueryClient();
  const toast = useToast();
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const config = useQuery({
    queryKey: ["config", "raw"],
    queryFn: () => api.config(true),
  });
  const status = useQuery<Status>({ queryKey: ["status"], queryFn: api.status });

  useEffect(() => {
    if (config.data?.raw && !editing) setDraft(config.data.raw);
  }, [config.data?.raw, editing]);

  const reload = useMutation({
    mutationFn: api.reloadConfig,
    onSuccess: async (next) => {
      queryClient.setQueryData(["config", "raw"], next);
      toast.push("Config reloaded", "success");
      await queryClient.invalidateQueries({ queryKey: ["projects"] });
      await queryClient.invalidateQueries({ queryKey: ["status"] });
    },
    onError: (error) =>
      toast.push(error instanceof Error ? error.message : "Reload failed", "error"),
  });

  const save = useMutation({
    mutationFn: (yaml: string) => api.saveConfig(yaml),
    onSuccess: async (result) => {
      if (result.ok) {
        toast.push("Config saved and reloaded", "success");
        setEditing(false);
        await queryClient.invalidateQueries({ queryKey: ["config"] });
        await queryClient.invalidateQueries({ queryKey: ["projects"] });
        await queryClient.invalidateQueries({ queryKey: ["status"] });
      } else {
        toast.push("Config has validation errors; not saved", "error");
        await queryClient.invalidateQueries({ queryKey: ["config"] });
      }
    },
    onError: (error) =>
      toast.push(error instanceof Error ? error.message : "Save failed", "error"),
  });

  if (config.isLoading) return <Spinner />;
  if (config.isError) return <ErrorState error={config.error} />;
  const data = config.data!;
  const errors = data.issues.filter((issue) => issue.level === "error");
  const warnings = data.issues.filter((issue) => issue.level === "warning");
  const canWrite = Boolean(status.data?.allow_config_write);

  return (
    <div className="space-y-5">
      <div className="flex flex-wrap items-end justify-between gap-3">
        <div>
          <h1 className="text-xl font-semibold tracking-tight text-zinc-100">Config</h1>
          <p className="mt-0.5 truncate font-mono text-[11px] text-zinc-500">{data.path}</p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button
            size="sm"
            variant="ghost"
            onClick={() => reload.mutate()}
            loading={reload.isPending}
          >
            <RefreshCw className="size-3.5" /> Reload
          </Button>
          {canWrite && !editing && (
            <Button size="sm" variant="primary" onClick={() => setEditing(true)}>
              <FileCode2 className="size-3.5" /> Edit YAML
            </Button>
          )}
          {editing && (
            <>
              <Button
                size="sm"
                variant="ghost"
                onClick={() => {
                  setEditing(false);
                  setDraft(data.raw ?? "");
                }}
              >
                Cancel
              </Button>
              <Button
                size="sm"
                variant="primary"
                loading={save.isPending}
                onClick={() => save.mutate(draft)}
              >
                <Save className="size-3.5" /> Save and reload
              </Button>
            </>
          )}
        </div>
      </div>

      <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
        <StatCard label="Projects" value={data.projects.length} />
        <StatCard
          label="Targets"
          value={data.projects.reduce((sum, project) => sum + project.targets.length, 0)}
        />
        <StatCard
          label="Errors"
          value={errors.length}
          tone={errors.length ? "bad" : "good"}
          icon={errors.length ? <ShieldAlert className="size-4 text-rose-400" /> : <CheckCircle2 className="size-4 text-emerald-400" />}
        />
        <StatCard label="Warnings" value={warnings.length} tone={warnings.length ? "warn" : "default"} />
      </div>

      <div className="grid gap-4 xl:grid-cols-[1.1fr_0.9fr]">
        <Card>
          <CardHeader title="Defaults" subtitle="Applied to all projects unless overridden" />
          <div className="grid gap-3 px-5 py-4 text-xs sm:grid-cols-2">
            <Field label="Projects dir" value={data.defaults.projects_dir} mono />
            <Field label="Max parallel" value={String(data.defaults.max_parallel)} />
            <Field label="History limit" value={String(data.defaults.history_limit)} />
            <Field label="Confirm deploys" value={data.defaults.confirm_deploys ? "yes" : "no"} />
            <Field
              label="TTS"
              value={
                data.defaults.speak.enabled
                  ? `enabled · ${data.defaults.speak.base_url}`
                  : "disabled"
              }
            />
            <Field label="Speak on" value={data.defaults.speak.on.join(", ")} />
          </div>
        </Card>

        <Card>
          <CardHeader title="Validation" subtitle="Paths, runners, and package scripts" />
          <div className="max-h-72 divide-y divide-zinc-800/60 overflow-y-auto">
            {data.issues.length === 0 ? (
              <div className="flex items-center gap-2 px-5 py-6 text-xs text-emerald-300">
                <CheckCircle2 className="size-4" /> No issues found
              </div>
            ) : (
              data.issues.map((issue, index) => (
                <div key={index} className="flex items-start gap-2 px-4 py-2.5 text-xs">
                  <AlertTriangle
                    className={`mt-0.5 size-3.5 shrink-0 ${
                      issue.level === "error" ? "text-rose-400" : "text-amber-400"
                    }`}
                  />
                  <div>
                    <span
                      className={`mr-2 rounded px-1.5 py-0.5 text-[10px] font-medium ${
                        issue.level === "error"
                          ? "bg-rose-400/10 text-rose-300"
                          : "bg-amber-400/10 text-amber-300"
                      }`}
                    >
                      {issue.level}
                    </span>
                    <span className="text-zinc-400">{issue.message}</span>
                  </div>
                </div>
              ))
            )}
          </div>
        </Card>
      </div>

      <Card>
        <CardHeader
          title="Projects and targets"
          subtitle="Resolved view of the current registry"
        />
        <div className="overflow-x-auto">
          <table className="w-full min-w-[760px] text-left text-xs">
            <thead className="border-b border-zinc-800 text-[10px] uppercase tracking-wider text-zinc-600">
              <tr>
                <th className="px-4 py-2.5 font-medium">Project</th>
                <th className="px-4 py-2.5 font-medium">Path</th>
                <th className="px-4 py-2.5 font-medium">Targets</th>
                <th className="px-4 py-2.5 font-medium">Runs</th>
                <th className="px-4 py-2.5 font-medium">Success</th>
                <th className="px-4 py-2.5 font-medium">Avg</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-zinc-800/60">
              {data.projects.map((project) => (
                <tr key={project.id} className="hover:bg-zinc-900/40">
                  <td className="px-4 py-3">
                    <div className="font-medium text-zinc-200">{project.name}</div>
                    <div className="font-mono text-[10px] text-zinc-600">{project.id}</div>
                  </td>
                  <td className="max-w-64 truncate px-4 py-3 font-mono text-[10px] text-zinc-500">
                    {project.path}
                  </td>
                  <td className="px-4 py-3 text-zinc-400">
                    {project.targets.map((target) => target.id).join(", ") || "—"}
                  </td>
                  <td className="px-4 py-3 text-zinc-400">{project.stats.total}</td>
                  <td className="px-4 py-3 text-zinc-400">{formatPercent(project.stats.success_rate)}</td>
                  <td className="px-4 py-3 text-zinc-400">
                    {formatDuration(project.stats.avg_duration_ms)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </Card>

      <Card>
        <CardHeader
          title={
            <span className="inline-flex items-center gap-2">
              <TerminalSquare className="size-4 text-cyan-300" /> Raw YAML
            </span>
          }
          subtitle={
            canWrite
              ? "Editable because --allow-config-write is enabled. A .bak is written before saving."
              : "Read-only. Restart with --allow-config-write to edit from the browser."
          }
        />
        {editing ? (
          <textarea
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            spellCheck={false}
            className="h-[28rem] w-full resize-y bg-[#070a10] px-4 py-3 font-mono text-[11.5px] leading-5 text-zinc-300 focus:outline-none"
          />
        ) : (
          <pre className="max-h-[28rem] overflow-auto px-4 py-3 font-mono text-[11.5px] leading-5 text-zinc-400">
            {data.raw ?? "Raw YAML is not available."}
          </pre>
        )}
      </Card>
    </div>
  );
}

function Field({ label, value, mono }: { label: string; value: string; mono?: boolean }) {
  return (
    <div>
      <div className="text-zinc-600">{label}</div>
      <div className={`mt-0.5 text-zinc-300 ${mono ? "break-all font-mono text-[11px]" : ""}`}>
        {value}
      </div>
    </div>
  );
}
