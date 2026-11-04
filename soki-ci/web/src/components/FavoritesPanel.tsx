import { useMutation, useQueryClient } from "@tanstack/react-query";
import { ChevronDown, ChevronUp, Play, Star } from "lucide-react";
import { Link } from "react-router-dom";

import { api } from "../api/client";
import { favoriteKey, useFavoriteActions } from "../lib/favorites";
import { useToast } from "./Toast";
import { Button, EmptyState } from "./UI";

export function FavoritesPanel({ compact = false }: { compact?: boolean }) {
  const { favorites, moveFavorite } = useFavoriteActions();
  const queryClient = useQueryClient();
  const toast = useToast();

  const run = useMutation({
    mutationFn: (args: { projectId: string; targetId: string; label: string }) =>
      api.trigger(args.projectId, args.targetId),
    onSuccess: async (result, args) => {
      toast.push(`Queued ${args.label}`, "success");
      await queryClient.invalidateQueries({ queryKey: ["projects"] });
      await queryClient.invalidateQueries({ queryKey: ["jobs"] });
      await queryClient.invalidateQueries({ queryKey: ["status"] });
      void result;
    },
    onError: (error) =>
      toast.push(error instanceof Error ? error.message : "Failed to queue", "error"),
  });

  if (favorites.length === 0) {
    return (
      <EmptyState
        title="No favorites yet"
        description="Star a deploy target on any project page. Favorites appear here and sort to the top on project pages."
      />
    );
  }

  return (
    <div className="divide-y divide-zinc-800/60">
      {favorites.map((fav, index) => {
        const key = favoriteKey(fav);
        return (
          <div
            key={key}
            className="flex flex-wrap items-center gap-2 px-4 py-3 transition hover:bg-zinc-900/40"
          >
            <Star className="size-3.5 shrink-0 fill-amber-300/30 text-amber-300" />
            <div className="min-w-0 flex-1">
              <div className="truncate text-sm font-medium text-zinc-200">{fav.targetLabel}</div>
              <div className="truncate text-[11px] text-zinc-500">
                <Link to={`/projects/${fav.projectId}`} className="hover:text-cyan-300">
                  {fav.projectName}
                </Link>
                {!compact && (
                  <span className="font-mono text-zinc-600"> · {fav.runnerSummary}</span>
                )}
              </div>
            </div>
            <div className="flex shrink-0 items-center gap-1">
              <button
                type="button"
                disabled={index === 0}
                onClick={() => moveFavorite(key, -1)}
                className="rounded p-1 text-zinc-600 hover:bg-zinc-800 hover:text-zinc-300 disabled:opacity-30"
                title="Move up"
              >
                <ChevronUp className="size-4" />
              </button>
              <button
                type="button"
                disabled={index === favorites.length - 1}
                onClick={() => moveFavorite(key, 1)}
                className="rounded p-1 text-zinc-600 hover:bg-zinc-800 hover:text-zinc-300 disabled:opacity-30"
                title="Move down"
              >
                <ChevronDown className="size-4" />
              </button>
              <Button
                size="sm"
                variant="primary"
                loading={run.isPending}
                onClick={() =>
                  run.mutate({
                    projectId: fav.projectId,
                    targetId: fav.targetId,
                    label: `${fav.projectName} / ${fav.targetLabel}`,
                  })
                }
              >
                <Play className="size-3.5" /> Run
              </Button>
            </div>
          </div>
        );
      })}
    </div>
  );
}
