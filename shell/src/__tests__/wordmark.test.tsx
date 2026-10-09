// The wordmark has one definition and two consumers. These pin the shape of
// the shared file and that the SVG actually draws from it — a logo that
// differs between the boot screen and the login screen is worse than none.

import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import spec from "../assets/wordmark.json";
import { Wordmark } from "../components/Wordmark";

type Spec = { cell_w: number; cell_h: number; glyphs: Record<string, string[]> };
const s = spec as Spec;

describe("wordmark.json", () => {
  it("carries every letter of the name", () => {
    for (const ch of "KHADI") expect(s.glyphs[ch]).toBeDefined();
  });

  it("is a rectangular grid at the declared size", () => {
    // khadi-theme rasterises this with a flat loop and no bounds checking; a
    // ragged row there is a corrupt PNG on the boot screen.
    for (const [name, rows] of Object.entries(s.glyphs)) {
      expect(rows.length, `${name} height`).toBe(s.cell_h);
      for (const r of rows) expect(r.length, `${name} row ${r}`).toBe(s.cell_w);
    }
  });

  it("uses only the two grid characters", () => {
    for (const rows of Object.values(s.glyphs)) {
      for (const r of rows) expect(/^[#.]+$/.test(r)).toBe(true);
    }
  });

  it("keeps Omarchy's proportion: taller than wide, stems against a narrow counter", () => {
    expect(s.cell_h).toBeGreaterThan(s.cell_w);
  });
});

describe("Wordmark", () => {
  it("draws rects and sizes its viewBox to the word", () => {
    const { container } = render(<Wordmark />);
    const svg = container.querySelector("svg")!;
    expect(svg.getAttribute("viewBox")).toBe(`0 0 ${5 * s.cell_w + 4} ${s.cell_h}`);
    expect(svg.querySelectorAll("rect").length).toBeGreaterThan(20);
  });

  it("renders every lit cell exactly once, as runs", () => {
    const { container } = render(<Wordmark text="I" />);
    const lit = s.glyphs["I"].join("").split("").filter((c) => c === "#").length;
    const covered = [...container.querySelectorAll("rect")].reduce(
      (n, r) => n + Number(r.getAttribute("width")),
      0,
    );
    expect(covered).toBe(lit);
  });

  it("skips a letter it has no glyph for rather than throwing", () => {
    const { container } = render(<Wordmark text="KZ" />);
    expect(container.querySelector("svg")).toBeTruthy();
  });
});
