import { hierarchy, treemap, treemapSquarify } from "d3-hierarchy";
import { useEffect, useMemo, useRef, useState } from "react";
import type { NodeView } from "@/lib/backend";
import { formatBytes } from "@/lib/utils";

/** File class → CSS token (defined in index.css). Folders use depth shades. */
const CLASS_EXT: Record<string, string> = {
  video: "mp4 mkv avi mov wmv webm m4v ts",
  audio: "mp3 flac wav aac ogg m4a wma",
  image: "jpg jpeg png gif webp bmp tif tiff heic psd raw",
  archive: "zip rar 7z tar gz xz bz2 iso cab",
  code: "js ts tsx jsx rs py go java c cpp h cs json toml yaml yml css html md",
  binary: "exe dll sys msi bin dat pak vhdx vdi vmdk img",
  document: "pdf doc docx xls xlsx ppt pptx txt csv",
};
const EXT_CLASS = new Map<string, string>();
for (const [cls, exts] of Object.entries(CLASS_EXT))
  for (const e of exts.split(" ")) EXT_CLASS.set(e, cls);

function classOf(name: string): string {
  const i = name.lastIndexOf(".");
  if (i < 0) return "other";
  return EXT_CLASS.get(name.slice(i + 1).toLowerCase()) ?? "other";
}

type Rect = { x0: number; y0: number; x1: number; y1: number; node: NodeView; depth: number };

/** Canvas needs concrete colours: read the CSS token (always defined in index.css). */
function token(name: string) {
  const v = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return v ? `hsl(${v})` : "gray";
}

/**
 * Squarified treemap on a canvas. Depth 2 (children + grandchildren) is enough per view;
 * clicking a folder re-roots the view, which keeps the layout cheap at any tree size.
 */
