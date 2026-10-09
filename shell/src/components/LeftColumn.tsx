// mod_column_left — clock, sysinfo, hardware, cpu, ram, toplist.
//
// Sizes are eDEX's, read out of its stylesheets: the clock is 7.41vh with 4vh
// digits in 2.3vh cells, sysinfo is 5.556vh at 1.111vh, the rest is 1.3vh on a
// 1.5vh line. Where a number looks arbitrary it is because it is eDEX's.

import { memo, useEffect, useMemo, useRef, useState } from "react";
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
    <div className="rule-top flex h-[calc(7.41vh*var(--ui-scale))] pt-[calc(0.645vh*var(--ui-scale))] font-[var(--font-ui-light)]">
      <h1 ref={ref} className="m-auto text-[calc(4vh*var(--ui-scale))] leading-none [&_span]:inline-block [&_span]:w-[calc(2.3vh*var(--ui-scale))] [&_span]:text-center [&_em]:inline-block [&_em]:w-[calc(2.5vh*var(--ui-scale))] [&_em]:not-italic [&_em]:text-center" />
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
    <div className="rule-top flex h-[calc(5.556vh*var(--ui-scale))] flex-row items-center justify-between text-[calc(1.111vh*var(--ui-scale))] tracking-[0.092vh] font-[var(--font-ui-light)]">
      {cells.map(([k, v]) => (
        <div key={k} className="flex h-full flex-col items-start justify-around box-border px-[calc(0.46vh*var(--ui-scale))] py-[calc(0.925vh*var(--ui-scale))]">
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
    <div className="rule-top flex py-[calc(0.645vh*var(--ui-scale))] tracking-[0.092vh] font-[var(--font-ui-light)]">
      <div className="flex w-full flex-row flex-wrap items-center justify-evenly">
        {cells.map(([k, v]) => (
          <div key={k} className="text-left">
            <h2 className="m-0 text-[calc(1.3vh*var(--ui-scale))] leading-[calc(1.5vh*var(--ui-scale))] opacity-50">{k}</h2>
            <h1 className="m-0 max-w-[6vw] truncate text-[calc(1.3vh*var(--ui-scale))] leading-[calc(1.5vh*var(--ui-scale))]">{v}</h1>
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
  return <canvas ref={ref} className="dashed-top dashed-bottom my-[calc(0.46vh*var(--ui-scale))] h-[calc(4.167vh*var(--ui-scale))] w-[76%]" />;
}

export function CpuInfo({ s }: { s: System | null }) {
  const cores = s?.cores ?? [];
  const pairs = Math.ceil(cores.length / 2);
  const avg = (v: number[] | undefined) => (v && v.length ? v[v.length - 1] : 0);
  return (
    <div className="rule-top flex py-[calc(0.645vh*var(--ui-scale))] tracking-[0.092vh] font-[var(--font-ui-light)]">
      <div className="flex w-full flex-col items-center justify-between">
        <h1 className="m-0 mb-[-1.5vh] w-[98%] pl-[2%] text-[calc(1.48vh*var(--ui-scale))]">
          CPU USAGE
          <i className="relative bottom-[calc(1.9vh*var(--ui-scale))] inline-block w-full text-right text-[calc(1.2vh*var(--ui-scale))] not-italic opacity-50">
            {s?.cpu_model || ""}
          </i>
        </h1>
        {Array.from({ length: pairs }, (_, i) => (
          <div key={i} className="my-[calc(0.278vh*var(--ui-scale))] flex w-full flex-row items-center justify-between">
            <div className="text-[calc(1.3vh*var(--ui-scale))] leading-[calc(1.5vh*var(--ui-scale))]">
              <h1 className="m-0 text-[calc(1.3vh*var(--ui-scale))] leading-[calc(1.5vh*var(--ui-scale))]">
                #{i * 2 + 1}-{i * 2 + 2}
              </h1>
              <i className="mt-[calc(0.5vh*var(--ui-scale))] text-[calc(1.3vh*var(--ui-scale))] not-italic opacity-50">
                Avg. {((avg(cores[i * 2]) + avg(cores[i * 2 + 1] ?? cores[i * 2])) / 2).toFixed(0)}%
              </i>
            </div>
            <CoreGraph a={cores[i * 2] ?? []} b={cores[i * 2 + 1] ?? []} />
          </div>
        ))}
        <div className="dashed-top flex w-[95%] flex-row items-center justify-between pt-[calc(0.838vh*var(--ui-scale))]">
          {([
            ["TEMP", s?.temp_c != null ? `${s.temp_c.toFixed(0)}°C` : "—"],
            ["CORES", s ? String(s.cores_total) : "—"],
            ["TASKS", s ? String(s.tasks) : "—"],
            ["HOST", s?.host || "—"],
          ] as [string, string][]).map(([k, v]) => (
            <div key={k} className="w-[20%] text-center">
              <h1 className="m-0 text-[calc(1.3vh*var(--ui-scale))] leading-[calc(1.5vh*var(--ui-scale))]">{k}</h1>
              <i className="text-[calc(1.3vh*var(--ui-scale))] not-italic opacity-50">{v}</i>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

/* -------------------------------------------------------- mod_ramwatcher */

/** Which cells are lit, spread rather than packed.
 *
 * eDEX's block is a page map and therefore scattered. There is no page map
 * without root, so the same COUNT of cells is distributed instead — the number
 * is honest either way, and neither version claims to know which pages are in
 * use. Multiplying by a step coprime to the cell count is a bijection, so
 * lighting the lowest N lights exactly N.
 */
export function litCells(frac: number, cells: number): boolean[] {
  if (cells <= 0) return [];
  const used = Math.round(Math.min(1, Math.max(0, frac)) * cells);
  // THE STEP MUST BE COPRIME TO THE CELL COUNT or the mapping is not a
  // bijection and the block lights the wrong number. The first version took
  // the golden-ratio step and used it unchecked: at 368 cells that is 115,
  // gcd(115, 368) = 23, and a block claiming 269 lit cells drew 276. The Rust
  // version this was ported from searched for a coprime step; the port lost
  // the search. A test found it, which is the entire argument for having any.
  const gcd = (a: number, b: number): number => (b === 0 ? a : gcd(b, a % b));
  let step = 2 * Math.round((cells * 0.309) / 2) + 1;
  while (gcd(step, cells) !== 1 && step < cells * 2) step += 2;
  if (gcd(step, cells) !== 1) step = 1; // nothing coprime found: pack, honestly
  return Array.from({ length: cells }, (_, n) => (n * step) % cells < used);
}

/** Memoised on the LIT COUNT, not on the System object. The object is new on
 *  every poll and 368 spans were being rebuilt once a second for a number that
 *  moves a few times a minute. */
const MemCells = memo(function MemCells({ used, cells, cols }: {
  used: number; cells: number; cols: number;
}) {
  const nodes = useMemo(
    () =>
      litCells(used / cells, cells).map((lit, n) => (
        <span
          key={n}
          className="aspect-square"
          style={{ background: lit ? "rgb(var(--c))" : "rgba(var(--c), 0.14)" }}
        />
      )),
    [used, cells],
  );
  return (
    <div className="my-[calc(0.5vh*var(--ui-scale))] grid gap-[0.14vh] px-[calc(0.46vh*var(--ui-scale))]" style={{ gridTemplateColumns: `repeat(${cols}, 1fr)` }}>
      {nodes}
    </div>
  );
});

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
  const swap = s && s.swap_total ? s.swap_used / s.swap_total : 0;
  return (
    <div className="rule-top py-[calc(0.645vh*var(--ui-scale))] tracking-[0.092vh] font-[var(--font-ui-light)]">
      <h3 className="m-0 flex justify-between px-[calc(0.46vh*var(--ui-scale))] text-[calc(1.3vh*var(--ui-scale))]">
        <span>MEMORY</span>
        <span className="opacity-50">
          {s ? `USING ${gib(s.mem_used).toFixed(1)} OUT OF ${gib(s.mem_total).toFixed(1)} GIB` : ""}
        </span>
      </h3>
      <MemCells used={used} cells={cells} cols={COLS} />
      <div className="flex items-center gap-[calc(0.6vh*var(--ui-scale))] px-[calc(0.46vh*var(--ui-scale))] text-[calc(1.3vh*var(--ui-scale))]">
        <span>SWAP</span>
        <div className="h-[calc(0.56vh*var(--ui-scale))] flex-1 bg-[rgba(var(--c),0.2)]">
          <div className="h-full bg-[rgb(var(--c))]" style={{ width: `${swap * 100}%` }} />
        </div>
        <span className="opacity-50">{s ? `${gib(s.swap_used).toFixed(1)} GiB` : ""}</span>
      </div>
    </div>
  );
}

/* ----------------------------------------------------------- mod_toplist */

/** How many whole rows fit in `ref`, at its own line height.
 *
 * A fixed count cannot be right: the column's height depends on the screen,
 * and the row height depends on `--ui-scale`. Nine rows fitted at 1080 and
 * clipped the ninth in half at 834 — and a half-drawn row reads as a bug, not
 * as "there is more". So it is measured. */
function useRowsThatFit(ref: React.RefObject<HTMLDivElement | null>): number {
  const [n, setN] = useState(6);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const measure = () => {
      const line = parseFloat(getComputedStyle(el).lineHeight);
      if (!Number.isFinite(line) || line <= 0) return;
      setN(Math.max(1, Math.floor(el.clientHeight / line)));
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, [ref]);
  return n;
}

export function TopList({ s }: { s: System | null }) {
  const list = useRef<HTMLDivElement>(null);
  const rows = useRowsThatFit(list);
  return (
    <div className="rule-top flex min-h-0 w-full flex-1 flex-col py-[calc(0.645vh*var(--ui-scale))] tracking-[0.092vh] font-[var(--font-ui-light)]">
      <h3 className="m-0 flex shrink-0 justify-between px-[calc(0.46vh*var(--ui-scale))] text-[calc(1.3vh*var(--ui-scale))]">
        <span>TOP PROCESSES</span>
        <span className="opacity-50">PID | NAME | CPU | MEM</span>
      </h3>
      <div
        ref={list}
        className="mt-[calc(0.4vh*var(--ui-scale))] min-h-0 flex-1 overflow-hidden px-[calc(0.46vh*var(--ui-scale))] text-[calc(1.3vh*var(--ui-scale))] leading-[calc(1.75vh*var(--ui-scale))]"
      >
        {(s?.procs ?? []).slice(0, rows).map((p, i) => (
          <div key={`${p.pid}-${i}`} className="flex justify-between gap-[calc(0.4vh*var(--ui-scale))]">
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
