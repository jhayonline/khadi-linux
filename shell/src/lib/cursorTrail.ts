// The smear.
//
// kitty draws this on the GPU as `cursor_trail`; xterm.js has no such thing,
// so it is drawn here on an overlay canvas above the terminal. The numbers
// are the ones from ~/.config/kitty/kitty.conf rather than new ones, so the
// shell inside Khadi moves the way the shell outside it does:
//
//   cursor_trail 3                   a millisecond GATE, not a duration — a
//                                    trail is only drawn for a cursor that had
//                                    been still for at least this long, which
//                                    keeps trails off a TUI repainting itself
//   cursor_trail_decay 0.08 0.3      head and tail decay, seconds
//   cursor_trail_start_threshold 2 2 minimum cells moved before trailing, so
//                                    plain typing and single-line steps stay
//                                    still
//
// The shape is kitty's: a quadrilateral spanning the two corner pairs that
// the old and new cursor rectangles do NOT share, which reads as the cursor
// stretching rather than as a second cursor chasing the first.

export type TrailOptions = {
  /** ms the cursor must have been still before a move trails at all. */
  gate: number;
  /** [head, tail] decay in seconds. */
  decay: [number, number];
  /** [x, y] minimum cells moved before a trail is drawn. */
  threshold: [number, number];
  /** `r, g, b` for the fill — the theme's text colour. */
  rgb: string;
};

export const KITTY_DEFAULTS: Omit<TrailOptions, "rgb"> = {
  gate: 3,
  decay: [0.08, 0.3],
  threshold: [2, 2],
};

type Rect = { x: number; y: number; w: number; h: number };

/** Subscribe to the terminal's own cursor-move events. See `attachCursorTrail`. */
export type MoveFeed = (cb: () => void) => () => void;

/** Attach a trail to a terminal. Returns a teardown.
 *
 * `onCursorMove`, when given, is the terminal's real cursor-move event. It is
 * what makes the gate mean anything: sampling the cursor once per frame can
 * only ever see gaps of a frame or more, so a 3ms gate measured that way never
 * refuses, and a directory listing smears a streak down the whole screen. Fed
 * the actual events, the gate sees a burst of writes as the burst it is. */