export function Treemap({
  root,
  onOpen,
  locale,
}: {
  root: NodeView;
  onOpen: (node: NodeView) => void;
  locale: string;
}) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const wrapRef = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ w: 800, h: 480 });
  const [hover, setHover] = useState<{ x: number; y: number; r: Rect } | null>(null);

  useEffect(() => {
    const el = wrapRef.current;
    if (!el) return;
    const ro = new ResizeObserver(([e]) => {
      if (e)
        setSize({ w: Math.max(200, e.contentRect.width), h: Math.max(200, e.contentRect.height) });
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const rects = useMemo(() => {
    const h = hierarchy<NodeView>(root, (d) => d.children)
      .sum((d) => (d.children?.length ? 0 : d.size))
      .sort((a, b) => (b.value ?? 0) - (a.value ?? 0));
    // "others" bucket keeps the proportions honest when a folder has more children than shown
    const laid = treemap<NodeView>()
      .tile(treemapSquarify.ratio(1.4))
      .size([size.w, size.h])
      .paddingOuter(3)
      .paddingTop((d) => (d.depth === 1 && d.children ? 16 : 3))
      .paddingInner(2)
      .round(true)(h);
    const out: Rect[] = [];
    laid.each((n) => {
      if (n.depth === 0) return;
      out.push({ x0: n.x0, y0: n.y0, x1: n.x1, y1: n.y1, node: n.data, depth: n.depth });
    });
    return out;
  }, [root, size]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const dpr = window.devicePixelRatio || 1;
    canvas.width = size.w * dpr;
    canvas.height = size.h * dpr;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    ctx.scale(dpr, dpr);
    ctx.clearRect(0, 0, size.w, size.h);
    const colors = {
      dir1: token("--card-hover"),
      dir2: token("--surface"),
      border: token("--border"),
      text: token("--foreground"),
      muted: token("--muted-foreground"),
      cls: Object.fromEntries(
        ["video", "audio", "image", "archive", "code", "binary", "document", "other"].map((c) => [
          c,
          token(`--class-${c}`),
        ]),
      ) as Record<string, string>,
    };
    ctx.font = "500 11px 'Chakra Petch', system-ui";
    for (const r of rects) {
      const w = r.x1 - r.x0;
      const hgt = r.y1 - r.y0;
      if (w < 2 || hgt < 2) continue;
      const isDir = r.node.is_dir;
      ctx.fillStyle = isDir
        ? r.depth === 1
          ? colors.dir1
          : colors.dir2
        : (colors.cls[classOf(r.node.name)] ?? "#444");
      if (!isDir && r.depth === 2) ctx.globalAlpha = 0.85;
      ctx.fillRect(r.x0, r.y0, w, hgt);
      ctx.globalAlpha = 1;
      ctx.strokeStyle = colors.border;
      ctx.lineWidth = 1;
      ctx.strokeRect(r.x0 + 0.5, r.y0 + 0.5, w - 1, hgt - 1);
      if (w > 48 && hgt > 14) {
        const label = r.node.name;
        ctx.fillStyle = isDir ? colors.text : "rgba(255,255,255,0.92)";
        ctx.save();
        ctx.beginPath();
        ctx.rect(r.x0 + 3, r.y0, w - 6, hgt);
        ctx.clip();
        ctx.fillText(label, r.x0 + 5, r.y0 + 11);
        if (hgt > 30 && w > 70) {
          ctx.fillStyle = colors.muted;
          ctx.fillText(formatBytes(r.node.size, locale, 1), r.x0 + 5, r.y0 + 24);
        }
        ctx.restore();
      }
    }
  }, [rects, size, locale]);

  const hit = (x: number, y: number): Rect | null => {
    // deepest rect containing the point wins
    let best: Rect | null = null;
    for (const r of rects)
      if (x >= r.x0 && x <= r.x1 && y >= r.y0 && y <= r.y1 && (!best || r.depth > best.depth))
        best = r;
    return best;
  };

  return (
    <div
      ref={wrapRef}
      className="relative h-130 w-full overflow-hidden rounded-md border border-border bg-background"
    >
      <canvas
        ref={canvasRef}
        style={{ width: size.w, height: size.h }}
        className="block cursor-pointer"
        onMouseMove={(e) => {
          const b = e.currentTarget.getBoundingClientRect();
          const r = hit(e.clientX - b.left, e.clientY - b.top);
          setHover(r ? { x: e.clientX - b.left, y: e.clientY - b.top, r } : null);
        }}
        onMouseLeave={() => setHover(null)}
        onDoubleClick={(e) => {
          const b = e.currentTarget.getBoundingClientRect();
          const r = hit(e.clientX - b.left, e.clientY - b.top);
          if (!r) return;
          // open the depth-1 folder that contains the point
          const top = rects.find(
            (t) => t.depth === 1 && r.x0 >= t.x0 && r.x1 <= t.x1 && r.y0 >= t.y0 && r.y1 <= t.y1,
          );
          const target = top?.node ?? r.node;
          if (target.is_dir) onOpen(target);
        }}
      />
      {hover ? (
        <div
          className="pointer-events-none absolute z-10 max-w-sm rounded-md border border-border bg-surface px-3 py-2 text-xs shadow-xl"
          style={{
            left: Math.min(hover.x + 14, size.w - 260),
            top: Math.min(hover.y + 14, size.h - 60),
          }}
        >
          <div className="truncate font-semibold">{hover.r.node.name}</div>
          <div className="num text-muted-foreground">
            {formatBytes(hover.r.node.size, locale)} ·{" "}
            {((100 * hover.r.node.size) / Math.max(1, root.size)).toFixed(1)}%
            {hover.r.node.is_dir ? ` · ${hover.r.node.files.toLocaleString(locale)} files` : ""}
          </div>
        </div>
      ) : null}
    </div>
  );
}

export const FILE_CLASSES = [
  "video",
  "audio",
  "image",
  "archive",
  "code",
  "binary",
  "document",
  "other",
] as const;
