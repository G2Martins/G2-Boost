import { useEffect, useState } from "react";
import { backend } from "@/lib/backend";
import { cn } from "@/lib/utils";

/** One request per path for the whole session: lists that poll (processes) never re-ask. */
const cache = new Map<string, Promise<string | null>>();

function iconFor(path: string): Promise<string | null> {
  const key = path.toLowerCase();
  let hit = cache.get(key);
  if (!hit) {
    hit = backend.exeIcon(path).then((s) => s ?? null);
    cache.set(key, hit);
  }
  return hit;
}

/** A program's own icon from its .exe; its initial on a tinted tile while loading or when none. */
export function ExeIcon({
  path,
  name,
  className = "size-5",
}: {
  path: string | null | undefined;
  name: string;
  className?: string;
}) {
  const [src, setSrc] = useState<string | null>(null);
  useEffect(() => {
    let alive = true;
    setSrc(null);
    if (path) iconFor(path).then((s) => alive && setSrc(s));
    return () => {
      alive = false;
    };
  }, [path]);
  return src ? (
    <img
      src={src}
      alt=""
      aria-hidden
      className={cn(className, "shrink-0 object-contain")}
      draggable={false}
    />
  ) : (
    <span
      aria-hidden
      className={cn(
        className,
        "flex shrink-0 items-center justify-center rounded bg-foreground/6 font-display text-[10px] font-semibold text-muted-foreground",
      )}
    >
      {name
        .replace(/^pid \d+$/, "?")
        .slice(0, 1)
        .toUpperCase()}
    </span>
  );
}
