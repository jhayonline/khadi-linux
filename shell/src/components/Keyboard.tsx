// section#keyboard — eDEX's on-screen keyboard.
//
// NOTE: you asked for this out when the UI was a cell grid, where it would
// have been useless decoration drawn in characters. It is back because you
// then asked for an exact replica, and it is a quarter of eDEX's screen — the
// composition does not stand without it. `showKeyboard` in App.tsx turns it
// off in one line if you still want it gone.
//
// It is not decoration here: it lights up on real key events, which is what
// eDEX's does, and it is the surface that would carry Khadi's modifier layers
// if that is ever wanted.

import { useEffect, useState } from "react";

type Key = { label: string; code?: string; flex?: number };

// eDEX's en-US layout, five rows.
const ROWS: Key[][] = [
  [
    { label: "ESC", code: "Escape", flex: 1.6 },
    ...["`", "1", "2", "3", "4", "5", "6", "7", "8", "9", "0", "-", "="].map((c) => ({
      label: c,
      code: `Digit${c}`,
    })),
    { label: "BACK", code: "Backspace", flex: 2 },
  ],
  [
    { label: "TAB", code: "Tab", flex: 1.6 },
    ...["Q", "W", "E", "R", "T", "Y", "U", "I", "O", "P", "[", "]"].map((c) => ({
      label: c,
      code: `Key${c}`,
    })),
    { label: "\\", code: "Backslash", flex: 1.4 },
  ],
  [
    { label: "CAPS", code: "CapsLock", flex: 2 },
    ...["A", "S", "D", "F", "G", "H", "J", "K", "L", ";", "'"].map((c) => ({
      label: c,
      code: `Key${c}`,
    })),
    { label: "ENTER", code: "Enter", flex: 2.4 },
  ],
  [
    { label: "SHIFT", code: "ShiftLeft", flex: 2.6 },
    ...["Z", "X", "C", "V", "B", "N", "M", ",", ".", "/"].map((c) => ({
      label: c,
      code: `Key${c}`,
    })),
    { label: "SHIFT", code: "ShiftRight", flex: 2.6 },
  ],
  [
    { label: "CTRL", code: "ControlLeft", flex: 1.6 },
    { label: "SUPER", code: "MetaLeft", flex: 1.6 },
    { label: "ALT", code: "AltLeft", flex: 1.4 },
    { label: "", code: "Space", flex: 9 },
    { label: "ALT GR", code: "AltRight", flex: 1.6 },
    { label: "CTRL", code: "ControlRight", flex: 1.6 },
    { label: "←", code: "ArrowLeft" },
    { label: "↓", code: "ArrowDown" },
    { label: "→", code: "ArrowRight" },
  ],
];

export function Keyboard() {
  const [down, setDown] = useState<Set<string>>(new Set());
  useEffect(() => {
    const on = (e: KeyboardEvent) =>
      setDown((s) => {
        const n = new Set(s);
        n.add(e.code);
        return n;
      });
    const off = (e: KeyboardEvent) =>
      setDown((s) => {
        const n = new Set(s);
        n.delete(e.code);
        return n;
      });
    window.addEventListener("keydown", on);
    window.addEventListener("keyup", off);
    return () => {
      window.removeEventListener("keydown", on);
      window.removeEventListener("keyup", off);
    };
  }, []);

  return (
    <section className="flex h-full flex-col justify-evenly gap-[0.4vh] font-[var(--font-ui-light)]">
      {ROWS.map((row, i) => (
        <div key={i} className="flex min-h-0 flex-1 flex-row items-stretch gap-[0.4vh]">
          {row.map((k, j) => {
            const lit = k.code ? down.has(k.code) : false;
            return (
              <div
                key={`${i}-${j}`}
                style={{ flex: k.flex ?? 1 }}
                className={`flex items-center justify-center border-[0.12vh] text-[1.5vh] tracking-[0.1vh] transition-colors duration-75 ${
                  lit
                    ? "border-[rgb(var(--c))] bg-[rgba(var(--c),0.85)] text-[var(--ground)]"
                    : "border-[rgba(var(--c),0.25)] text-[rgba(var(--c),0.85)]"
                }`}
              >
                {k.label}
              </div>
            );
          })}
        </div>
      ))}
    </section>
  );
}