export function attachCursorTrail(
  host: HTMLElement,
  readCursor: () => Rect | null,
  opts: TrailOptions,
  onCursorMove?: MoveFeed,
): () => void {
  const canvas = document.createElement("canvas");
  canvas.style.cssText = "position:absolute;pointer-events:none;z-index:5";
  host.style.position = host.style.position || "relative";
  host.appendChild(canvas);
  const ctx = canvas.getContext("2d");
  if (!ctx) return () => canvas.remove();

  let from: Rect | null = null; // where the smear is stretching from
  let to: Rect | null = null; // the cursor now
  let last: Rect | null = null; // last position seen
  // The gap the cursor sat still for before its most recent move. Infinity
  // until it has moved twice, so the first move is always eligible.
  let gap = Number.POSITIVE_INFINITY;
  let movedAt = performance.now();
  let alive = true;
  let raf = 0;
  let running = false;

  // The cursor's coordinates come from the terminal's cell grid, whose origin
  // is the screen element — NOT the host, which is inset by the pane's
  // padding. Aligning to the host instead put the whole trail about a
  // character up and to the left of the cursor it was supposed to be leaving.
  const originOf = () =>
    (host.querySelector(".xterm-screen") as HTMLElement | null) ?? host;

  const resize = () => {
    const el = originOf();
    const hr = host.getBoundingClientRect();
    const r = el.getBoundingClientRect();
    canvas.style.left = `${r.left - hr.left}px`;
    canvas.style.top = `${r.top - hr.top}px`;
    canvas.style.width = `${r.width}px`;
    canvas.style.height = `${r.height}px`;
    canvas.width = Math.max(1, Math.round(r.width * devicePixelRatio));
    canvas.height = Math.max(1, Math.round(r.height * devicePixelRatio));
  };
  resize();
  const ro = new ResizeObserver(resize);
  ro.observe(host);
  const originEl = originOf();
  if (originEl !== host) ro.observe(originEl);

  const offFeed = onCursorMove?.(() => {
    const t = performance.now();
    gap = t - movedAt;
    movedAt = t;
    start();
  });

  const step = () => {
    const now = readCursor();
    if (!now) return;

    if (!last) {
      last = now;
      return;
    }

    if (now.x !== last.x || now.y !== last.y) {
      // Without the terminal's own events, fall back to frame sampling. It
      // cannot see a burst, but it is still right about a cursor that has
      // genuinely been parked.
      const stillFor = onCursorMove ? gap : performance.now() - movedAt;
      const far =
        Math.abs(now.x - last.x) / Math.max(1, now.w) >= opts.threshold[0] ||
        Math.abs(now.y - last.y) / Math.max(1, now.h) >= opts.threshold[1];
      // The gate. A cursor that has been churning — a TUI redrawing — does not
      // smear; one that was parked and then jumped does.
      if (far && stillFor >= opts.gate) from = from ?? last;
      // THE HEAD ALWAYS FOLLOWS THE CURSOR, including for a step too small to
      // have started a trail. It used to null `from` on any such step, which
      // meant a jump that zle followed up with a one-column correction — every
      // ^A, every history recall, because the prompt is redrawn and then the
      // cursor placed — erased its own smear before a single frame of it was
      // drawn. The threshold decides when a trail STARTS. It has no business
      // ending one.
      to = now;
      last = now;
      movedAt = performance.now();
    }

    const d = devicePixelRatio;
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    if (!from || !to) return;

    // Ease the tail toward the head. Two rates, so the leading edge catches up
    // first and the trailing edge lags — which is what makes it a smear and
    // not a slide.
    const head = 1 - Math.exp(-(1 / 60) / opts.decay[0]);
    const tail = 1 - Math.exp(-(1 / 60) / opts.decay[1]);
    from = {
      x: from.x + (to.x - from.x) * tail,
      y: from.y + (to.y - from.y) * tail,
      w: from.w + (to.w - from.w) * head,
      h: from.h + (to.h - from.h) * head,
    };

    const dx = Math.abs(from.x - to.x);
    const dy = Math.abs(from.y - to.y);
    if (dx < 0.5 && dy < 0.5) {
      from = null; // caught up
      return;
    }

    ctx.fillStyle = `rgba(${opts.rgb}, 0.55)`;
    ctx.beginPath();
    // The two rectangles, hulled. Taking the outer corners of each in the
    // direction of travel gives the stretched quad; the naive version drew a
    // bounding box and read as a growing rectangle.
    const a = from;
    const b = to;
    const pts: [number, number][] =
      (b.x - a.x) * (b.y - a.y) >= 0
        ? [
            [a.x, a.y], [a.x + a.w, a.y], [b.x + b.w, b.y], [b.x + b.w, b.y + b.h],
            [b.x, b.y + b.h], [a.x, a.y + a.h],
          ]
        : [
            [a.x + a.w, a.y], [b.x + b.w, b.y], [b.x + b.w, b.y + b.h],
            [b.x, b.y + b.h], [a.x, a.y + a.h], [a.x, a.y],
          ];
    pts.forEach(([x, y], i) =>
      i === 0 ? ctx.moveTo(x * d, y * d) : ctx.lineTo(x * d, y * d),
    );
    ctx.closePath();
    ctx.fill();
  };

  // A LOOP THAT SLEEPS. Driven by the terminal's own events there is nothing
  // to do between a settled cursor and the next keystroke, and an empty frame
  // sixty times a second on a machine that already felt slow is a cost with
  // nothing to show for it. The decision is made AFTER the step, because the
  // frame that starts a smear is the one that would otherwise have been
  // judged idle and stopped the animation before its first frame.
  const frame = () => {
    if (!alive) return;
    running = false;
    step();
    if (from || !onCursorMove) start();
  };
  const start = () => {
    if (running || !alive) return;
    running = true;
    raf = requestAnimationFrame(frame);
  };
  start();

  return () => {
    alive = false;
    running = false;
    cancelAnimationFrame(raf);
    ro.disconnect();
    offFeed?.();
    canvas.remove();
  };
}
