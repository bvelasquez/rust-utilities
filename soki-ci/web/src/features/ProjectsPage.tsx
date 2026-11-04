import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ArrowLeft,
  Boxes,
  FolderKanban,
  Play,
  Search,
  ShieldAlert,
  TerminalSquare,
} from "lucide-react";
import { useMemo, useState } from "react";
import { Link, useParams } from "react-router-dom";

import { api } from "../api/client";
import type { Job, Project, Target } from "../api/types";
import { FavoriteStar } from "../components/FavoriteStar";
import { PhaseBadge, LiveDuration } from "../components/PhaseBadge";
import { sortTargetsWithFavorites } from "../lib/favorites";
import { useToast } from "../components/Toast";
import {
  Button,
  Card,
  CardHeader,
  EmptyState,
  ErrorState,
  Modal,
  Spinner,
  StatCard,
} from "../components/UI";
import {
  cn,
  formatDuration,
  formatPercent,
  formatRelative,
  phaseClass,
  phaseLabel,
} from "../lib/utils";

export function ProjectsPage() {
  const projects = useQuery({ queryKey: ["projects"], queryFn: api.projects });
  const [search, setSearch] = useState("");

  const grouped = useMemo(() => {
    const filtered = (projects.data?.projects ?? []).filter((project) => {
      const needle = search.toLowerCase();
      return (
        !needle ||
        project.name.toLowerCase().includes(needle) ||
        project.id.toLowerCase().includes(needle) ||
        (project.group ?? "").toLowerCase().includes(needle) ||
        project.tags.some((tag) => tag.toLowerCase().includes(needle))
      );
    });
    const groups = new Map<string, Project[]>();
    for (const project of filtered) {
      const key = project.group ?? "Ungrouped";
      groups.set(key, [...(groups.get(key) ?? []), project]);
    }
    return [...groups.entries()].sort(([a], [b]) => a.localeCompare(b));
  }, [projects.data?.projects, search]);

  if (projects.isLoading) return <Spinner />;
  if (projects.isError) return <ErrorState error={projects.error} />;

  return (
    <div className="space-y-5">
      <div className="flex flex-wrap items-end justify-between gap-3">
        <div>
          <h1 className="text-xl font-semibold tracking-tight text-zinc-100">Projects</h1>
          <p className="mt-0.5 text-xs text-zinc-500">
            {projects.data?.projects.length ?? 0} projects ·{" "}
            {(projects.data?.projects ?? []).reduce((sum, project) => sum + project.targets.length, 0)}{" "}
            deploy targets
          </p>
        </div>
        <div className="relative w-full max-w-sm">
          <Search className="pointer-events-none absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-zinc-600" />
          <input
            value={search}
            onChange={(event) => setSearch(event.target.value)}
            placeholder="Filter by project, group, or tag…"
            className="w-full rounded-lg border border-zinc-800 bg-zinc-900 py-2 pl-8 pr-3 text-xs text-zinc-200 placeholder:text-zinc-600 focus:border-cyan-400/40 focus:outline-none"
          />
        </div>
      </div>

      {grouped.length === 0 ? (
        <Card>
          <EmptyState
            title="No matching projects"
            description="Try a different search, or add projects to projects.yaml and reload the config."
          />
        </Card>
      ) : (
        grouped.map(([group, projects]) => (
          <section key={group} className="space-y-2">
            <div className="flex items-center gap-2 px-1 text-[11px] font-medium uppercase tracking-wider text-zinc-600">
              <Boxes className="size-3.5" /> {group}
              <span className="text-zinc-700">({projects.length})</span>
            </div>
            <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
              {projects.map((project) => (
                <ProjectCard key={project.id} project={project} />
              ))}
            </div>
          </section>
        ))
      )}
    </div>
  );
}

