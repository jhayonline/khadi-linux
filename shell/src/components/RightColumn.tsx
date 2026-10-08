// mod_column_right — netstat, globe, conninfo.

import { useEffect, useRef } from "react";
import type { Network } from "../lib/ipc";
import { bytes, rate } from "../lib/format";
import land from "../assets/land.json";

/* ----------------------------------------------------------- mod_netstat */

export function NetStat({ n }: { n: Network | null }) {
  return (
    <div className="rule-top py-[0.645vh] tracking-[0.092vh] font-[var(--font-ui-light)]">
      <h3 className="m-0 flex justify-between px-[0.46vh] text-[1.3vh]">
        <span>NETWORK STATUS</span>
        <span className="opacity-50">Interface: {n?.iface || "—"}</span>
      </h3>
      <div className="mt-[0.4vh] flex flex-row items-start justify-between px-[0.46vh]">
        {([
          ["STATE", n ? (n.up ? "ONLINE" : "OFFLINE") : "—"],
          ["IPv4", n?.ipv4 || "—"],
          ["GATEWAY", n?.gateway || "—"],
        ] as [string, string][]).map(([k, v]) => (
          <div key={k}>
            <h2 className="m-0 text-[1.3vh] leading-[1.6vh] opacity-50">{k}</h2>
            <h1 className="m-0 text-[1.6vh] leading-[1.9vh]">{v}</h1>
          </div>
        ))}
      </div>
      <div className="mt-[0.3vh] flex flex-row items-start justify-between px-[0.46vh]">
        {([
          ["DNS", n?.dns || "—"],
          ["MAC", n?.mac || "—"],
          ["MTU", n?.mtu || "—"],
        ] as [string, string][]).map(([k, v]) => (
          <div key={k}>
            <h2 className="m-0 text-[1.1vh] leading-[1.4vh] opacity-50">{k}</h2>
            <h1 className="m-0 text-[1.3vh] leading-[1.6vh]">{v}</h1>
          </div>
        ))}
      </div>
    </div>
  );
}

/* ------------------------------------------------------------- mod_globe */

/** eDEX's own 3937 continent tiles, out of src/assets/misc/grid.json.
 *
 * encom-globe draws them as WebGL hexagons; this draws them as points on a
 * rotating sphere, which is the part that actually reads at panel size. The
 * geolocation stays gone — GeoLite2 needs an account and carries
 * redistribution terms an ISO should not take on — so the globe shows the
 * world without claiming to know where anyone is. */
export function Globe() {
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const cv = ref.current;
    if (!cv) return;
    const ctx = cv.getContext("2d");
    if (!ctx) return;
    const pts = land as [number, number][];
    let raf = 0;
    let spin = 0;
    let last = performance.now();

    const frame = (now: number) => {
      // Six degrees a second: one turn a minute, fast enough to read as alive
      // and slow enough not to pull the eye.
      spin = (spin + ((now - last) / 1000) * 6) % 360;
      last = now;
      const w = (cv.width = cv.clientWidth * devicePixelRatio);
      const h = (cv.height = cv.clientHeight * devicePixelRatio);
      ctx.clearRect(0, 0, w, h);
      const css = getComputedStyle(document.documentElement);
      const c = css.getPropertyValue("--c").trim();
      const r = Math.min(w, h) * 0.46;
      const cx = w / 2;
      const cy = h / 2;
      const rad = Math.PI / 180;
      // The meridian/parallel cage first, under the land.
      ctx.strokeStyle = `rgba(${c}, 0.12)`;
      ctx.lineWidth = devicePixelRatio;
      for (let latd = -60; latd <= 60; latd += 30) {
        const y = Math.sin(latd * rad);
        const rr = Math.cos(latd * rad);
        ctx.beginPath();
        ctx.ellipse(cx, cy - y * r, rr * r, rr * r * 0.18, 0, 0, Math.PI * 2);
        ctx.stroke();
      }
      ctx.beginPath();
      ctx.arc(cx, cy, r, 0, Math.PI * 2);
      ctx.stroke();

      const dot = r * 0.016;
      for (const [lat, lon] of pts) {
        const la = lat * rad;
        const lo = (lon + spin) * rad;
        const x = Math.cos(la) * Math.sin(lo);
        const z = Math.cos(la) * Math.cos(lo);
        const y = Math.sin(la);
        if (z < 0) continue; // the far side
        // Fade toward the limb, which is what gives the dot matrix its
        // curvature instead of reading as a flat disc.
        ctx.fillStyle = `rgba(${c}, ${(0.25 + 0.75 * z).toFixed(3)})`;
        ctx.beginPath();
        ctx.arc(cx + x * r, cy - y * r, dot * (0.5 + 0.5 * z), 0, Math.PI * 2);
        ctx.fill();
      }
      raf = requestAnimationFrame(frame);
    };
    raf = requestAnimationFrame(frame);
    return () => cancelAnimationFrame(raf);
  }, []);

  return (
    <div className="rule-top flex w-full flex-col py-[0.645vh] tracking-[0.092vh] font-[var(--font-ui-light)]">
      <h3 className="m-0 flex justify-between px-[0.46vh] text-[1.3vh]">
        <span>WORLD VIEW</span>
        <span className="opacity-50">NO GEOIP</span>
      </h3>
      <canvas ref={ref} className="h-[22vh] w-full" />
    </div>
  );
}

