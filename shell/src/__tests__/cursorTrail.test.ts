// The smear's maths, without a terminal.
//
// The trail is drawn on a canvas, so what is testable is the GATING — when a
// move smears and when it does not. Those are the rules taken from kitty.conf
// and they are the ones worth pinning: get them wrong and the trail either
// never appears or smears every redraw of a TUI.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { attachCursorTrail, KITTY_DEFAULTS, type MoveFeed } from "../lib/cursorTrail";

type Rect = { x: number; y: number; w: number; h: number };

let fills = 0;
let frameCbs: FrameRequestCallback[] = [];
// A clock the test drives. The gate is "had the cursor been still for at
// least 3ms" — frames fired in a tight loop are all within the same
// millisecond, so without advancing time nothing is ever eligible to smear
// and every one of these tests passes for the wrong reason.
let clock = 0;

beforeEach(() => {
  fills = 0;
  frameCbs = [];
  clock = 1000;
  vi.stubGlobal("performance", { now: () => clock });
  vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => {
    frameCbs.push(cb);
    return frameCbs.length;
  });
  vi.stubGlobal("cancelAnimationFrame", () => {});
  vi.stubGlobal("ResizeObserver", class {
    observe() {} unobserve() {} disconnect() {}
  });
  HTMLCanvasElement.prototype.getContext = (() => ({
    clearRect() {}, beginPath() {}, moveTo() {}, lineTo() {},
    closePath() {}, fill() { fills++; }, set fillStyle(_v: string) {},
  })) as never;
});
afterEach(() => vi.unstubAllGlobals());

/** Run the animation loop `n` times against a scripted cursor. */
function run(positions: Rect[], n = 40, stepMs = 16, feed?: MoveFeed) {
  fills = 0;
  const host = document.createElement("div");
  document.body.appendChild(host);
  let i = 0;
  const detach = attachCursorTrail(
    host,
    () => positions[Math.min(i, positions.length - 1)] ?? null,
    { ...KITTY_DEFAULTS, rgb: "1,2,3" },
    feed,
  );
  for (let f = 0; f < n; f++) {
    clock += stepMs;
    const cbs = frameCbs;
    frameCbs = [];
    cbs.forEach((cb) => cb(clock));
    if (i < positions.length - 1) i++;
  }
  detach();
  host.remove();
  return fills;
}

const CELL = { w: 8, h: 16 };
const at = (cx: number, cy: number): Rect => ({ x: cx * CELL.w, y: cy * CELL.h, ...CELL });

