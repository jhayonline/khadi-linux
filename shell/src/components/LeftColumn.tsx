// mod_column_left — clock, sysinfo, hardware, cpu, ram, toplist.
//
// Sizes are eDEX's, read out of its stylesheets: the clock is 7.41vh with 4vh
// digits in 2.3vh cells, sysinfo is 5.556vh at 1.111vh, the rest is 1.3vh on a
// 1.5vh line. Where a number looks arbitrary it is because it is eDEX's.

import { useEffect, useRef } from "react";
import type { System } from "../lib/ipc";
import { bytes, clockDur, gib } from "../lib/format";

/* ------------------------------------------------------------- mod_clock */

export function Clock() {
  const ref = useRef<HTMLHeadingElement>(null);
  useEffect(() => {
    const tick = () => {
      const d = new Date();
      const p = (n: number) => String(n).padStart(2, "0");
      const s = `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
      if (!ref.current) return;
      // Each glyph gets its own fixed-width cell so the clock does not jitter
      // as the digits change — eDEX's `span` for digits, `em` for colons.
      ref.current.innerHTML = [...s]
        .map((c) => (c === ":" ? `<em>:</em>` : `<span>${c}</span>`))
        .join("");
    };
    tick();
    const t = setInterval(tick, 1000);
    return () => clearInterval(t);
  }, []);
  return (
    <div className="rule-top flex h-[7.41vh] pt-[0.645vh] font-[var(--font-ui-light)]">
      <h1 ref={ref} className="m-auto text-[4vh] leading-none [&_span]:inline-block [&_span]:w-[2.3vh] [&_span]:text-center [&_em]:inline-block [&_em]:w-[2.5vh] [&_em]:not-italic [&_em]:text-center" />
    </div>
  );
}

/* ----------------------------------------------------------- mod_sysinfo */

export function SysInfo({ s }: { s: System | null }) {
  const d = new Date();
  const cells: [string, string][] = [
    [String(d.getFullYear()), d.toLocaleString("en", { month: "short", day: "2-digit" }).toUpperCase()],
    ["UPTIME", s ? clockDur(s.uptime_secs) : "—"],
    ["TYPE", "linux"],
    ["TASKS", s ? String(s.tasks) : "—"],
  ];
  return (
    <div className="rule-top flex h-[5.556vh] flex-row items-center justify-between text-[1.111vh] tracking-[0.092vh] font-[var(--font-ui-light)]">
      {cells.map(([k, v]) => (
        <div key={k} className="flex h-full flex-col items-start justify-around box-border px-[0.46vh] py-[0.925vh]">
          <h1 className="m-0 opacity-50">{k}</h1>
          <h2 className="m-0">{v}</h2>
        </div>
      ))}
    </div>
  );
}

/* -------------------------------------------------- mod_hardwareInspector */

export function Hardware({ s }: { s: System | null }) {
  const cells: [string, string][] = [
    ["MANUFACTURER", s?.vendor || "—"],
    ["MODEL", s?.model || "—"],
    ["CHASSIS", s?.chassis || "—"],
  ];
  return (
    <div className="rule-top flex py-[0.645vh] tracking-[0.092vh] font-[var(--font-ui-light)]">
      <div className="flex w-full flex-row flex-wrap items-center justify-evenly">
        {cells.map(([k, v]) => (
          <div key={k} className="text-left">
            <h2 className="m-0 text-[1.3vh] leading-[1.5vh] opacity-50">{k}</h2>
            <h1 className="m-0 max-w-[6vw] truncate text-[1.3vh] leading-[1.5vh]">{v}</h1>
          </div>
        ))}
      </div>
    </div>
  );
}

/* ----------------------------------------------------------- mod_cpuinfo */

/** One canvas per core PAIR, both cores drawn in it — eDEX's arrangement. */
function CoreGraph({ a, b }: { a: number[]; b: number[] }) {
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const cv = ref.current;
    if (!cv) return;
    const ctx = cv.getContext("2d");
    if (!ctx) return;
    const w = (cv.width = cv.clientWidth * devicePixelRatio);
    const h = (cv.height = cv.clientHeight * devicePixelRatio);
    ctx.clearRect(0, 0, w, h);
    const css = getComputedStyle(document.documentElement);
    const c = css.getPropertyValue("--c").trim();
    const draw = (data: number[], alpha: number) => {
      if (data.length < 2) return;
      ctx.beginPath();
      ctx.lineWidth = 1.5 * devicePixelRatio;
      ctx.strokeStyle = `rgba(${c}, ${alpha})`;
      ctx.lineJoin = "round";
      data.forEach((v, i) => {
        const x = (i / (data.length - 1)) * w;
        const y = h - (Math.min(100, Math.max(0, v)) / 100) * h;
        i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y);
      });
      ctx.stroke();
    };
    draw(a, 1);
    draw(b, 0.6);
  }, [a, b]);
  return <canvas ref={ref} className="dashed-top dashed-bottom my-[0.46vh] h-[4.167vh] w-[76%]" />;
}

export function CpuInfo({ s }: { s: System | null }) {
  const cores = s?.cores ?? [];
  const pairs = Math.ceil(cores.length / 2);
  const avg = (v: number[] | undefined) => (v && v.length ? v[v.length - 1] : 0);
  return (
    <div className="rule-top flex py-[0.645vh] tracking-[0.092vh] font-[var(--font-ui-light)]">
      <div className="flex w-full flex-col items-center justify-between">
        <h1 className="m-0 mb-[-1.5vh] w-[98%] pl-[2%] text-[1.48vh]">
          CPU USAGE
          <i className="relative bottom-[1.9vh] inline-block w-full text-right text-[1.2vh] not-italic opacity-50">
            {s?.cpu_model || ""}
          </i>
        </h1>
        {Array.from({ length: pairs }, (_, i) => (
          <div key={i} className="my-[0.278vh] flex w-full flex-row items-center justify-between">
            <div className="text-[1.3vh] leading-[1.5vh]">
              <h1 className="m-0 text-[1.3vh] leading-[1.5vh]">
                #{i * 2 + 1}-{i * 2 + 2}
              </h1>
              <i className="mt-[0.5vh] text-[1.3vh] not-italic opacity-50">
                Avg. {((avg(cores[i * 2]) + avg(cores[i * 2 + 1] ?? cores[i * 2])) / 2).toFixed(0)}%
              </i>
            </div>
            <CoreGraph a={cores[i * 2] ?? []} b={cores[i * 2 + 1] ?? []} />
          </div>
        ))}
        <div className="dashed-top flex w-[95%] flex-row items-center justify-between pt-[0.838vh]">
          {([
            ["TEMP", s?.temp_c != null ? `${s.temp_c.toFixed(0)}°C` : "—"],
            ["CORES", s ? String(s.cores_total) : "—"],
            ["TASKS", s ? String(s.tasks) : "—"],
            ["HOST", s?.host || "—"],
          ] as [string, string][]).map(([k, v]) => (
            <div key={k} className="w-[20%] text-center">
              <h1 className="m-0 text-[1.3vh] leading-[1.5vh]">{k}</h1>
              <i className="text-[1.3vh] not-italic opacity-50">{v}</i>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

/* -------------------------------------------------------- mod_ramwatcher */

export function RamWatcher({ s }: { s: System | null }) {
  // eDEX's block is a page map, so it looks scattered. There is no page map
  // without root, so the same count of cells is spread instead of packed —
  // the number is honest either way, and neither version claims to know WHICH
  // pages are in use.
  const COLS = 46;
  const ROWS = 8;
  const cells = COLS * ROWS;
  const frac = s && s.mem_total ? s.mem_used / s.mem_total : 0;
  const used = Math.round(frac * cells);
  const step = 2 * Math.round((cells * 0.309) / 2) + 1;
  const swap = s && s.swap_total ? s.swap_used / s.swap_total : 0;
  return (
    <div className="rule-top py-[0.645vh] tracking-[0.092vh] font-[var(--font-ui-light)]">
      <h3 className="m-0 flex justify-between px-[0.46vh] text-[1.3vh]">
        <span>MEMORY</span>
        <span className="opacity-50">
          {s ? `USING ${gib(s.mem_used).toFixed(1)} OUT OF ${gib(s.mem_total).toFixed(1)} GIB` : ""}
        </span>
      </h3>
      <div
        className="my-[0.5vh] grid gap-[0.14vh] px-[0.46vh]"
        style={{ gridTemplateColumns: `repeat(${COLS}, 1fr)` }}
      >
        {Array.from({ length: cells }, (_, n) => (
          <span
            key={n}
            className="aspect-square"
            style={{
              background: (n * step) % cells < used ? "rgb(var(--c))" : "rgba(var(--c), 0.14)",
            }}
          />
        ))}
      </div>
      <div className="flex items-center gap-[0.6vh] px-[0.46vh] text-[1.3vh]">
        <span>SWAP</span>
        <div className="h-[0.56vh] flex-1 bg-[rgba(var(--c),0.2)]">
          <div className="h-full bg-[rgb(var(--c))]" style={{ width: `${swap * 100}%` }} />
        </div>
        <span className="opacity-50">{s ? `${gib(s.swap_used).toFixed(1)} GiB` : ""}</span>
      </div>
    </div>
  );
}

/* ----------------------------------------------------------- mod_toplist */

export function TopList({ s }: { s: System | null }) {
  return (
    <div className="rule-top flex w-full flex-col py-[0.645vh] tracking-[0.092vh] font-[var(--font-ui-light)]">
      <h3 className="m-0 flex justify-between px-[0.46vh] text-[1.3vh]">
        <span>TOP PROCESSES</span>
        <span className="opacity-50">PID | NAME | CPU | MEM</span>
      </h3>
      <div className="mt-[0.4vh] overflow-hidden px-[0.46vh] text-[1.3vh] leading-[1.75vh]">
        {(s?.procs ?? []).slice(0, 9).map((p, i) => (
          <div key={`${p.pid}-${i}`} className="flex justify-between gap-[0.4vh]">
            <span className="w-[22%] opacity-50">{p.pid}</span>
            <span className="flex-1 truncate font-[var(--font-ui)]">{p.name}</span>
            <span className="w-[18%] text-right opacity-70">{p.cpu.toFixed(1)}%</span>
            <span className="w-[20%] text-right opacity-50">{bytes(p.mem).replace(" ", "")}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
