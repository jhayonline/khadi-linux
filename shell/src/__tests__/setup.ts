// jsdom has no ResizeObserver, and TOP PROCESSES uses one to work out how many
// whole rows fit. The stub never fires, so the component falls back to its
// default row count — which is the behaviour worth testing anyway: the panel
// must draw something before it has been measured.
class NoopResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}
globalThis.ResizeObserver ??= NoopResizeObserver as unknown as typeof ResizeObserver;

// Canvas is a no-op here. The panels that draw on one (CPU graph, globe,
// traffic) are asserted on their TEXT, not their pixels; a 2D context that
// returns null must not take the panel down with it.
HTMLCanvasElement.prototype.getContext = (() => null) as never;
