import { CornerDownLeft, Send } from "lucide-react";
import { useState } from "react";

import { cn } from "../lib/utils";
import { Button } from "./UI";

export function StdinBar({
  onSubmit,
  disabled,
}: {
  onSubmit: (text: string) => Promise<void> | void;
  disabled?: boolean;
}) {
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    if (busy) return;
    setBusy(true);
    try {
      await onSubmit(value);
      setValue("");
    } finally {
      setBusy(false);
    }
  };

  return (
    <div
      className={cn(
        "flex items-center gap-2 border-t border-zinc-800 bg-zinc-950/80 px-3 py-2",
        disabled && "opacity-60",
      )}
    >
      <Send className="size-4 shrink-0 text-fuchsia-300" />
      <input
        value={value}
        onChange={(event) => setValue(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            void submit();
          }
        }}
        disabled={disabled || busy}
        placeholder="Send a line to the running deploy (e.g. y)"
        className="min-w-0 flex-1 bg-transparent font-mono text-xs text-zinc-100 placeholder:text-zinc-600 focus:outline-none"
      />
      <span className="hidden items-center gap-1 text-[10px] text-zinc-600 sm:flex">
        <CornerDownLeft className="size-3" /> send
      </span>
      <Button size="sm" variant="primary" onClick={() => void submit()} disabled={disabled || busy}>
        Send
      </Button>
    </div>
  );
}
