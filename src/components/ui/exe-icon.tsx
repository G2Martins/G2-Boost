import { type ReactNode, useEffect, useState } from "react";
import { backend } from "@/lib/backend";
import { cn } from "@/lib/utils";

/** One request per key for the whole session: lists that poll (processes, RAM) never re-ask. */
const cache = new Map<string, Promise<string | null>>();

function iconFor(
  key: string,
  load: () => Promise<string | null | undefined>,
): Promise<string | null> {
  let hit = cache.get(key);
  if (!hit) {
    hit = load().then((s) => s ?? null);
    cache.set(key, hit);
  }
  return hit;
}

/** A cached icon data URL by key (`null` key = none); `null` while loading or when there is none. */
export function useIcon(
  key: string | null,
  load: () => Promise<string | null | undefined>,
): string | null {
  const [src, setSrc] = useState<string | null>(null);
  // biome-ignore lint/correctness/useExhaustiveDependencies: the key names what `load` fetches
  useEffect(() => {
    let alive = true;
    setSrc(null);
    if (key) iconFor(key, load).then((s) => alive && setSrc(s));
    return () => {
      alive = false;
    };
  }, [key]);
  return src;
}

/** An icon, or `fallback`, or the name's initial on a tinted tile (loading or no icon). */
export function IconTile({
  src,
  name,
  className = "size-5",
  fallback,
}: {
  src: string | null;
  name: string;
  className?: string;
  fallback?: ReactNode;
}) {
  if (src) {
    return (
      <img
        src={src}
        alt=""
        aria-hidden
        className={cn(className, "shrink-0 object-contain")}
        draggable={false}
      />
    );
  }
  return (
    <span
      aria-hidden
      data-icon-fallback
      className={cn(
        "flex shrink-0 items-center justify-center rounded bg-foreground/6 font-display text-[10px] font-semibold text-muted-foreground",
        className,
      )}
    >
      {fallback ??
        name
          .replace(/^pid \d+$/, "?")
          .slice(0, 1)
          .toUpperCase()}
    </span>
  );
}

/** A program's own icon from its .exe (checked again in the core: local, plain file). */
export function ExeIcon({
  path,
  name,
  className = "size-5",
  fallback,
}: {
  path: string | null | undefined;
  name: string;
  className?: string;
  fallback?: ReactNode;
}) {
  const src = useIcon(path ? `exe:${path.toLowerCase()}` : null, () => backend.exeIcon(path ?? ""));
  return <IconTile src={src} name={name} className={className} fallback={fallback} />;
}
