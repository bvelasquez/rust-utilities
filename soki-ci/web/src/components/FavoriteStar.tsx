import { Star } from "lucide-react";

import type { FavoriteEntry } from "../lib/favorites";
import { useFavoriteActions } from "../lib/favorites";
import { cn } from "../lib/utils";

type Props = {
  projectId: string;
  projectName: string;
  targetId: string;
  targetLabel: string;
  runnerSummary: string;
  className?: string;
};

export function FavoriteStar({
  projectId,
  projectName,
  targetId,
  targetLabel,
  runnerSummary,
  className,
}: Props) {
  const { isFavorite, toggleFavorite } = useFavoriteActions();
  const active = isFavorite({ projectId, targetId });

  const entry: FavoriteEntry = {
    projectId,
    projectName,
    targetId,
    targetLabel,
    runnerSummary,
  };

  return (
    <button
      type="button"
      title={active ? "Remove from favorites" : "Add to favorites"}
      aria-label={active ? "Remove from favorites" : "Add to favorites"}
      aria-pressed={active}
      onClick={(event) => {
        event.preventDefault();
        event.stopPropagation();
        toggleFavorite(entry);
      }}
      className={cn(
        "rounded-md p-1.5 transition hover:bg-zinc-800/80",
        active ? "text-amber-300" : "text-zinc-600 hover:text-zinc-400",
        className,
      )}
    >
      <Star className={cn("size-4", active && "fill-amber-300/40")} />
    </button>
  );
}
