// The regression that shipped.
//
// eDEX's grid.json negates latitude — its y axis points down — so taken at
// face value the globe renders the world upside down, and it did, for a whole
// release. Nothing caught it: the gate only knew a window existed, and there
// were no frontend tests at all. A human looked at it and said Africa was
// wrong. These are the three lines that would have said so first.

import { describe, expect, it } from "vitest";
import land from "../assets/land.json";

const pts = land as [number, number][];

describe("globe land data", () => {
  it("has the continents the right way up", () => {
    // Earth's land is roughly two thirds northern. The inverted data had it
    // 2677 south to 1225 north, which is the giveaway.
    const north = pts.filter(([lat]) => lat > 0).length;
    const south = pts.filter(([lat]) => lat < 0).length;
    expect(north).toBeGreaterThan(south);
  });

  it("puts the Sahara north of the equator", () => {
    // A landmark either side of the line: the Sahara is land, and the box
    // mirrored across the equator from it is open South Atlantic.
    const box = (loA: number, hiA: number) =>
      pts.filter(([lat, lon]) => lat > loA && lat < hiA && lon > 0 && lon < 20).length;
    expect(box(15, 25)).toBeGreaterThan(box(-25, -15));
  });

  it("covers both hemispheres and the whole longitude range", () => {
    const lats = pts.map(([lat]) => lat);
    const lons = pts.map(([, lon]) => lon);
    expect(Math.max(...lats)).toBeGreaterThan(60); // the Arctic
    expect(Math.min(...lats)).toBeLessThan(-60); // Antarctica
    expect(Math.max(...lons) - Math.min(...lons)).toBeGreaterThan(300);
  });

  it("is eDEX's own tile set, not a resample", () => {
    expect(pts.length).toBe(3937);
    expect(pts.every(([lat, lon]) => Number.isFinite(lat) && Number.isFinite(lon))).toBe(true);
  });
});
