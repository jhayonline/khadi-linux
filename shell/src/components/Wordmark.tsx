// The Khadi wordmark.
//
// The same pixel grid the boot splash is rasterised from — `wordmark.json` is
// the one definition, and a logo that differs between the boot screen and the
// login screen is worse than no logo. Inline SVG here, so it scales to any
// size, takes the theme colour through `currentColor`, and needs no asset
// pipeline to reach a webview.
//
// The face is Delta Corps Priest 1, the FIGlet font Omarchy's own wordmark is
// drawn in. FIGlet renders it in half-blocks, which are a pixel grid at twice
// the resolution in both directions, so `logo.txt` converts into that grid
// with nothing lost. Two earlier versions of this file redrew the letterforms
// by hand; these are the real ones.

import spec from "../assets/wordmark.json";

type Spec = { w: number; h: number; rows: string[] };

export function Wordmark({ className = "" }: { className?: string }) {
  const { w, h, rows } = spec as Spec;

  // One rect per RUN of lit cells, not per cell. The grid is 109x34 and
  // mostly ink, so runs turn roughly 1,500 nodes into about 200 — and
  // adjacent rects would seam at fractional scales where a run does not.
  const rects: React.ReactElement[] = [];
  rows.forEach((row, y) => {
    let run = 0;
    for (let x = 0; x <= row.length; x++) {
      if (row[x] === "#") {
        run++;
        continue;
      }
      if (run > 0) {
        rects.push(<rect key={`${y}-${x}`} x={x - run} y={y} width={run} height={1} />);
        run = 0;
      }
    }
  });

  return (
    <svg
      viewBox={`0 0 ${w} ${h}`}
      className={className}
      fill="currentColor"
      shapeRendering="crispEdges"
      role="img"
      aria-label="KHADI"
    >
      {rects}
    </svg>
  );
}
