// The eDEX composition.
//
// Geometry out of edex-ui/src/assets/css: columns at 17%, main shell at 65%
// wide and 60.3% tall, filesystem 43vw x 30vh, keyboard 55.5vw. The side
// columns are absolutely positioned and the three flow children wrap, which
// is how eDEX gets the terminal on one row and the browser plus keyboard on
// the next.

import { useEffect, useState } from "react";
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
import { Keyboard } from "./components/Keyboard";

/** Turn the on-screen keyboard off here if you want the terminal taller. */
const showKeyboard = true;

/** Poll on a timer, as eDEX does. The Rust side refreshes on demand, so the
 *  interval is the only clock in the system. */
function usePoll<T>(fn: () => Promise<T>, ms: number): T | null {
  const [v, setV] = useState<T | null>(null);
  useEffect(() => {
    let alive = true;
    const run = () => {
      fn()
        .then((x) => alive && setV(x))
        .catch(() => {});
    };
    run();
    const t = setInterval(run, ms);
    return () => {
      alive = false;
      clearInterval(t);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ms]);
  return v;
}

export default function App() {
  const [theme, setTheme] = useState<Theme | null>(null);
  const sys = usePoll<System>(getSystem, 1000);
  const net = usePoll<Network>(getNetwork, 1000);
  const fs = usePoll<Filesystem>(getFilesystem, 2000);

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

  return (
    <div className="relative flex h-full w-full flex-row flex-wrap items-center justify-center pt-[1.85vh]">
      {/* ------------------------------------------------ mod_column_left */}
      <section className="absolute left-[-0.555vh] top-[2.5vh] z-10 flex max-h-[96%] w-[17%] flex-col items-end overflow-hidden box-border p-[1.39vh] pb-0">
        <div className="w-full">
          <Title left="PANEL" right="SYSTEM" />
          <Clock />
          <SysInfo s={sys} />
          <Hardware s={sys} />
          <CpuInfo s={sys} />
          <RamWatcher s={sys} />
        </div>
        <TopList s={sys} />
      </section>

      {/* ----------------------------------------------- mod_column_right */}
      <section className="absolute right-[-0.555vh] top-[2.5vh] z-10 flex max-h-[96%] w-[17%] flex-col items-start overflow-hidden box-border p-[1.39vh] pb-0">
        <div className="w-full">
          <Title left="PANEL" right="NETWORK" />
          <NetStat n={net} />
          <Globe />
          <ConnInfo n={net} />
        </div>
      </section>

      {/* ------------------------------------------------------ main_shell */}
      <section
        className="box-border p-[0.74vh]"
        style={{ width: "65%", height: showKeyboard ? "60.3%" : "88%" }}
      >
        <MainShell theme={theme} />
      </section>

      {/* ------------------------------------- filesystem + keyboard row */}
      {/* ONE container, not two flow siblings. As siblings the filesystem's
          17% inset counted against the row's width, the total passed 100vw and
          the keyboard wrapped onto a third row that fell off the bottom of the
          screen. The row clears the left column with padding and splits what
          is left. */}
      <div className="flex h-[30vh] w-full flex-row items-stretch pl-[17%]">
        <section
          className="relative top-[-0.925vh] box-border flex min-h-0 flex-col px-[1vh]"
          style={{ width: showKeyboard ? "40%" : "100%" }}
        >
          <FilesystemPanel fs={fs} />
        </section>
        {showKeyboard && (
          <section className="box-border min-w-0 flex-1 pr-[1vh]">
            <Keyboard />
          </section>
        )}
      </div>
    </div>
  );
}
