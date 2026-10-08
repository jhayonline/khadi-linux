# khadi-shell

eDEX-UI's interface, rebuilt as a Tauri app: React + TypeScript + Tailwind in
the webview, Rust underneath.

## Why it exists

`khadi-hud` drew the same panels in a terminal, and the terminal was the
ceiling. A cell is about 8x20 px and indivisible, so eDEX's hairlines became
2 px lines floating in 20 px rows, its proportional display face became a
monospace, its SVG icons became block characters and its WebGL globe became
braille dots. Every one of those is a measurement, not an opinion — they are
in PLAN.md.

A webview has no cell. The numbers in `src/index.css` are eDEX's own, read out
of `edex-ui/src/assets/css`: `0.092vh` hairlines, `0.833vh` tick stubs, 17%
columns, a 65% x 60.3% main shell, 8.5vh filesystem cells. eDEX is GPL-3.0 and
so is Khadi, so this is a port rather than an imitation.

## What runs where

| | |
| --- | --- |
| `src-tauri/src/lib.rs` | one Tauri command per panel, all reading `khadi-core` |
| `src-tauri/src/pty.rs` | the terminal's process half — eDEX used node-pty |
| `src/components/` | the modules, one file per eDEX column |
| `src/assets/land.json` | eDEX's own 3937 continent tiles, from `grid.json` |

`khadi-core` did not change to support any of this. It was kept free of UI
dependencies in Phase 2 because two surfaces consumed it; it now feeds three,
and a webview was never one of the two.

## Build

Needs `nodejs`, `npm` and `webkit2gtk-4.1`.

```sh
cd shell
npm install
npm run tauri build       # or: npm run tauri dev
```

Tauri embeds the built frontend into the binary, so there is one file to
install and nothing to put beside it.

## Known gaps

- The file icons are the five structural shapes. eDEX matches ~1000 extensions
  across five icon packs; that table is a later pass.
- The globe is a 2D canvas projection of eDEX's tile data, not encom-globe's
  WebGL mesh.
- The boot screen, media player, PDF reader and fuzzy finder are not ported.
