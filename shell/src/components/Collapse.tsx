// The collapse control.
//
// NOT eDEX'S. eDEX's three panels are fixed furniture; nothing in it folds
// away, because it was one app in one window and the terminal inside it was
// never the point. Khadi's is — this is a desktop, and a desktop sometimes
// needs the whole screen for the thing you are actually doing.
//
// The first version was a bare chevron at 0.55 alpha inside the header text.
// It read as punctuation: you could not tell it was a control without already
// knowing, and the hit target was eleven pixels. This one is a bordered box
// with the same hairline every rule in the design uses, so it reads as a thing
// you press while staying inside the palette. No animation on the fold itself,
// because eDEX does not fade and a panel that takes 200ms to get out of the
// way is worse than one that does not fold at all.

type Side = "left" | "right" | "bottom";

const CHEVRON: Record<Side, [string, string]> = {
  // [shown, hidden] — the glyph always points at where the panel will go.
  left: ["◀", "▶"],
  right: ["▶", "◀"],
  bottom: ["▼", "▲"],
};

export function CollapseButton({
  side,
  collapsed,
  onClick,
}: {
  side: Side;
  collapsed: boolean;
  onClick: () => void;
}) {
  const [shown, hidden] = CHEVRON[side];
  return (
    <button
      onClick={onClick}
      title={`${collapsed ? "Show" : "Hide"} this panel  (Ctrl+Alt+${
        side === "bottom" ? "Down" : side === "left" ? "Left" : "Right"
      })`}
      className="-my-[calc(0.6vh*var(--ui-scale))] ml-[calc(0.6vh*var(--ui-scale))] flex
                 h-[calc(2.2vh*var(--ui-scale))] w-[calc(3.2vh*var(--ui-scale))]
                 cursor-pointer items-center justify-center border
                 border-[rgba(var(--c),0.45)] bg-[rgba(var(--c),0.1)] p-0
                 text-[calc(0.9vh*var(--ui-scale))] leading-none text-[rgb(var(--c))]
                 hover:border-[rgb(var(--c))] hover:bg-[rgba(var(--c),0.3)]"
    >
      {collapsed ? hidden : shown}
    </button>
  );
}

/** What a folded panel leaves behind: its name, and a way back. */
export function CollapsedRail({
  label,
  side,
  onClick,
}: {
  label: string;
  side: Side;
  onClick: () => void;
}) {
  const vertical = side !== "bottom";
  return (
    <button
      onClick={onClick}
      title={`Show ${label}`}
      className={`flex shrink-0 cursor-pointer items-center justify-center
                  gap-[calc(1vh*var(--ui-scale))] border-0 bg-[rgba(var(--c),0.06)]
                  text-[calc(1.1vh*var(--ui-scale))] tracking-[0.2vh]
                  text-[rgba(var(--c),0.75)] hover:bg-[rgba(var(--c),0.18)]
                  hover:text-[rgb(var(--c))] ${
                    vertical
                      ? `h-full w-[calc(2.8vh*var(--ui-scale))] flex-col ${
                          side === "left" ? "border-r" : "border-l"
                        } border-[rgba(var(--c),0.3)]`
                      : "h-[calc(2.8vh*var(--ui-scale))] w-full flex-row border-t border-[rgba(var(--c),0.3)]"
                  }`}
      style={vertical ? { writingMode: "vertical-rl", textOrientation: "mixed" } : undefined}
    >
      <span>{CHEVRON[side][1]}</span>
      <span>{label}</span>
    </button>
  );
}
