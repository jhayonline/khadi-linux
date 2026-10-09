// mod_column_right — netstat, globe, conninfo.

import { useEffect, useRef } from "react";
import type { Network } from "../lib/ipc";
import { bytes, rate } from "../lib/format";
import land from "../assets/land.json";

/* ----------------------------------------------------------- mod_netstat */

export function NetStat({ n }: { n: Network | null }) {
  return (
    <div className="rule-top py-[calc(0.645vh*var(--ui-scale))] tracking-[0.092vh] font-[var(--font-ui-light)]">
      <h3 className="m-0 flex justify-between px-[calc(0.46vh*var(--ui-scale))] text-[calc(1.3vh*var(--ui-scale))]">
        <span>NETWORK STATUS</span>
        <span className="opacity-50">Interface: {n?.iface || "—"}</span>
      </h3>
      <div className="mt-[calc(0.4vh*var(--ui-scale))] flex flex-row items-start justify-between px-[calc(0.46vh*var(--ui-scale))]">
        {([
          ["STATE", n ? (n.up ? "ONLINE" : "OFFLINE") : "—"],
          ["IPv4", n?.ipv4 || "—"],
          ["GATEWAY", n?.gateway || "—"],
        ] as [string, string][]).map(([k, v]) => (
          <div key={k}>
            <h2 className="m-0 text-[calc(1.3vh*var(--ui-scale))] leading-[calc(1.6vh*var(--ui-scale))] opacity-50">{k}</h2>
            <h1 className="m-0 text-[calc(1.6vh*var(--ui-scale))] leading-[calc(1.9vh*var(--ui-scale))]">{v}</h1>
          </div>
        ))}
      </div>
      <div className="mt-[calc(0.3vh*var(--ui-scale))] flex flex-row items-start justify-between px-[calc(0.46vh*var(--ui-scale))]">
        {([
          ["DNS", n?.dns || "—"],
          ["MAC", n?.mac || "—"],
          ["MTU", n?.mtu || "—"],
        ] as [string, string][]).map(([k, v]) => (
          <div key={k}>
            <h2 className="m-0 text-[calc(1.1vh*var(--ui-scale))] leading-[calc(1.4vh*var(--ui-scale))] opacity-50">{k}</h2>
            <h1 className="m-0 text-[calc(1.3vh*var(--ui-scale))] leading-[calc(1.6vh*var(--ui-scale))]">{v}</h1>
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
    const ctx = cv.getContext("2d", { alpha: true });
    if (!ctx) return;

    // Trig once, not 3937 times a frame. Only the spin changes between
    // frames, so everything that depends solely on the tile is precomputed.
    const rad = Math.PI / 180;
    const pts = land as [number, number][];
    const n = pts.length;
    const sinLat = new Float32Array(n);
    const cosLat = new Float32Array(n);
    const lon = new Float32Array(n);
    for (let i = 0; i < n; i++) {
      sinLat[i] = Math.sin(pts[i][0] * rad);
      cosLat[i] = Math.cos(pts[i][0] * rad);
      lon[i] = pts[i][1] * rad;
    }

    // Four depth buckets instead of a per-point fillStyle. Each bucket is one
    // path and one fill, so a frame costs 4 fills rather than 3937 arc+fill
    // pairs — which is what made the whole UI feel heavy, because canvas work
    // on the main thread blocks everything else the webview wants to do.
    const BUCKETS = 4;
    const ALPHA = [0.3, 0.5, 0.75, 1];

    let raf = 0;
    let spin = 0;
    let last = performance.now();
    let acc = 0;
    const FRAME = 1000 / 24; // eDEX's globe turns slowly; 24fps reads the same

    const frame = (now: number) => {
      raf = requestAnimationFrame(frame);
      const dt = now - last;
      last = now;
      acc += dt;
      if (acc < FRAME) return;
      acc = 0;

      // Six degrees a second: one turn a minute, fast enough to read as alive
      // and slow enough not to pull the eye.
      spin = (spin + (dt / 1000) * 6) % 360;

      const w = cv.clientWidth * devicePixelRatio;
      const h = cv.clientHeight * devicePixelRatio;
      if (cv.width !== w || cv.height !== h) {
        cv.width = w;
        cv.height = h;
      }
      ctx.clearRect(0, 0, w, h);

      const c = getComputedStyle(document.documentElement).getPropertyValue("--c").trim();
      const r = Math.min(w, h) * 0.46;
      const cx = w / 2;
      const cy = h / 2;

      // The cage, under the land.
      ctx.strokeStyle = `rgba(${c}, 0.12)`;
      ctx.lineWidth = devicePixelRatio;
      ctx.beginPath();
      for (let latd = -60; latd <= 60; latd += 30) {
        const y = Math.sin(latd * rad);
        const rr = Math.cos(latd * rad);
        ctx.ellipse(cx, cy - y * r, rr * r, rr * r * 0.18, 0, 0, Math.PI * 2);
      }
      ctx.arc(cx, cy, r, 0, Math.PI * 2);
      ctx.stroke();

      const spinRad = spin * rad;
      const sinSpin = Math.sin(spinRad);
      const cosSpin = Math.cos(spinRad);
      const d = Math.max(1, r * 0.028);

      for (let b = 0; b < BUCKETS; b++) {
        ctx.beginPath();
        const lo = b / BUCKETS;
        const hi = (b + 1) / BUCKETS;
        for (let i = 0; i < n; i++) {
          // sin(lon + spin) and cos(lon + spin), expanded so the per-tile trig
          // stays in the precomputed arrays.
          const sl = Math.sin(lon[i]) * cosSpin + Math.cos(lon[i]) * sinSpin;
          const cl = Math.cos(lon[i]) * cosSpin - Math.sin(lon[i]) * sinSpin;
          const z = cosLat[i] * cl;
          if (z <= lo || z > hi) continue; // far side, or another bucket
          const x = cosLat[i] * sl;
          const y = sinLat[i];
          const sz = d * (0.55 + 0.45 * z);
          ctx.rect(cx + x * r - sz / 2, cy - y * r - sz / 2, sz, sz);
        }
        ctx.fillStyle = `rgba(${c}, ${ALPHA[b]})`;
        ctx.fill();
      }
    };
    raf = requestAnimationFrame(frame);
    return () => cancelAnimationFrame(raf);
  }, []);

  return (
    <div className="rule-top flex w-full flex-col py-[calc(0.645vh*var(--ui-scale))] tracking-[0.092vh] font-[var(--font-ui-light)]">
      <h3 className="m-0 flex justify-between px-[calc(0.46vh*var(--ui-scale))] text-[calc(1.3vh*var(--ui-scale))]">
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
    <div className="rule-top py-[calc(0.645vh*var(--ui-scale))] tracking-[0.092vh] font-[var(--font-ui-light)]">
      <h3 className="m-0 flex justify-between px-[calc(0.46vh*var(--ui-scale))] text-[calc(1.3vh*var(--ui-scale))]">
        <span>NETWORK TRAFFIC</span>
        <span className="opacity-50">UP / DOWN</span>
      </h3>
      <div className="flex justify-between px-[calc(0.46vh*var(--ui-scale))] text-[calc(1.3vh*var(--ui-scale))]">
        <span className="opacity-50">
          TOTAL {n ? bytes(n.tx_total) : "—"} OUT, {n ? bytes(n.rx_total) : "—"} IN
        </span>
      </div>
      <canvas ref={ref} className="my-[calc(0.4vh*var(--ui-scale))] h-[calc(9vh*var(--ui-scale))] w-full" />
      <div className="flex justify-between px-[calc(0.46vh*var(--ui-scale))] text-[calc(1.3vh*var(--ui-scale))]">
        <span>DOWN {n ? rate(n.rx_rate) : "—"}</span>
        <span className="opacity-70">UP {n ? rate(n.tx_rate) : "—"}</span>
      </div>
      <div className="mt-[calc(0.5vh*var(--ui-scale))] px-[calc(0.46vh*var(--ui-scale))] text-[calc(1.3vh*var(--ui-scale))]">
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
