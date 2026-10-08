// The signature motif: a label pair over a hairline that terminates in a tick
// at each end. Every eDEX module opens with one.

export function Title({ left, right }: { left: string; right?: string }) {
  return (
    <h3 className="title rule-top">
      <span>{left}</span>
      {right !== undefined && <span>{right}</span>}
    </h3>
  );
}

/** Label over value, the unit eDEX's info grids are built from. */
export function Pair({
  k,
  v,
  className = "",
}: {
  k: string;
  v: React.ReactNode;
  className?: string;
}) {
  return (
    <div className={`pair ${className}`}>
      <h1 className="k">{k}</h1>
      <h2 className="v">{v}</h2>
    </div>
  );
}
