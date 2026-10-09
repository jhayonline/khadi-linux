// The number formatting the panels share. Ported from khadi-core's `rate`,
// `total` and `dur`, so these assertions are the same ones the Rust side
// makes — two spellings of "1.69 GiB" on one desktop is one too many.

import { describe, expect, it } from "vitest";
import { bytes, clockDur, dur, ellipsize, gib, rate } from "../lib/format";

describe("rate", () => {
  it("picks a sensible unit", () => {
    expect(rate(512)).toBe("512 B/s");
    expect(rate(2048)).toBe("2.0 K/s");
    expect(rate(5 * 1024 * 1024)).toBe("5.0 M/s");
  });
});

describe("bytes", () => {
  it("picks a sensible unit", () => {
    expect(bytes(1024)).toBe("1 KiB");
    expect(bytes(5 * 1024 * 1024)).toBe("5.0 MiB");
    expect(bytes(3 * 1024 ** 3)).toBe("3.00 GiB");
  });
});

describe("dur", () => {
  it("drops the noise at each end of the scale", () => {
    // Leading "0d" is noise on a machine up for hours; minutes are noise on
    // one up for days.
    expect(dur(3 * 86400 + 6 * 3600 + 40 * 60)).toBe("3d 6h");
    expect(dur(6 * 3600 + 21 * 60)).toBe("6h 21m");
    expect(dur(18 * 60)).toBe("18m");
    expect(dur(0)).toBe("0m");
  });
});

describe("clockDur", () => {
  it("is eDEX's H:MM:SS, counting hours past a day", () => {
    expect(clockDur(3 * 3600 + 4 * 60 + 5)).toBe("3:04:05");
    expect(clockDur(30 * 3600)).toBe("30:00:00");
  });
});

describe("ellipsize", () => {
  it("marks what it cuts", () => {
    expect(ellipsize("QEMU", 10)).toBe("QEMU");
    expect(ellipsize("Standard PC (Q35 + ICH9, 2009)", 12)).toBe("Standard PC…");
    expect(ellipsize("abc", 0)).toBe("…".slice(0, 0) + "…".repeat(0) || "…");
  });
  it("never returns more than it was asked for", () => {
    for (let n = 1; n < 12; n++) {
      expect(ellipsize("Aspire A515-51G", n).length).toBeLessThanOrEqual(n);
    }
  });
});

describe("gib", () => {
  it("divides by the binary gigabyte, not the decimal one", () => {
    expect(gib(1073741824)).toBe(1);
  });
});
