export type Phase =
  | "queued"
  | "running"
  | "success"
  | "error"
  | "cancelled"
  | "interrupted";

export type LogStream = "stdout" | "stderr" | "system" | "stdin";

export interface Job {
  id: string;
  project_id: string;
  project_name: string;
  target_id: string;
  target_label: string;
  phase: Phase;
  created_at: string;
  started_at: string | null;
  finished_at: string | null;
  exit_code: number | null;
  duration_ms: number | null;
  source: string;
  runner: string;
  cwd: string;
  log_path: string;
  interrupted: boolean;
  cancel_requested: boolean;
  batch_id: string | null;
  parent_job_id: string | null;
  log_size_bytes: number;
  can_cancel: boolean;
  can_input: boolean;
}

export interface JobStats {
  total: number;
  success: number;
  error: number;
  cancelled: number;
  interrupted: number;
  running: number;
  queued: number;
  success_rate: number;
  avg_duration_ms: number | null;
  last_started_at: string | null;
  last_finished_at: string | null;
  last_phase: Phase | null;
}

export interface Runner {
  kind: "pnpm_script" | "npm_script" | "make" | "shell";
  script?: string;
  target?: string;
  command?: string[];
}

export interface Target {
  id: string;
  label: string;
  runner: Runner;
  runner_summary: string;
  cwd: string;
  env_keys: string[];
  timeout_secs: number | null;
  speak: boolean | null;
  description: string | null;
  tags: string[];
  danger: boolean;
  stats: JobStats;
  active_job: Job | null;
  last_job: Job | null;
}

export interface Project {
  id: string;
  name: string;
  path: string;
  package_manager: "pnpm" | "npm";
  group: string | null;
  tags: string[];
  description: string | null;
  stats: JobStats;
  targets: Target[];
}

export interface OrchestratorStatus {
  max_parallel: number;
  available_parallelism: number;
  running: number;
  queued: number;
  active: number;
  project_locks: { project_id: string; job_id: string; target_id: string }[];
}

export interface Status {
  service: string;
  version: string;
  started_at: string;
  uptime_secs: number;
  bind: string | null;
  config_path: string;
  config_error: string | null;
  max_parallel: number;
  history_limit: number;
  allow_config_write?: boolean;
  orchestrator: OrchestratorStatus;
  tts: {
    enabled: boolean;
    base_url: string;
    voice: string | null;
    on: string[];
    reachable: boolean;
  };
}

export interface LogLine {
  seq: number;
  stream: LogStream;
  line: string;
  ts: string;
}

export interface JobsResponse {
  jobs: Job[];
  total: number;
  total_all: number;
  count: number;
  limit: number;
  offset: number;
  has_more: boolean;
  stats: JobStats;
}

export interface ConfigIssue {
  level: "error" | "warning";
  message: string;
}

export interface ConfigResponse {
  path: string;
  version: number;
  defaults: {
    projects_dir: string;
    max_parallel: number;
    history_limit: number;
    speak: {
      enabled: boolean;
      voice: string | null;
      base_url: string;
      on: string[];
    };
    confirm_deploys: boolean;
  };
  projects: Project[];
  issues: ConfigIssue[];
  raw: string | null;
}

export interface SessionResponse {
  auth_required: boolean;
  authenticated: boolean;
  csrf_token?: string;
  method?: string;
}

export interface TtsStatus {
  enabled: boolean;
  base_url: string;
  voice: string | null;
  on: string[];
  reachable: boolean;
}