function ProjectCard({ project }: { project: Project }) {
  const active = project.targets.filter((target) => target.active_job);
  const lastJobs = project.targets
    .map((target) => target.last_job)
    .filter((job): job is Job => Boolean(job))
    .sort((a, b) => (a.created_at < b.created_at ? 1 : -1));
  const last = lastJobs[0];

  return (
    <Link
      to={`/projects/${project.id}`}
      className="group flex flex-col rounded-2xl border border-zinc-800 bg-zinc-950/60 p-4 transition hover:border-zinc-700 hover:bg-zinc-900/40"
    >
      <div className="flex items-start justify-between gap-2">
        <div className="min-w-0">
          <div className="truncate text-sm font-semibold text-zinc-100 group-hover:text-cyan-100">
            {project.name}
          </div>
          <div className="truncate font-mono text-[10px] text-zinc-600">{project.id}</div>
        </div>
        {last ? <PhaseBadge phase={last.phase} /> : <span className="text-[10px] text-zinc-600">never run</span>}
      </div>
      <div className="mt-3 flex flex-wrap gap-1">
        {project.tags.map((tag) => (
          <span
            key={tag}
            className="rounded border border-zinc-800 bg-zinc-900 px-1.5 py-0.5 text-[10px] text-zinc-500"
          >
            {tag}
          </span>
        ))}
      </div>
      <div className="mt-auto grid grid-cols-3 gap-2 pt-4 text-[11px]">
        <div>
          <div className="text-zinc-600">Targets</div>
          <div className="mt-0.5 font-medium text-zinc-300">{project.targets.length}</div>
        </div>
        <div>
          <div className="text-zinc-600">Success</div>
          <div className="mt-0.5 font-medium text-zinc-300">
            {formatPercent(project.stats.success_rate)}
          </div>
        </div>
        <div>
          <div className="text-zinc-600">Last</div>
          <div className="mt-0.5 truncate font-medium text-zinc-300">
            {last ? formatRelative(last.finished_at ?? last.started_at) : "—"}
          </div>
        </div>
      </div>
      {active.length > 0 && (
        <div className="mt-3 flex items-center gap-2 rounded-lg border border-cyan-400/20 bg-cyan-400/5 px-2.5 py-1.5 text-[11px] text-cyan-200">
          <span className="size-1.5 animate-pulse rounded-full bg-cyan-300" />
          {active.length} active: {active.map((target) => target.label).join(", ")}
        </div>
      )}
    </Link>
  );
}

