// main_shell — the tab strip and the terminal.
//
// eDEX runs xterm.js over node-pty; this runs xterm.js over portable-pty. The
// division is the same: the webview owns the screen, Rust owns the process.

import { useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { listen } from "@tauri-apps/api/event";
import "@xterm/xterm/css/xterm.css";
import { attachCursorTrail, KITTY_DEFAULTS } from "../lib/cursorTrail";
import {
  getWorkspaces,
  gotoWorkspace,
  ptyResize,
  ptySpawn,
  ptyWrite,
  type Theme,
  type Workspace,
} from "../lib/ipc";

const TAB_COUNT = 5;

/** Hyprland's workspaces, right of the shell tabs.
 *
 * This is all that is left of `khadi-bar`. Its clock, CPU, memory and network
 * readouts repeated the side panels — its own module doc admitted as much —
 * and its angled strip existed because a cell grid cannot skew. One strip now,
 * one process, and the workspace switching survives. */
function Workspaces() {
  const [ws, setWs] = useState<Workspace[]>([]);
  useEffect(() => {
    let alive = true;
    let t: number | undefined;
    const run = async () => {
      try {
        const v = await getWorkspaces();
        if (alive) setWs(v);
      } catch {
        /* no compositor: the strip is simply absent */
      }
      if (alive) t = window.setTimeout(run, 1500);
    };
    void run();
    return () => {
      alive = false;
      if (t !== undefined) clearTimeout(t);
    };
  }, []);

  if (ws.length === 0) return null;
  return (
    <div className="flex shrink-0 flex-row items-stretch">
      {ws.map((w) => (
        <button
          key={w.id}
          onClick={() => void gotoWorkspace(w.id)}
          title={`Workspace ${w.name}`}
          className={`cursor-pointer border-0 border-l border-[rgba(var(--c),0.25)] px-[calc(1.1vh*var(--ui-scale))] text-[calc(1.2vh*var(--ui-scale))] tracking-[0.15vh] ${
            w.active
              ? "bg-[rgba(var(--c),0.18)] text-[rgb(var(--c))]"
              : w.windows > 0
                ? "bg-transparent text-[rgba(var(--c),0.7)]"
                : "bg-transparent text-[rgba(var(--c),0.3)]"
          }`}
        >
          {w.name}
        </button>
      ))}
    </div>
  );
}

export function MainShell({ theme }: { theme: Theme | null }) {
  const host = useRef<HTMLDivElement>(null);
  const [active, setActive] = useState(0);
  const [live, setLive] = useState<Set<number>>(new Set([0]));

  useEffect(() => {
    if (!host.current || !theme) return;
    const id = `shell-${active}`;
    const css = getComputedStyle(document.documentElement);
    // The chrome scales with --ui-scale and the terminal did not, so the
    // shell text read a third smaller than every label around it.
    const uiScale = Number.parseFloat(css.getPropertyValue("--ui-scale")) || 1;
    const term = new Terminal({
      fontFamily: `"${theme.mono}", monospace`,
      fontSize: Math.round(14 * uiScale),
      allowTransparency: true,
      // Block, and no blink — kitty.conf has `cursor_shape block` and
      // `cursor_blink_interval 0`. A blinking cursor also fights the trail:
      // the smear is drawn from where the cursor WAS, and a cursor that is
      // invisible half the time leaves the smear pointing at nothing.
      cursorStyle: "block",
      cursorBlink: false,
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

    // THE TERMINAL IS ALWAYS THE KEYBOARD'S DESTINATION. Nothing called
    // focus(), so on launch the keystrokes went to <body> and the shell
    // looked dead until you happened to click inside it. eDEX has the same
    // rule and enforces it the same way: any click anywhere comes back to the
    // terminal once the thing clicked has had its event. The tab strip, the
    // workspace buttons and the filesystem are all buttons, so they still
    // work — you just do not have to click twice to resume typing.
    //
    // Safe to do unconditionally HERE and nowhere else: this window has no
    // text field. The greeter does, and it is a different window.
    term.focus();
    const refocus = () => term.focus();
    window.addEventListener("click", refocus);
    window.addEventListener("focus", refocus);

    const ro = new ResizeObserver(() => {
      fit.fit();
      void ptyResize(id, term.cols, term.rows);
    });
    ro.observe(host.current);

    // The smear. xterm.js has no cursor trail of its own, so it is drawn on
    // an overlay canvas from the cursor's cell position — see cursorTrail.ts
    // for why the numbers are kitty's rather than new ones.
    const detachTrail = attachCursorTrail(
      host.current,
      () => {
        // xterm does not expose cell geometry publicly; this is the same
        // internal the fit addon reads, and it is guarded because a private
        // field is a thing that can vanish in a minor release. No geometry
        // means no trail, not a crash.
        const dims = (term as unknown as {
          _core?: { _renderService?: { dimensions?: { css?: { cell?: { width: number; height: number } } } } };
        })._core?._renderService?.dimensions?.css?.cell;
        if (!dims?.width || !dims?.height) return null;
        const b = term.buffer.active;
        return {
          x: b.cursorX * dims.width,
          y: b.cursorY * dims.height,
          w: dims.width,
          h: dims.height,
        };
      },
      { ...KITTY_DEFAULTS, rgb: css.getPropertyValue("--c").trim() || "170,207,209" },
      // The terminal's own cursor-move event, so the stillness gate measures
      // real gaps between moves rather than the gaps between frames.
      (cb) => {
        const d = term.onCursorMove(cb);
        return () => d.dispose();
      },
    );

    return () => {
      disposed = true;
      ro.disconnect();
      window.removeEventListener("click", refocus);
      window.removeEventListener("focus", refocus);
      unlisten.forEach((f) => f());
      detachTrail();
      term.dispose();
    };
    // Re-mounting per tab keeps one Terminal per pty, which is what xterm
    // expects; sharing one instance across tabs loses scrollback.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [theme, active]);

  return (
    <section className="flex h-full min-h-0 w-full flex-col overflow-hidden border-[0.18vh] border-[rgba(var(--c),0.5)]">
      <div className="flex shrink-0 flex-row items-stretch overflow-hidden border-b-[0.18vh] border-[rgba(var(--c),0.5)]">
        <ul className="m-0 flex min-w-0 flex-1 list-none flex-row flex-nowrap items-stretch p-0">
          {Array.from({ length: TAB_COUNT }, (_, i) => (
            <li
              key={i}
              onClick={() => setActive(i)}
              className={`tab flex flex-1 cursor-pointer items-center justify-center py-[calc(0.5vh*var(--ui-scale))] text-[calc(1.4vh*var(--ui-scale))] tracking-[0.15vh] ${
                i === active
                  ? "bg-[rgba(var(--c),0.15)] text-[rgb(var(--c))]"
                  : "text-[rgba(var(--c),0.35)]"
              }`}
            >
              <span>{i === 0 ? "MAIN SHELL" : live.has(i) ? `SHELL ${i + 1}` : "EMPTY"}</span>
            </li>
          ))}
        </ul>
        <Workspaces />
      </div>
      <div ref={host} className="min-h-0 w-full flex-1 p-[calc(0.74vh*var(--ui-scale))]" />
    </section>
  );
}
