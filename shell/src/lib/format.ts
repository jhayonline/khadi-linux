// The number formatting the panels share. Ported from khadi-core's `rate`,
// `total` and `dur`, because the shell shows the same values the TUI did and
// two spellings of "1.69 GiB" on one desktop is one too many.

export const bytes = (b: number): string => {
  const K = 1024;
  if (b < K * K) return `${(b / K).toFixed(0)} KiB`;
  if (b < K * K * K) return `${(b / (K * K)).toFixed(1)} MiB`;
  return `${(b / (K * K * K)).toFixed(2)} GiB`;
};

export const rate = (bps: number): string => {
  const K = 1024;
  if (bps < K) return `${bps.toFixed(0)} B/s`;
  if (bps < K * K) return `${(bps / K).toFixed(1)} K/s`;
  return `${(bps / (K * K)).toFixed(1)} M/s`;
};

export const gib = (b: number): number => b / 1073741824;

/** Short enough for a quarter-width cell: `3d 6h`, `6h 21m`, `18m`. */
export const dur = (secs: number): string => {
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  const m = Math.floor((secs % 3600) / 60);
  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m`;
};

/** eDEX's uptime is `H:MM:SS`, counting hours past a day rather than days. */
export const clockDur = (secs: number): string => {
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = Math.floor(secs % 60);
  return `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
};

/** Truncate with an ellipsis, so a clipped value is visibly clipped. */
export const ellipsize = (s: string, max: number): string =>
  s.length <= max ? s : s.slice(0, Math.max(0, max - 1)) + "…";