export function ProjectDetailPage() {
  const { projectId = "" } = useParams();
  const queryClient = useQueryClient();
  const toast = useToast();
  const project = useQuery({
    queryKey: ["project", projectId],
    queryFn: () => api.project(projectId),
    enabled: Boolean(projectId),
  });
  const [pending, setPending] = useState<Target | null>(null);

  const run = useMutation({
    mutationFn: (target: Target) => api.trigger(projectId, target.id),
    onSuccess: async (result) => {
      toast.push(`Queued ${result.project_id}/${result.build_id}`, "success");
      setPending(null);
      await queryClient.invalidateQueries({ queryKey: ["projects"] });
      await queryClient.invalidateQueries({ queryKey: ["project", projectId] });
      await queryClient.invalidateQueries({ queryKey: ["jobs"] });
      await queryClient.invalidateQueries({ queryKey: ["status"] });
    },
    onError: (error) => {
      toast.push(error instanceof Error ? error.message : "Failed to queue deploy", "error");
    },
  });

  if (project.isLoading) return <Spinner />;
  if (project.isError) return <ErrorState error={project.error} />;
  const data = project.data!;
  const targets = sortTargetsWithFavorites(data.id, data.targets);

  return (
    <div className="space-y-5">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <Link
            to="/projects"
            className="mb-2 inline-flex items-center gap-1 text-xs text-zinc-500 transition hover:text-zinc-300"
          >
            <ArrowLeft className="size-3.5" /> Projects
          </Link>
          <h1 className="text-xl font-semibold tracking-tight text-zinc-100">{data.name}</h1>
          <p className="mt-0.5 truncate font-mono text-[11px] text-zinc-500">{data.path}</p>
        </div>
        <div className="flex flex-wrap items-center gap-2 text-xs">
          <span className="rounded-full border border-zinc-800 bg-zinc-900 px-2.5 py-1 text-zinc-400">
            {data.package_manager}
          </span>
          {data.group && (
            <span className="rounded-full border border-zinc-800 bg-zinc-900 px-2.5 py-1 text-zinc-400">
              {data.group}
            </span>
          )}
          {data.tags.map((tag) => (
            <span
              key={tag}
              className="rounded-full border border-zinc-800 bg-zinc-900 px-2.5 py-1 text-zinc-500"
            >
              {tag}
            </span>
          ))}
        </div>
      </div>

      {data.description && <p className="max-w-3xl text-sm text-zinc-400">{data.description}</p>}

      <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
        <StatCard label="Targets" value={data.targets.length} icon={<FolderKanban className="size-4 text-cyan-400" />} />
        <StatCard
          label="Success rate"
          value={formatPercent(data.stats.success_rate)}
          hint={`${data.stats.total} runs`}
          tone={data.stats.success_rate >= 80 && data.stats.total > 0 ? "good" : "default"}
        />
        <StatCard label="Avg duration" value={formatDuration(data.stats.avg_duration_ms)} />
        <StatCard
          label="Active"
          value={data.targets.filter((target) => target.active_job).length}
          tone={data.targets.some((target) => target.active_job) ? "warn" : "default"}
        />
      </div>

      <Card>
        <CardHeader
          title="Deploy targets"
          subtitle="Each target runs in the project working tree. Same-project deploys are serialized."
        />
        <div className="divide-y divide-zinc-800/60">
          {targets.map((target) => (
            <div key={target.id} className="px-4 py-4">
              <div className="flex flex-wrap items-start justify-between gap-3">
                <div className="min-w-0">
                  <div className="flex flex-wrap items-center gap-2">
                    <FavoriteStar
                      projectId={data.id}
                      projectName={data.name}
                      targetId={target.id}
                      targetLabel={target.label}
                      runnerSummary={target.runner_summary}
                    />
                    <span className="text-sm font-medium text-zinc-200">{target.label}</span>
                    <span className="rounded border border-zinc-800 bg-zinc-900 px-1.5 py-0.5 font-mono text-[10px] text-zinc-500">
                      {target.id}
                    </span>
                    {target.danger && (
                      <span className="inline-flex items-center gap-1 rounded-full border border-rose-400/30 bg-rose-400/10 px-2 py-0.5 text-[10px] font-medium text-rose-300">
                        <ShieldAlert className="size-3" /> dangerous
                      </span>
                    )}
                    {target.active_job && (
                      <PhaseBadge
                        phase={target.active_job.phase}
                        cancelRequested={target.active_job.cancel_requested}
                      />
                    )}
                  </div>
                  <div className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-zinc-500">
                    <span className="inline-flex items-center gap-1 font-mono">
                      <TerminalSquare className="size-3" /> {target.runner_summary}
                    </span>
                    {target.timeout_secs && <span>timeout {target.timeout_secs}s</span>}
                    {target.env_keys.length > 0 && <span>{target.env_keys.length} env vars</span>}
                    {target.speak !== null && (
                      <span>speak {target.speak ? "on" : "off"}</span>
                    )}
                    {target.tags.map((tag) => (
                      <span key={tag} className="text-zinc-600">
                        #{tag}
                      </span>
                    ))}
                  </div>
                  {target.description && (
                    <div className="mt-1 text-[11px] text-zinc-500">{target.description}</div>
                  )}
                  <div className="mt-2 flex flex-wrap items-center gap-3 text-[11px] text-zinc-600">
                    <span>
                      {target.stats.total} runs · {formatPercent(target.stats.success_rate)} success ·{" "}
                      {formatDuration(target.stats.avg_duration_ms)} avg
                    </span>
                    {target.last_job && (
                      <Link
                        to={`/deployments/${target.last_job.id}`}
                        className="inline-flex items-center gap-1 text-zinc-500 hover:text-cyan-300"
                      >
                        last {phaseLabel(target.last_job.phase)} ·{" "}
                        {formatRelative(target.last_job.finished_at ?? target.last_job.started_at)}
                      </Link>
                    )}
                  </div>
                  {target.last_job && (
                    <div className="mt-1 flex items-center gap-2 text-[10px] text-zinc-700">
                      <span className={cn("rounded-full border px-1.5 py-0.5", phaseClass(target.last_job.phase))}>
                        {phaseLabel(target.last_job.phase)}
                      </span>
                      <LiveDuration job={target.last_job} />
                    </div>
                  )}
                </div>
                <div className="flex shrink-0 items-center gap-2">
                  {target.active_job && (
                    <Link
                      to={`/deployments/${target.active_job.id}`}
                      className="rounded-lg border border-cyan-400/30 bg-cyan-400/10 px-2.5 py-1.5 text-xs font-medium text-cyan-200 hover:bg-cyan-400/20"
                    >
                      View live
                    </Link>
                  )}
                  <Button
                    variant={target.danger ? "danger" : "primary"}
                    onClick={() => setPending(target)}
                    disabled={Boolean(target.active_job)}
                  >
                    <Play className="size-3.5" /> Run
                  </Button>
                </div>
              </div>
            </div>
          ))}
          {targets.length === 0 && (
            <EmptyState
              title="No targets"
              description="Add targets to this project in projects.yaml, then reload the config."
            />
          )}
        </div>
      </Card>

      <Modal
        open={Boolean(pending)}
        title={pending?.danger ? "Run dangerous deploy?" : "Run deploy?"}
        onClose={() => setPending(null)}
      >
        {pending && (
          <div className="space-y-4">
            <div className="rounded-lg border border-zinc-800 bg-zinc-900/60 p-3 text-sm">
              <div className="font-medium text-zinc-200">{pending.label}</div>
              <div className="mt-0.5 font-mono text-[11px] text-zinc-500">
                {pending.runner_summary}
              </div>
              <div className="mt-0.5 truncate font-mono text-[10px] text-zinc-600">
                {pending.cwd}
              </div>
            </div>
            {pending.danger && (
              <div className="flex items-start gap-2 rounded-lg border border-rose-400/30 bg-rose-950/30 px-3 py-2 text-xs text-rose-200">
                <ShieldAlert className="mt-0.5 size-4 shrink-0" />
                This target is marked dangerous. It may deploy everything, release to a store,
                or otherwise make broad changes.
              </div>
            )}
            <div className="flex justify-end gap-2">
              <Button variant="ghost" onClick={() => setPending(null)}>
                Cancel
              </Button>
              <Button
                variant={pending.danger ? "danger" : "primary"}
                loading={run.isPending}
                onClick={() => run.mutate(pending)}
              >
                <Play className="size-4" /> Queue deploy
              </Button>
            </div>
          </div>
        )}
      </Modal>
    </div>
  );
}