/* ---------------------------------------------------------- mod_conninfo */

export function ConnInfo({ n }: { n: Network | null }) {
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const cv = ref.current;
    if (!cv || !n) return;
    const ctx = cv.getContext("2d");
    if (!ctx) return;
    const w = (cv.width = cv.clientWidth * devicePixelRatio);
    const h = (cv.height = cv.clientHeight * devicePixelRatio);
    ctx.clearRect(0, 0, w, h);
    const css = getComputedStyle(document.documentElement);
    const c = css.getPropertyValue("--c").trim();
    // Scaled to the window's own peak: absolute byte rates span six orders of
    // magnitude and a fixed scale is either flat or clipped.
    const peak = Math.max(1, ...n.rx_hist, ...n.tx_hist);
    ctx.strokeStyle = `rgba(${c}, 0.15)`;
    ctx.lineWidth = devicePixelRatio;
    for (let i = 1; i < 4; i++) {
      ctx.beginPath();
      ctx.moveTo(0, (h / 4) * i);
      ctx.lineTo(w, (h / 4) * i);
      ctx.stroke();
    }
    const draw = (data: number[], alpha: number) => {
      if (data.length < 2) return;
      ctx.beginPath();
      ctx.lineWidth = 1.5 * devicePixelRatio;
      ctx.strokeStyle = `rgba(${c}, ${alpha})`;
      data.forEach((v, i) => {
        const x = (i / (data.length - 1)) * w;
        const y = h - (v / peak) * h * 0.95;
        i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y);
      });
      ctx.stroke();
    };
    draw(n.rx_hist, 1);
    draw(n.tx_hist, 0.6);
  }, [n]);

  return (
    <div className="rule-top py-[0.645vh] tracking-[0.092vh] font-[var(--font-ui-light)]">
      <h3 className="m-0 flex justify-between px-[0.46vh] text-[1.3vh]">
        <span>NETWORK TRAFFIC</span>
        <span className="opacity-50">UP / DOWN</span>
      </h3>
      <div className="flex justify-between px-[0.46vh] text-[1.3vh]">
        <span className="opacity-50">
          TOTAL {n ? bytes(n.tx_total) : "—"} OUT, {n ? bytes(n.rx_total) : "—"} IN
        </span>
      </div>
      <canvas ref={ref} className="my-[0.4vh] h-[9vh] w-full" />
      <div className="flex justify-between px-[0.46vh] text-[1.3vh]">
        <span>DOWN {n ? rate(n.rx_rate) : "—"}</span>
        <span className="opacity-70">UP {n ? rate(n.tx_rate) : "—"}</span>
      </div>
      <div className="mt-[0.5vh] px-[0.46vh] text-[1.3vh]">
        <div className="flex justify-between">
          <span>SOCKETS</span>
          <span className="opacity-50">{n?.established ?? 0} ESTABLISHED</span>
        </div>
        {(n?.peers ?? []).slice(0, 6).map((p) => (
          <div key={p.addr} className="flex justify-between opacity-70">
            <span className="truncate">{p.addr}</span>
            {p.count > 1 && <span className="opacity-60">x{p.count}</span>}
          </div>
        ))}
      </div>
    </div>
  );
}
