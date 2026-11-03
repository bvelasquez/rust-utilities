import { KeyRound, Loader2, Radio, Server, TriangleAlert } from "lucide-react";
import { useState, type ReactNode } from "react";

import { api, getApiBase, setApiBase } from "../api/client";
import { useLogin, useSession } from "../hooks/useAuth";
import { Button, Card, ErrorState } from "./UI";

export function AuthGate({ children }: { children: ReactNode }) {
  const session = useSession();
  const login = useLogin();
  const [token, setToken] = useState("");
  const [apiBase, setApiBaseValue] = useState(getApiBase());
  const [showConnection, setShowConnection] = useState(false);

  if (session.isLoading) {
    return (
      <div className="flex min-h-screen items-center justify-center gap-2 text-sm text-zinc-500">
        <Loader2 className="size-4 animate-spin" /> Connecting to soki-ci…
      </div>
    );
  }

  if (session.isError) {
    return (
      <div className="mx-auto flex min-h-screen w-full max-w-xl flex-col justify-center gap-5 px-6">
        <div className="flex items-center gap-3">
          <div className="grid size-10 place-items-center rounded-xl border border-cyan-400/30 bg-cyan-400/10">
            <Radio className="size-5 text-cyan-300" />
          </div>
          <div>
            <div className="text-lg font-semibold text-zinc-100">soki-ci web</div>
            <div className="text-xs text-zinc-500">Could not reach the service</div>
          </div>
        </div>
        <ErrorState error={session.error} />
        <Card className="p-4">
          <label className="text-xs font-medium uppercase tracking-wider text-zinc-500">
            API base URL
          </label>
          <input
            value={apiBase}
            onChange={(event) => setApiBaseValue(event.target.value)}
            placeholder="http://127.0.0.1:9847"
            className="mt-2 w-full rounded-lg border border-zinc-800 bg-zinc-900 px-3 py-2 font-mono text-sm text-zinc-200 focus:border-cyan-400/40 focus:outline-none"
          />
          <Button
            className="mt-3"
            variant="primary"
            onClick={() => {
              setApiBase(apiBase);
              void session.refetch();
            }}
          >
            <Server className="size-4" /> Connect
          </Button>
        </Card>
        <p className="text-xs text-zinc-600">
          Start the service with <code className="text-zinc-400">soki-ci serve</code> or{" "}
          <code className="text-zinc-400">soki-ci</code> (TUI + API).
        </p>
      </div>
    );
  }

  if (session.data?.auth_required && !session.data.authenticated) {
    return (
      <div className="mx-auto flex min-h-screen w-full max-w-md flex-col justify-center gap-5 px-6">
        <div className="flex items-center gap-3">
          <div className="grid size-10 place-items-center rounded-xl border border-cyan-400/30 bg-cyan-400/10">
            <KeyRound className="size-5 text-cyan-300" />
          </div>
          <div>
            <div className="text-lg font-semibold text-zinc-100">soki-ci web</div>
            <div className="text-xs text-zinc-500">
              This service requires an API token.
            </div>
          </div>
        </div>
        <Card className="p-5">
          <form
            onSubmit={(event) => {
              event.preventDefault();
              login.mutate(token.trim());
            }}
            className="space-y-3"
          >
            <label className="block text-xs font-medium uppercase tracking-wider text-zinc-500">
              API token
            </label>
            <input
              type="password"
              autoFocus
              value={token}
              onChange={(event) => setToken(event.target.value)}
              placeholder="SOKI_CI_API_TOKEN"
              className="w-full rounded-lg border border-zinc-800 bg-zinc-900 px-3 py-2 font-mono text-sm text-zinc-200 focus:border-cyan-400/40 focus:outline-none"
            />
            {login.isError && (
              <div className="flex items-start gap-2 text-xs text-rose-300">
                <TriangleAlert className="mt-0.5 size-3.5 shrink-0" />
                {login.error instanceof Error ? login.error.message : "Login failed"}
              </div>
            )}
            <Button
              type="submit"
              variant="primary"
              className="w-full"
              loading={login.isPending}
              disabled={!token.trim()}
            >
              Sign in
            </Button>
          </form>
        </Card>
        <button
          type="button"
          onClick={() => setShowConnection((current) => !current)}
          className="text-left text-xs text-zinc-600 underline-offset-2 hover:text-zinc-400 hover:underline"
        >
          Advanced connection settings
        </button>
        {showConnection && (
          <Card className="p-4">
            <label className="text-xs font-medium uppercase tracking-wider text-zinc-500">
              API base URL
            </label>
            <input
              value={apiBase}
              onChange={(event) => setApiBaseValue(event.target.value)}
              placeholder="http://127.0.0.1:9847"
              className="mt-2 w-full rounded-lg border border-zinc-800 bg-zinc-900 px-3 py-2 font-mono text-sm text-zinc-200 focus:border-cyan-400/40 focus:outline-none"
            />
            <Button
              className="mt-3"
              onClick={async () => {
                setApiBase(apiBase);
                await api.session().catch(() => undefined);
                void session.refetch();
              }}
            >
              Apply and reconnect
            </Button>
          </Card>
        )}
      </div>
    );
  }

  return <>{children}</>;
}
