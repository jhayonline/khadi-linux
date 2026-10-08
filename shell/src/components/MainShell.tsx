// main_shell — the tab strip and the terminal.
//
// eDEX runs xterm.js over node-pty; this runs xterm.js over portable-pty. The
// division is the same: the webview owns the screen, Rust owns the process.

import { useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { listen } from "@tauri-apps/api/event";
import "@xterm/xterm/css/xterm.css";
import { ptyResize, ptySpawn, ptyWrite, type Theme } from "../lib/ipc";

const TAB_COUNT = 5;

export function MainShell({ theme }: { theme: Theme | null }) {
  const host = useRef<HTMLDivElement>(null);
  const [active, setActive] = useState(0);
  const [live, setLive] = useState<Set<number>>(new Set([0]));

  useEffect(() => {
    if (!host.current || !theme) return;
    const id = `shell-${active}`;
    const term = new Terminal({
      fontFamily: `"${theme.mono}", monospace`,
      fontSize: 14,
      allowTransparency: true,
      cursorBlink: true,
      // The palette is the theme's, so the shell inside matches the chrome
      // around it. khadi-theme already resolves these; nothing is re-derived.
      theme: {
        background: theme.ground,
        foreground: theme.text,
        cursor: theme.text,
        cursorAccent: theme.ground,
        selectionBackground: theme.ramp[2],
      },
      scrollback: 10000,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(host.current);
    fit.fit();

    let disposed = false;
    const unlisten: Array<() => void> = [];
    (async () => {
      try {
        const off = await listen<string>(`pty:${id}:data`, (e) => term.write(e.payload));
        if (disposed) off();
        else unlisten.push(off);
        if (!live.has(active)) setLive((s) => new Set(s).add(active));
        await ptySpawn(id, term.cols, term.rows);
      } catch (err) {
        // Visible, not swallowed. A capability the manifest forgot to grant
        // fails exactly here, and a silently rejected promise looks identical
        // to a shell that printed nothing.
        term.write(`\r\n\x1b[31mkhadi-shell: ${String(err)}\x1b[0m\r\n`);
      }
    })();

    term.onData((d) => void ptyWrite(id, d));

    const ro = new ResizeObserver(() => {
      fit.fit();
      void ptyResize(id, term.cols, term.rows);
    });
    ro.observe(host.current);

    return () => {
      disposed = true;
      ro.disconnect();
      unlisten.forEach((f) => f());
      term.dispose();
    };
    // Re-mounting per tab keeps one Terminal per pty, which is what xterm
    // expects; sharing one instance across tabs loses scrollback.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [theme, active]);

  return (
    <section className="flex h-full min-h-0 w-full flex-col overflow-hidden border-[0.18vh] border-[rgba(var(--c),0.5)]">
      <ul className="m-0 flex list-none flex-row flex-nowrap items-center justify-evenly overflow-hidden border-b-[0.18vh] border-[rgba(var(--c),0.5)] p-0">
        {Array.from({ length: TAB_COUNT }, (_, i) => (
          <li
            key={i}
            onClick={() => setActive(i)}
            className={`flex-1 cursor-pointer py-[0.5vh] text-center text-[1.4vh] tracking-[0.15vh] ${
              i === active
                ? "bg-[rgba(var(--c),0.15)] text-[rgb(var(--c))]"
                : "text-[rgba(var(--c),0.35)]"
            }`}
          >
            {i === 0 ? "MAIN SHELL" : live.has(i) ? `SHELL ${i + 1}` : "EMPTY"}
          </li>
        ))}
      </ul>
      <div ref={host} className="min-h-0 w-full flex-1 p-[0.74vh]" />
    </section>
  );
}
