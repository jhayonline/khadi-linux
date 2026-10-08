// The collapse rail.
//
// NOT eDEX'S. eDEX's three panels are fixed furniture; nothing in it folds
// away, because it was one app in one window and the terminal inside it was
// never the point. Khadi's is — this is a desktop, and a desktop sometimes
// needs the whole screen for the thing you are actually doing.
//
// So the affordance is new, and it is kept as quiet as the design allows: a
// chevron in the header you already read, and when a panel is folded, a rail
// one and a half rows wide carrying its name sideways. No animation, because
// eDEX does not fade and a panel that takes 200ms to get out of the way is
// worse than one that does not fold at all.

type Side = "left" | "right" | "bottom";

const CHEVRON: Record<Side, [string, string]> = {
  // [shown, hidden] — the glyph always points at where the panel will go.
  left: ["‹", "›"],
  right: ["›", "‹"],
  bottom: ["∨", "∧"],
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
      title={collapsed ? "Expand" : "Collapse"}
      // A 1.3vh glyph is an eleven-pixel target. The chevron stays that size
      // because the header is that size, but the BUTTON is 2.4vh square, which
      // is a thing you can actually hit without aiming.
      className="-my-[0.5vh] flex h-[2.4vh] w-[2.4vh] cursor-pointer items-center justify-center border-0 bg-transparent p-0 text-[1.3vh] leading-none text-[rgba(var(--c),0.55)] hover:bg-[rgba(var(--c),0.12)] hover:text-[rgb(var(--c))]"
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
      // A rail is the only thing left of a panel, so it has to read as an
      // edge rather than as a smudge: a hairline on the side it folded from,
      // and the same 0.3 alpha every other rule in the design uses.
      className={`flex shrink-0 cursor-pointer items-center justify-center gap-[1vh] border-0 bg-transparent text-[1.1vh] tracking-[0.2vh] text-[rgba(var(--c),0.6)] hover:bg-[rgba(var(--c),0.08)] hover:text-[rgb(var(--c))] ${
        vertical
          ? `h-full w-[2.6vh] flex-col ${side === "left" ? "border-r" : "border-l"} border-[rgba(var(--c),0.3)]`
          : "h-[2.6vh] w-full flex-row border-t border-[rgba(var(--c),0.3)]"
      }`}
      style={vertical ? { writingMode: "vertical-rl", textOrientation: "mixed" } : undefined}
    >
      <span>{CHEVRON[side][1]}</span>
      <span>{label}</span>
    </button>
  );
}
