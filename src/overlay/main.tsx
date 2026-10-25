import { listen } from "@tauri-apps/api/event";
import { StrictMode, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import type { OverlayFrame } from "@/lib/backend";
import { I18nProvider } from "@/lib/i18n";
import { Hud } from "./hud";
import "../index.css";
import "./overlay.css";

// The in-game overlay window (label `overlay`, made by src-tauri/src/overlay_cmds.rs). Only draws
// what the Rust worker pushes: no app shell, no commands, nothing to click (it is click-through).

const ANCHOR = {
  top_left: "items-start justify-start",
  top_right: "items-start justify-end",
  bottom_left: "items-end justify-start",
  bottom_right: "items-end justify-end",
} as const;

function Overlay() {
  const [frame, setFrame] = useState<OverlayFrame | null>(null);
  useEffect(() => {
    const offs = [
      listen<OverlayFrame>("overlay:frame", (e) => setFrame(e.payload)),
      // Hidden: drop the last numbers so the next show never starts with stale ones.
      listen<boolean>("overlay:visible", (e) => {
        if (!e.payload) setFrame(null);
      }),
    ];
    return () => {
      for (const off of offs) void off.then((f) => f());
    };
  }, []);
  if (!frame) return null;
  return (
    <div className={`flex h-full w-full ${ANCHOR[frame.config.corner] ?? ANCHOR.top_left}`}>
      <Hud frame={frame} />
    </div>
  );
}

createRoot(document.getElementById("root") as HTMLElement).render(
  <StrictMode>
    <I18nProvider>
      <Overlay />
    </I18nProvider>
  </StrictMode>,
);