describe("cursor trail gating", () => {
  it("does not smear a single-cell step — that is just typing", () => {
    // cursor_trail_start_threshold 2 2: one column is a keystroke.
    expect(run([at(0, 0), at(1, 0), at(2, 0)])).toBe(0);
  });

  it("smears a jump of more than the threshold", () => {
    expect(run([at(0, 0), at(40, 0)])).toBeGreaterThan(0);
  });

  it("smears a vertical jump too", () => {
    expect(run([at(0, 0), at(0, 20)])).toBeGreaterThan(0);
  });

  it("does not smear a cursor that is churning", () => {
    // `cursor_trail 3` is a gate on STILLNESS, not a duration: a TUI
    // repainting moves the cursor every frame, and smearing that would drag
    // a streak across every redraw. Stepping faster than the gate must draw
    // nothing even though the distance is well past the threshold.
    expect(run([at(0, 0), at(40, 0), at(0, 20), at(40, 20)], 40, 1)).toBe(0);
  });

  it("keeps smearing when a small step follows the jump", () => {
    // THE BUG THAT MADE THE TRAIL INVISIBLE IN PRACTICE. A one-column step
    // used to null the smear outright, and zle ends almost every jump with
    // one: ^A redraws the prompt and then places the cursor. The threshold
    // decides when a trail starts; it must not end one that is running.
    // Counted, not merely non-zero: the old code DID draw one frame before
    // the step wiped it, so "greater than zero" would have passed on the bug.
    const jumpOnly = run([at(0, 0), at(40, 0)]);
    const jumpThenStep = run([at(0, 0), at(40, 0), at(41, 0)]);
    expect(jumpOnly).toBeGreaterThan(5);
    expect(jumpThenStep).toBeGreaterThanOrEqual(jumpOnly - 2);
  });

  it("refuses a burst the frame clock cannot see", () => {
    // Frames are 16ms apart, so a 3ms gate measured BETWEEN FRAMES can never
    // refuse anything — which is why a directory listing would have smeared a
    // streak down the whole screen. Fed the terminal's own move events, the
    // gate sees a stream of writes for the burst it is.
    let notify: (() => void) | undefined;
    let subscriptions = 0;
    let unsubscribed = 0;
    const feed: MoveFeed = (cb) => {
      subscriptions++;
      notify = cb;
      return () => unsubscribed++;
    };
    const host = document.createElement("div");
    document.body.appendChild(host);
    const positions = [at(0, 0), at(40, 0), at(0, 20), at(40, 20)];
    let i = 0;
    const detach = attachCursorTrail(
      host,
      () => positions[Math.min(i, positions.length - 1)] ?? null,
      { ...KITTY_DEFAULTS, rgb: "1,2,3" },
      feed,
    );
    for (let f = 0; f < 40; f++) {
      // Two moves a millisecond apart inside every frame: a burst, under the
      // 3ms gate, even though the frames themselves are 16ms apart.
      clock += 15;
      notify?.();
      clock += 1;
      notify?.();
      const batch = frameCbs;
      frameCbs = [];
      batch.forEach((cb) => cb(clock));
      if (i < positions.length - 1) i++;
    }
    detach();
    host.remove();
    expect(fills).toBe(0);
    expect(subscriptions).toBe(1);
    expect(unsubscribed).toBe(1); // and it lets go on teardown
  });

  it("stops asking for frames once the smear has finished", () => {
    // An empty frame sixty times a second costs something and shows nothing.
    // Given the terminal's events to wake on, the loop must go quiet when
    // the cursor is settled — and must still be running while a smear is in
    // flight, which is the half that is easy to break.
    let notify: (() => void) | undefined;
    const feed: MoveFeed = (cb) => {
      notify = cb;
      return () => {};
    };
    const host = document.createElement("div");
    document.body.appendChild(host);
    let pos = at(0, 0);
    const detach = attachCursorTrail(host, () => pos, { ...KITTY_DEFAULTS, rgb: "1,2,3" }, feed);

    const pump = () => {
      const batch = frameCbs;
      frameCbs = [];
      batch.forEach((cb) => cb(clock));
      return batch.length;
    };

    clock += 16;
    pump(); // seeds `last`
    expect(frameCbs.length).toBe(0); // settled: asleep

    clock += 500;
    pos = at(40, 0);
    notify?.();
    expect(frameCbs.length).toBe(1); // the move woke it

    clock += 16;
    pump();
    expect(frameCbs.length).toBe(1); // a smear is in flight: still awake

    // Let it run to the end of the decay.
    for (let f = 0; f < 200 && frameCbs.length; f++) {
      clock += 16;
      pump();
    }
    expect(frameCbs.length).toBe(0); // caught up: asleep again
    expect(fills).toBeGreaterThan(5);

    detach();
    host.remove();
  });

  it("aligns to the terminal's screen, not the padded pane", () => {
    // The cell grid's origin is .xterm-screen. Drawing against the host put
    // the trail a character up and left of the cursor.
    const host = document.createElement("div");
    const screen = document.createElement("div");
    screen.className = "xterm-screen";
    host.appendChild(screen);
    document.body.appendChild(host);
    screen.getBoundingClientRect = () =>
      ({ left: 20, top: 12, width: 100, height: 50 }) as DOMRect;
    host.getBoundingClientRect = () =>
      ({ left: 12, top: 4, width: 116, height: 66 }) as DOMRect;
    const detach = attachCursorTrail(host, () => null, {
      ...KITTY_DEFAULTS,
      rgb: "1,2,3",
    });
    const canvas = host.querySelector("canvas") as HTMLCanvasElement;
    expect(canvas.style.left).toBe("8px");
    expect(canvas.style.top).toBe("8px");
    detach();
    host.remove();
  });

  it("draws nothing when the cursor never moves", () => {
    expect(run([at(5, 5)])).toBe(0);
  });

  it("survives a terminal that reports no geometry", () => {
    // xterm's cell dimensions come from a private field. If it ever vanishes
    // the reader returns null, and that must be a still cursor rather than a
    // crash in the render loop.
    const host = document.createElement("div");
    document.body.appendChild(host);
    const detach = attachCursorTrail(host, () => null, { ...KITTY_DEFAULTS, rgb: "1,2,3" });
    expect(() => {
      clock += 16;
      const cbs = frameCbs;
      frameCbs = [];
      cbs.forEach((cb) => cb(clock));
    }).not.toThrow();
    detach();
    host.remove();
  });
});

describe("kitty's numbers", () => {
  it("are the ones from kitty.conf, not new ones", () => {
    // If these drift, the shell inside Khadi stops moving like the shell
    // outside it, which is the whole point of the feature.
    expect(KITTY_DEFAULTS.gate).toBe(3);
    expect(KITTY_DEFAULTS.decay).toEqual([0.08, 0.3]);
    expect(KITTY_DEFAULTS.threshold).toEqual([2, 2]);
  });
});
