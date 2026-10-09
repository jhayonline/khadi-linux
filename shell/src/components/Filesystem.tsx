// section#filesystem — the icon grid and the mount bar.
//
// eDEX's geometry: 8.5vh cells in an auto-fill grid with a 1vh gap, a 5vh
// icon, and the name at 1.3vh underneath. Dotfiles at 0.7 opacity, symlink
// names underlined. The space bar sits below the grid.
//
// It follows the SHELL's directory, resolved in khadi-core by looking for the
// newest interactive shell on a pty. A file list that disagrees with the
// prompt above it is worse than no file list.

import type { Filesystem as Fs } from "../lib/ipc";
import { bytes } from "../lib/format";
import { ICONS } from "./icons";
import { Title } from "./Title";

export function FilesystemPanel({ fs, action }: { fs: Fs | null; action?: React.ReactNode }) {
  const pct = fs && fs.total ? Math.round((fs.used / fs.total) * 100) : 0;
  return (
    <section className="flex h-full min-h-0 flex-col">
      <Title left="FILESYSTEM" right={fs?.cwd ?? ""} action={action} />
      <div
        className="mt-[calc(1vh*var(--ui-scale))] min-h-0 flex-1 overflow-auto"
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(auto-fill, minmax(8.5vh, 1fr))",
          gridAutoRows: "8.5vh",
          gap: "1vh",
        }}
      >
        {(fs?.entries ?? []).map((e, i) => {
          const Icon = ICONS[e.kind] ?? ICONS.other;
          return (
            <div
              key={`${e.name}-${i}`}
              className="flex h-[calc(8.5vh*var(--ui-scale))] w-[calc(8.5vh*var(--ui-scale))] flex-col items-center justify-center overflow-hidden text-center"
              style={{ opacity: e.hidden ? 0.7 : 1 }}
              title={e.name}
            >
              <div className="h-[calc(5vh*var(--ui-scale))] w-[calc(5vh*var(--ui-scale))] text-[rgb(var(--c))]">
                <Icon />
              </div>
              <h3
                className={`m-0 max-h-[30%] max-w-full overflow-hidden truncate pt-[calc(0.5vh*var(--ui-scale))] text-[calc(1.3vh*var(--ui-scale))] ${
                  e.kind === "link" ? "underline" : ""
                } ${e.kind === "dir" || e.kind === "up" ? "font-medium" : "font-normal"}`}
              >
                {e.name}
              </h3>
            </div>
          );
        })}
      </div>
      <div className="mt-[calc(0.8vh*var(--ui-scale))] flex shrink-0 items-center gap-[calc(0.8vh*var(--ui-scale))] text-[calc(1.4vh*var(--ui-scale))]">
        <h3 className="m-0 whitespace-nowrap font-normal">
          Mount <b className="font-medium">{fs?.mount ?? "/"}</b> used {pct}%
        </h3>
        <div className="h-[calc(0.7vh*var(--ui-scale))] flex-1 bg-[rgba(var(--c),0.4)]">
          <div className="h-full bg-[rgb(var(--c))]" style={{ width: `${pct}%` }} />
        </div>
        <span className="whitespace-nowrap opacity-50">
          {fs ? `${bytes(fs.used)} / ${bytes(fs.total)}` : ""}
        </span>
      </div>
    </section>
  );
}
