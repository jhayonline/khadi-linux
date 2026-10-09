// The Khadi wordmark.
//
// Same pixel grid the boot splash is rasterised from — `wordmark.json` is the
// one definition, and a logo that differs between the boot screen and the
// login screen is worse than no logo. Here it is inline SVG, so it scales to
// any size, takes the theme colour through `currentColor`, and needs no asset
// pipeline to reach a webview.
//
// The face is after Omarchy's: a coarse bitmap, condensed, blackletter-ish,
// every diagonal a stair. 7 cells wide by 13 tall per letter, measured off
// its logo.png.

import spec from "../assets/wordmark.json";

type Spec = { cell_w: number; cell_h: number; glyphs: Record<string, string[]> };

export function Wordmark({ text = "KHADI", className = "" }: { text?: string; className?: string }) {
  const { cell_w: gw, cell_h: gh, glyphs } = spec as Spec;
  const GAP = 1;
  const cols = text.length * gw + (text.length - 1) * GAP;

  const rects: React.ReactElement[] = [];
  let x0 = 0;
  for (const ch of text) {
    const g = glyphs[ch];
    if (!g) continue;
    g.forEach((row, y) => {
      // One rect per RUN of lit cells, not per cell: a run of four is four
      // nodes the browser does not have to lay out, and the edges stay hard
      // because adjacent rects would otherwise seam at fractional scales.
      let run = 0;
      for (let x = 0; x <= row.length; x++) {
        if (row[x] === "#") {
          run++;
          continue;
        }
        if (run > 0) {
          rects.push(
            <rect key={`${x0}-${y}-${x}`} x={x0 + x - run} y={y} width={run} height={1} />,
          );
          run = 0;
        }
      }
    });
    x0 += gw + GAP;
  }

  return (
    <svg
      viewBox={`0 0 ${cols} ${gh}`}
      className={className}
      fill="currentColor"
      shapeRendering="crispEdges"
      role="img"
      aria-label={text}
    >
      {rects}
    </svg>
  );
}
