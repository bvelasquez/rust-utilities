import { useMutation, useQuery } from "@tanstack/react-query";
import {
  Bell,
  BellOff,
  ExternalLink,
  Radio,
  Server,
  Settings2,
  Volume2,
} from "lucide-react";
import { useEffect, useState } from "react";

import { api, apiUrl, getApiBase, setApiBase } from "../api/client";
import { useSession } from "../hooks/useAuth";
import { useToast } from "../components/Toast";
import { Button, Card, CardHeader, ErrorState, Spinner } from "../components/UI";
import { formatDuration } from "../lib/utils";

export function SettingsPage() {
  const toast = useToast();
  const session = useSession();
  const [apiBase, setApiBaseValue] = useState(getApiBase());
  const [notifications, setNotifications] = useState(
    typeof Notification !== "undefined" && Notification.permission === "granted",
  );
  const status = useQuery({ queryKey: ["status"], queryFn: api.status });
  const tts = useQuery({ queryKey: ["tts"], queryFn: api.tts });

  useEffect(() => {
    if (typeof Notification !== "undefined") {
      setNotifications(Notification.permission === "granted");
    }
  }, []);

  const testTts = useMutation({
    mutationFn: () => api.testTts(),
    onSuccess: () => toast.push("Sent a test announcement to the TTS broker", "success"),
    onError: (error) =>
      toast.push(error instanceof Error ? error.message : "TTS test failed", "error"),
  });

  if (status.isLoading) return <Spinner />;
  if (status.isError) return <ErrorState error={status.error} />;
  const data = status.data!;

  return (
    <div className="space-y-5">
      <div>
        <h1 className="text-xl font-semibold tracking-tight text-zinc-100">Settings</h1>
        <p className="mt-0.5 text-xs text-zinc-500">Connection, notifications, and service details.</p>
      </div>

      <div className="grid gap-4 xl:grid-cols-2">
        <Card>
          <CardHeader title="Connection" subtitle="Where the web app talks to soki-ci" />
          <div className="space-y-3 px-5 py-4">
            <label className="block text-xs font-medium uppercase tracking-wider text-zinc-500">
              API base URL
            </label>
            <input
              value={apiBase}
              onChange={(event) => setApiBaseValue(event.target.value)}
              placeholder="Same origin (empty) or http://127.0.0.1:9847"
              className="w-full rounded-lg border border-zinc-800 bg-zinc-900 px-3 py-2 font-mono text-xs text-zinc-200 placeholder:text-zinc-600 focus:border-cyan-400/40 focus:outline-none"
            />
            <div className="flex items-center gap-2">
              <Button
                size="sm"
                variant="primary"
                onClick={() => {
                  setApiBase(apiBase);
                  window.location.reload();
                }}
              >
                <Server className="size-3.5" /> Save and reload
              </Button>
              <Button
                size="sm"
                variant="ghost"
                onClick={() => {
                  setApiBase("");
                  setApiBaseValue("");
                  window.location.reload();
                }}
              >
                Use same origin
              </Button>
            </div>
            <div className="rounded-lg border border-zinc-800 bg-zinc-900/60 px-3 py-2 text-[11px] text-zinc-500">
              Auth: {session.data?.auth_required ? session.data.method ?? "required" : "local (no token)"}
              {" · "}
              Service {data.bind ?? "unknown"}
            </div>
          </div>
        </Card>

        <Card>
          <CardHeader title="Notifications" subtitle="Browser notifications for terminal deploy states" />
          <div className="space-y-3 px-5 py-4">
            <div className="flex items-center justify-between gap-3 rounded-lg border border-zinc-800 bg-zinc-900/50 px-3 py-2.5">
              <div className="flex items-center gap-2 text-xs">
                {notifications ? (
                  <Bell className="size-4 text-emerald-300" />
                ) : (
                  <BellOff className="size-4 text-zinc-500" />
                )}
                <div>
                  <div className="text-zinc-200">Browser notifications</div>
                  <div className="text-[11px] text-zinc-500">
                    {notifications ? "Enabled for this browser" : "Permission not granted"}
                  </div>
                </div>
              </div>
              {!notifications && (
                <Button
                  size="sm"
                  variant="primary"
                  onClick={async () => {
                    const permission = await Notification.requestPermission();
                    setNotifications(permission === "granted");
                    toast.push(
                      permission === "granted"
                        ? "Notifications enabled"
                        : "Notification permission denied",
                      permission === "granted" ? "success" : "error",
                    );
                  }}
                >
                  Enable
                </Button>
              )}
            </div>
            <p className="text-[11px] text-zinc-500">
              The service still speaks success/failure through the TTS broker independently of browser
              notifications.
            </p>
          </div>
        </Card>

        <Card>
          <CardHeader
            title="TTS broker"
            subtitle="Server-side voice announcements on terminal deploy states"
            action={
              <span
                className={`rounded-full border px-2 py-0.5 text-[10px] ${
                  tts.data?.reachable
                    ? "border-emerald-400/30 bg-emerald-400/10 text-emerald-300"
                    : "border-amber-400/30 bg-amber-400/10 text-amber-300"
                }`}
              >
                {tts.data?.reachable ? "reachable" : "offline"}
              </span>
            }
          />
          <div className="space-y-3 px-5 py-4 text-xs">
            <div className="grid grid-cols-2 gap-3">
              <div>
                <div className="text-zinc-600">Enabled</div>
                <div className="mt-0.5 text-zinc-300">{tts.data?.enabled ? "yes" : "no"}</div>
              </div>
              <div>
                <div className="text-zinc-600">Voice</div>
                <div className="mt-0.5 text-zinc-300">{tts.data?.voice ?? "broker default"}</div>
              </div>
              <div className="col-span-2">
                <div className="text-zinc-600">Base URL</div>
                <div className="mt-0.5 break-all font-mono text-[11px] text-zinc-300">
                  {tts.data?.base_url ?? "—"}
                </div>
              </div>
              <div className="col-span-2">
                <div className="text-zinc-600">Speak on</div>
                <div className="mt-0.5 text-zinc-300">{tts.data?.on.join(", ") ?? "—"}</div>
              </div>
            </div>
            <Button
              size="sm"
              variant="secondary"
              loading={testTts.isPending}
              disabled={!tts.data?.enabled}
              onClick={() => testTts.mutate()}
            >
              <Volume2 className="size-3.5" /> Test announcement
            </Button>
          </div>
        </Card>

        <Card>
          <CardHeader title="Service" subtitle="Version, limits, and links" />
          <div className="space-y-2 px-5 py-4 text-xs">
            <Row label="Version" value={data.version} />
            <Row label="Started" value={new Date(data.started_at).toLocaleString()} />
            <Row label="Uptime" value={formatDuration(data.uptime_secs * 1000)} />
            <Row label="Max parallel" value={String(data.max_parallel)} />
            <Row label="History limit" value={String(data.history_limit)} />
            <Row label="Config path" value={data.config_path} mono />
            <div className="flex flex-wrap gap-2 pt-2">
              <a
                href={apiUrl("/capabilities")}
                target="_blank"
                rel="noreferrer"
                className="inline-flex items-center gap-1.5 rounded-lg border border-zinc-700 bg-zinc-900 px-2.5 py-1.5 text-xs text-zinc-200 hover:border-zinc-500"
              >
                <Radio className="size-3.5" /> Capabilities manifest
                <ExternalLink className="size-3" />
              </a>
              <a
                href={apiUrl("/health")}
                target="_blank"
                rel="noreferrer"
                className="inline-flex items-center gap-1.5 rounded-lg border border-zinc-700 bg-zinc-900 px-2.5 py-1.5 text-xs text-zinc-200 hover:border-zinc-500"
              >
                <Settings2 className="size-3.5" /> Health
                <ExternalLink className="size-3" />
              </a>
            </div>
          </div>
        </Card>
      </div>
    </div>
  );
}

function Row({ label, value, mono }: { label: string; value: string; mono?: boolean }) {
  return (
    <div className="flex items-start justify-between gap-3">
      <span className="shrink-0 text-zinc-600">{label}</span>
      <span className={`min-w-0 text-right text-zinc-300 ${mono ? "break-all font-mono text-[11px]" : ""}`}>
        {value}
      </span>
    </div>
  );
}
