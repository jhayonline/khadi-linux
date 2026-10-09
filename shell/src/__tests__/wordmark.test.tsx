// The wordmark has one definition and three consumers — the splash PNG, this
// SVG, and logo.txt. These pin the shape of the shared file and that the SVG
// draws every lit cell from it.

import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import spec from "../assets/wordmark.json";
import { Wordmark } from "../components/Wordmark";

type Spec = { w: number; h: number; rows: string[] };
const s = spec as Spec;

describe("wordmark.json", () => {
  it("is a rectangular grid at the declared size", () => {
    // khadi-theme rasterises this with a flat loop and no bounds checking, so
    // a ragged row here is a corrupt PNG on the boot screen.
    expect(s.rows.length).toBe(s.h);
    for (const r of s.rows) expect(r.length).toBe(s.w);
  });

  it("uses only the two grid characters", () => {
    for (const r of s.rows) expect(/^[#.]+$/.test(r)).toBe(true);
  });

  it("is trimmed to its own ink", () => {
    expect(s.rows[0]).toContain("#");
    expect(s.rows[s.h - 1]).toContain("#");
    expect(s.rows.some((r) => r[0] === "#")).toBe(true);
    expect(s.rows.some((r) => r[s.w - 1] === "#")).toBe(true);
  });

  it("keeps the proportions of the text it was made from", () => {
    // A terminal cell is about 1:2. Mapping each block character to a square
    // 2x2 made the mark twice as wide as the thing people see in a terminal;
    // it is 2 wide by 4 tall now, which lands near 3:1.
    const ratio = s.w / s.h;
    expect(ratio).toBeGreaterThan(2.5);
    expect(ratio).toBeLessThan(4);
  });
});

describe("Wordmark", () => {
  it("sizes its viewBox to the grid", () => {
    const { container } = render(<Wordmark />);
    expect(container.querySelector("svg")!.getAttribute("viewBox")).toBe(`0 0 ${s.w} ${s.h}`);
  });

  it("covers every lit cell exactly once", () => {
    const { container } = render(<Wordmark />);
    const lit = s.rows.join("").split("").filter((c) => c === "#").length;
    const covered = [...container.querySelectorAll("rect")].reduce(
      (n, r) => n + Number(r.getAttribute("width")),
      0,
    );
    expect(covered).toBe(lit);
  });

  it("emits runs, not one rect per cell", () => {
    const { container } = render(<Wordmark />);
    const lit = s.rows.join("").split("").filter((c) => c === "#").length;
    expect(container.querySelectorAll("rect").length).toBeLessThan(lit / 3);
  });
});
