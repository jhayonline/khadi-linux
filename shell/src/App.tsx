// The eDEX composition, with the panels able to get out of the way.
//
// Geometry is still eDEX's — 17% columns, a 30vh bottom row — but the
// positioning is not. eDEX hangs its columns off `position: absolute` and
// leans on flex-wrap for the bottom row, which works for furniture that never
// moves and falls apart the moment a panel can fold. This is three flex boxes:
// a row of [left | shell | right] over a filesystem strip. At the default
// settings it lands where eDEX's does; with a panel folded it just works.

import { useCallback, useEffect, useState } from "react";
import {
  getFilesystem,
  getNetwork,
  getSystem,
  getTheme,
  type Filesystem,
  type Network,
  type System,
  type Theme,
} from "./lib/ipc";
import { Title } from "./components/Title";
import { Clock, CpuInfo, Hardware, RamWatcher, SysInfo, TopList } from "./components/LeftColumn";
import { ConnInfo, Globe, NetStat } from "./components/RightColumn";
import { MainShell } from "./components/MainShell";
import { FilesystemPanel } from "./components/Filesystem";
import { CollapseButton, CollapsedRail } from "./components/Collapse";

type Folded = { left: boolean; right: boolean; bottom: boolean };
const FOLD_KEY = "khadi.folded";

/** Poll on a timer, as eDEX does. The Rust side refreshes on demand, so the
 *  interval is the only clock in the system. */
function usePoll<T>(fn: () => Promise<T>, ms: number): T | null {
  const [v, setV] = useState<T | null>(null);
  useEffect(() => {
    let alive = true;
    let timer: number | undefined;
    const run = async () => {
      try {
        const x = await fn();
        if (alive) setV(x);
      } catch {
        /* a panel that cannot read is better blank than crashed */
      }
      // CHAINED, NOT setInterval. An interval queues another call whether or
      // not the last one came back; on a loaded machine the refreshes stack up
      // behind each other and the UI gets slower exactly when it matters most.
      if (alive) timer = window.setTimeout(run, ms);
    };
    void run();
    return () => {
      alive = false;
      if (timer !== undefined) clearTimeout(timer);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ms]);
  return v;
}

export default function App() {
  const [theme, setTheme] = useState<Theme | null>(null);
  const [fold, setFold] = useState<Folded>(() => {
    try {
      const raw = localStorage.getItem(FOLD_KEY);
      if (raw) return { left: false, right: false, bottom: false, ...JSON.parse(raw) };
    } catch {
      /* a browser with storage switched off still gets a desktop */
    }
    return { left: false, right: false, bottom: false };
  });

  const toggle = useCallback((k: keyof Folded) => {
    setFold((f) => {
      const next = { ...f, [k]: !f[k] };
      try {
        localStorage.setItem(FOLD_KEY, JSON.stringify(next));
      } catch {
        /* not worth failing a click over */
      }
      return next;
    });
  }, []);

  // A folded panel is not polled. Nothing reads the result, and on a laptop
  // the cheapest work is the work that does not happen.
  const sys = usePoll<System>(getSystem, fold.left ? 60000 : 1000);
  const net = usePoll<Network>(getNetwork, fold.right ? 60000 : 1000);
  const fs = usePoll<Filesystem>(getFilesystem, fold.bottom ? 60000 : 3000);

  useEffect(() => {
    getTheme()
      .then((t) => {
        setTheme(t);
        const hex = (s: string) => {
          const n = parseInt(s.slice(1), 16);
          return `${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}`;
        };
        const r = document.documentElement.style;
        r.setProperty("--c", hex(t.text));
        r.setProperty("--ground", t.ground);
        r.setProperty("--grid", t.ramp[1]);
      })
      .catch(() => {});
  }, []);

  // Ctrl+Alt+<arrow> folds the panel in that direction. Ctrl+Alt is free:
  // Hyprland owns Super, the shell inside owns Ctrl, and foot's Ctrl+Shift is
  // not in this window at all.
  useEffect(() => {
    const on = (e: KeyboardEvent) => {
      if (!e.ctrlKey || !e.altKey) return;
      const k =
        e.code === "ArrowLeft" ? "left" : e.code === "ArrowRight" ? "right" : e.code === "ArrowDown" ? "bottom" : null;
      if (!k) return;
      e.preventDefault();
      toggle(k);
    };
    // CAPTURE PHASE. xterm.js owns the keyboard once the terminal has focus
    // and calls preventDefault on almost everything, so a bubble-phase
    // listener on window never sees Ctrl+Alt+Left — it was added, it was
    // correct, and it did nothing at all until this flag.
    window.addEventListener("keydown", on, { capture: true });
    return () => window.removeEventListener("keydown", on, { capture: true });
  }, [toggle]);

  return (
    <div className="flex h-full w-full flex-col pt-[calc(1.85vh*var(--ui-scale))]">
      <div className="flex min-h-0 flex-1 flex-row">
        {/* ------------------------------------------------ mod_column_left */}
        {fold.left ? (
          <CollapsedRail label="SYSTEM" side="left" onClick={() => toggle("left")} />
        ) : (
          <section className="flex w-[20%] shrink-0 flex-col overflow-hidden box-border p-[calc(1.39vh*var(--ui-scale))] pt-0">
            <Title
              left="PANEL"
              right="SYSTEM"
              action={<CollapseButton side="left" collapsed={false} onClick={() => toggle("left")} />}
            />
            <Clock />
            <SysInfo s={sys} />
            <Hardware s={sys} />
            <CpuInfo s={sys} />
            <RamWatcher s={sys} />
            <TopList s={sys} />
          </section>
        )}

        {/* ------------------------------------------------------ main_shell */}
        <section className="min-w-0 flex-1 box-border p-[calc(0.74vh*var(--ui-scale))] pt-0">
          <MainShell theme={theme} />
        </section>

        {/* ----------------------------------------------- mod_column_right */}
        {fold.right ? (
          <CollapsedRail label="NETWORK" side="right" onClick={() => toggle("right")} />
        ) : (
          <section className="flex w-[20%] shrink-0 flex-col overflow-hidden box-border p-[calc(1.39vh*var(--ui-scale))] pt-0">
            <Title
              left="PANEL"
              right="NETWORK"
              action={<CollapseButton side="right" collapsed={false} onClick={() => toggle("right")} />}
            />
            <NetStat n={net} />
            <Globe />
            <ConnInfo n={net} />
          </section>
        )}
      </div>

      {/* -------------------------------------------------- section#filesystem */}
      {fold.bottom ? (
        <CollapsedRail label="FILESYSTEM" side="bottom" onClick={() => toggle("bottom")} />
      ) : (
        <section className="box-border flex h-[22vh] shrink-0 flex-col px-[calc(1.39vh*var(--ui-scale))] pb-[calc(1vh*var(--ui-scale))]">
          <FilesystemPanel
            fs={fs}
            action={<CollapseButton side="bottom" collapsed={false} onClick={() => toggle("bottom")} />}
          />
        </section>
      )}
    </div>
  );
}
