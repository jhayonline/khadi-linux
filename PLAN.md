# Khadi — Build Plan

A keyboard-driven, terminal-first Arch distribution with an eDEX-UI aesthetic.

Status: Phase 3b started — khadi-bar draws the motifs a terminal cannot.
Last updated 2026-10-06.

---

## 1. What we're building

Khadi is Arch Linux plus an opinionated, keyboard-driven desktop whose aesthetic
descends from [eDEX-UI](https://github.com/GitSquared/edex-ui) — assembled from
maintained tools rather than by reviving an archived Electron app.

The product has three tiers, and the split is the whole plan:

- **Chassis — composed.** Compositor, terminal, panes, system monitor, file
  browser and editor are existing projects with their own communities. Khadi
  owns configuration and theme, not applications.
- **Hero — original.** One component is written from scratch: `khadi-hud`, the
  status and dashboard layer carrying the eDEX identity. It is the
  differentiator and the only deep engineering commitment.
- **Kiosk — optional.** The [Tauri eDEX rewrite](https://github.com/zluo01/edex-ui)
  ships as an optional package for anyone wanting the full single-app
  experience. No architectural commitment, high demo value.

The reason for this shape is blast radius. On rolling-release Arch, a bug in a
composed component makes one pane ugly while the system keeps working. A bug in
a single-app shell takes the entire product down, in code nobody here wrote.

The success condition is not a screenshot. It is a system someone installs on a
primary machine and still uses six months later.

### Why not the alternatives

| Strategy | Verdict |
| --- | --- |
| **A.** Ship an eDEX fork as a fullscreen kiosk shell | Rejected. The product becomes one app: a webkit2gtk bump breaks everything, and a single fullscreen kiosk hits a ceiling the first time a user wants a browser. |
| **B.** Compose maintained tools under a unified theme | **Chosen.** Outsources terminal emulation, sensors and file browsing to upstreams. Proven shape — it is what Omarchy does. |
| **C.** Write the whole shell from scratch | Rejected for v1. A ratatui app runs *inside* a terminal, so it can never be the thing containing terminals. Writing a terminal emulator is a multi-year project. Kept as the hero component only. |

---

## 2. Architecture

Khadi is four layers, and only one of them is Khadi's to maintain.

```
┌─ Khadi ─ written and maintained here ─────────────────────────────────┐
│   khadi-hud          khadi-theme         khadi-config                 │
└───────────────────────────────────────────────────────────────────────┘
┌─ Workspace ─ inside the terminal, where the work happens ─────────────┐
│   Zellij      btop          Yazi            Helix                     │
└───────────────────────────────────────────────────────────────────────┘
┌─ Session ─ compositor and surfaces ───────────────────────────────────┐
│   Hyprland    Ghostty       greetd+tuigreet fuzzel+mako               │
└───────────────────────────────────────────────────────────────────────┘
┌─ Base ─ upstream Arch, never forked ──────────────────────────────────┐
│   pacman      core·extra·multilib          [khadi] repo  ← owned      │
└───────────────────────────────────────────────────────────────────────┘
```

Every box outside the Khadi layer has its own maintainers, release cadence and
bug tracker. That is the entire point: the surface area Khadi owns stays small
enough for one person to carry.

### Component selection

| Slot | Choice | Why | Alternate |
| --- | --- | --- | --- |
| Compositor | **Hyprland** | Proven, deep ecosystem, and its model suits a single very-wide terminal | — locked |
| Login | greetd + tuigreet | Text-mode greeter matches the aesthetic; keyboard-only | regreet |
| Terminal | **foot** | CPU-rendered (no GPU dependency), 812 KiB, draws box glyphs itself with tunable stroke weight | — locked |
| Panes | Zellij | KDL layout files make the panelled look declarative and shippable | tmux |
| System monitor | btop | Themeable via `.theme` files, broad sensor coverage | `khadi-hud dash`, from Phase 3a |
| Files | Yazi | Rust, fast, TOML theming, image preview | lf |
| Editor | Helix | Modal and usable with zero config | Neovim |
| Launcher | fuzzel | Wayland-native, tiny | walker |
| Notifications | mako | Minimal, themeable from the same source | swaync |
| Bar | Waybar | Placeholder only — replaced by `khadi-hud panel` in Phase 3b | — |
| Network | impala + bandwhich | TUI wifi picker, per-process bandwidth | nmtui |

Nothing in that table is novel, and that is deliberate. Novelty costs
maintenance, and it is spent in exactly one place.

---

## 3. The design system

Extracted from the eDEX source on 2026-10-05. Lives in `themes/tron.toml`.

The reference turned out to be far smaller and more portable than expected. The
valuable part of the repo is not the JavaScript — it is ~2,300 lines of CSS and
a theme schema of six values.

### A theme is six values

All 21 upstream themes share one shape: an accent `r/g/b` triple plus `black`,
`light_black` and `grey`. ANSI 16 is optional — only 9 of 21 define it, and
`tron` is not one of them. eDEX is a **one-colour design**: a single accent
paints every glyph and every rule, and all depth comes from alpha.

For `tron`: accent `#aacfd1`, ground `#05080d`, grid `#262828`.

### The three motifs

1. **Hairline rules** — `0.092vh solid rgba(accent, 0.3)`. 35 occurrences, the
   dominant border in the codebase. The `0.3` alpha is the single most
   important number in the whole aesthetic.
2. **Bracket ticks** — every horizontal rule terminates in a short vertical
   stroke at each end, via `::before` / `::after`. Present in 10 of 21
   stylesheets. This is the signature, and the hardest thing to reproduce
   outside CSS.
3. **Label pairs** — title flush left, value flush right, hairline beneath,
   `letter-spacing: 0.092vh`. One letter-spacing value in the entire codebase,
   used 8 times.

Everything is sized in `vh`, which is why the UI holds together at any
resolution.

### The alpha-to-hex translation

eDEX gets its depth from `rgba(accent, N)` over the background. Terminals and
TUIs cannot do alpha, so `themes/tron.toml` precomputes the ramp as solid hex
against `background`. This is the most important single step in the port —
without it every TUI rule renders at full accent and the design reads flat and
shouty rather than layered.

| alpha | flattened | role |
| --- | --- | --- |
| 0.3 | `#364448` | every hairline and every tick |
| 0.5 | `#586c6f` | units, timestamps, inactive text |
| 0.8 | `#89a7aa` | secondary values in label pairs |
| 1.0 | `#aacfd1` | primary text |

### Fonts

eDEX exposes exactly two font slots, identical across all 21 themes:
`--font_main` (United Sans Medium, 5 rules) and `--font_main_light` (United
Sans Light, 9 rules).

United Sans is a **commercial House Industries typeface**, shipped inside the
eDEX repo as `.woff2`. That is fine for a GPL source tree on GitHub and not
fine on installable media. Because it is only two variables, the substitution
is cheap — but it must happen before any template is written.

| Role | Locked choice | Licence | Packaging |
| --- | --- | --- | --- |
| UI labels, headings | **Rajdhani** (Light 300 + Medium 500) | OFL | vendored in `khadi-fonts` |
| Display — the clock | **Orbitron** | OFL | vendored in `khadi-fonts` |
| Terminal, mono | **Iosevka Term** | OFL | vendored in `khadi-fonts` |

Locked 2026-10-05. Reasoning:

- **Rajdhani** ships Light and Medium as designed weights, mapping 1:1 onto
  eDEX's two UI slots, and stays legible at the 1.02–1.48vh label sizes. Chakra
  Petch is closer to United Sans in width and its chamfered corners echo the
  chrome, but it mushes at small label sizes. The chrome carries the identity,
  so the typeface should be quiet.
- **Iosevka Term**, measured against the installed font: **0.500 em advance
  versus Fira Code's 0.600 — 20% more columns at the same point size.** That is
  precisely the resource the motif spike showed was scarce. The `Term` variant
  sizes symbols to the cell; plain `Iosevka` lets ligatures overhang and
  `Fixed` drops them entirely. Note also that eDEX's terminal slot is
  `"Fira Mono"`, not Fira Code — so the fidelity argument for Fira Code was
  always weak, and Fira Mono has no ligatures anyway.

**Packaging correction.** An earlier draft had Iosevka as a plain
`extra/ttc-iosevka` dependency "needing no vendoring". That was true and naive:
the package installs **446 MiB and exposes 189 faces** — roughly 40% of a base
Arch install, for one font. We need two faces.

| Option | Size | Faces |
| --- | --- | --- |
| `extra/ttc-iosevka` | 446.3 MiB | 189 |
| `extra/ttf-iosevka-nerd` | 1141.5 MiB | — |
| **Regular + Bold subset, vendored** | **20.7 MiB** | 2 |
| `extra/ttf-fira-code` (rejected font) | 1.7 MiB | 4 |

So all three families ship inside `khadi-fonts` and none is a repo dependency.
Subsetting further by codepoint — Latin, box drawing, symbols — should reach
low single-digit MiB; logged as a Phase 4 packaging task. Nothing touches the
AUR either way.

### One-colour has a cost

Under a pure one-colour theme there is no palette for syntax highlighting in
Helix or file-type colour in Yazi. Themes that need it must define
`[terminal.ansi]`. Upstream is inconsistent here — nord uses `light*`, the
other eight use `bright*`; Khadi normalises to `bright*`.

---

## 4. The theme engine

One aesthetic must land consistently across a dozen applications sharing no
config format: TOML, KDL, INI, CSS, Lua and bespoke `.theme` files.

**Single source of truth.** One `themes/<name>.toml` per theme. A generator
renders each app's config from a template. This is the pywal / base16 pattern,
deliberately not a new idea.

**Build on [tinted-theming](https://github.com/tinted-theming), don't
reinvent.** The base16 ecosystem already ships templates for most of the stack.
Khadi writes templates only for what it doesn't cover.

**Colour is only about 60% of it.** The rest is chrome — frames, rules, letter
spacing, padding — and lives in Zellij frame config, Hyprland borders and gaps,
and `khadi-hud`'s own rendering.

---

## 5. khadi-hud — the hero component

`khadi-hud` is the one thing Khadi writes from scratch, and the answer to "how
is this not Omarchy with a different theme?"

Everything else in the chassis can be reproduced by anyone willing to edit
config files. An original status and dashboard layer cannot. It is also bounded
work — reading metrics and drawing them — not a terminal emulator.

| Surface | What it is | Replaces | Phase |
| --- | --- | --- | --- |
| `khadi-hud panel` | Wayland layer-shell bar with angled eDEX chrome: clock, CPU bars, memory grid, network rates | Waybar | 3b |
| `khadi-hud dash` | Fullscreen ratatui dashboard summoned into any pane: CPU history, process table, disk, sensors | btop, as a styled alternative | 3a |

Both read the same theme file as every other app, so they cannot drift from the
system palette.

**Technical choices**

- **Rust.** `sysinfo` for metrics, `ratatui` for the TUI surface. The
  [ratatui-sci-fi](https://docs.rs/ratatui-sci-fi/latest/ratatui_sci_fi/) crate
  (v0.2.1) is worth reading for theming patterns but is early-stage and ships
  no meters or charts — reference, not dependency.
- **Panel: `gtk4-layer-shell` + CSS.** The pragmatic path, and the one Waybar
  proved. Raw `smithay-client-toolkit` gives more control over angled geometry
  but costs weeks.
- **One shared core crate.** Metrics, theme parsing and formatting in
  `khadi-core`; the two surfaces are thin front-ends.

**Sequencing.** Ship the TUI dashboard first — far cheaper, validates the
metrics core and the visual language, usable the day it compiles. Until Phase
3, Khadi runs a heavily themed Waybar. The product is shippable without
`khadi-hud`; it just isn't yet differentiated.

---

## 6. Keyboard model

The hard problem is collision, not binding choice. Four layers compete for the
same keys: compositor, multiplexer, terminal and the application inside it.
Most rices get this wrong and it is the fastest way to feel amateur.

**The rule: each layer owns one modifier, exclusively.**

| Layer | Modifier | Owns |
| --- | --- | --- |
| Hyprland | `Super` | Workspaces, window focus and movement, launcher, lock, screenshot, quit |
| Zellij | `Alt` | Pane split, pane focus, tab switch, layout swap, detach |
| Terminal | `Ctrl+Shift` | Font size, copy, paste, new window |
| Application | `Ctrl`, plus its own | Everything else, untouched |

Zellij's defaults violate this — it claims `Ctrl+p`, `Ctrl+n`, `Ctrl+o` and
more, which collide with readline, Helix and every REPL. Khadi rebinds Zellij
entirely onto `Alt` and runs it in a locked mode so applications see a clean
`Ctrl`.

**Pane-centric, not window-centric.** This is the second half of the
differentiation answer. Omarchy and most Hyprland setups are window-centric:
one app per window, `Super` to move between them. Khadi's default workspace is
a single terminal running a Zellij layout, and most work happens inside it.
Windows exist for browsers and graphical apps, not for terminals.

That choice is what makes the panelled aesthetic coherent rather than
decorative — the panels are the workflow.

**Discoverability is a product requirement.** A keyboard-driven system with
hidden bindings is a toy. Ship a `Super+/` cheatsheet overlay and keep Zellij's
status hints on by default.

---

## 7. Packaging and the khadi repo

Khadi ships as a pacman repository, not a fork of Arch. This is exactly the
[Omarchy](https://github.com/omacom/omarchy) model — its `pacman.conf` carries
`[omarchy] Server = https://pkgs.omarchy.org/stable/$arch` alongside core,
extra and multilib, and users keep the full Arch ecosystem.

| Package | Contents |
| --- | --- |
| `khadi-keyring` | Project signing key. Installed first, bootstraps trust. |
| `khadi-fonts` | The OFL font set (Rajdhani, Iosevka Term, Orbitron), subset so themes never depend on user fonts and never pull 446 MiB. |
| `khadi-theme` | Theme generator, templates, bundled themes. |
| `khadi-config` | Default configs for every chassis app. The dotfiles, versioned. |
| `khadi-hud` | The hero component. |
| `khadi` | Meta-package depending on all of the above plus the chassis. `pacman -S khadi` is the whole product. |
| `khadi-edex` | Optional kiosk shell from the Tauri rewrite. Not a dependency of `khadi`. |

**Constraints worth knowing now**

- **A repo cannot depend on the AUR.** Anything Khadi needs that lives only in
  the AUR must be rebuilt into the Khadi repo and maintained there. Audit this
  early — it is a recurring tax and it is how package counts quietly triple.
  The Tauri eDEX fork publishes no releases, so `khadi-edex` is a source build
  and lands squarely in this category.
- **Sign everything.** Packages signed with a project key, `khadi-keyring`
  distributes it, repo database signed too. Unsigned repos are a non-starter
  for anything a stranger installs.
- **Vendor nothing Arch already ships.** Every rebuilt package is one Khadi
  must now track for CVEs.
- **Hosting is a real cost.** Object storage plus CDN — S3 or Cloudflare R2
  fronted by a cache, synced with `rclone`. An ongoing bill, not a one-off.

---

## 8. Installer and ISO

Keep the ISO thin. It installs base Arch, adds the Khadi repo, and runs
`pacman -S khadi`. Everything that defines the product lives in the repo, where
it can be fixed without cutting new media. A fat ISO means every bug fix needs
a release; a thin one means the ISO almost never changes.

**The build.** [`archiso`](https://wiki.archlinux.org/title/Archiso) is the
tool. Copy the `releng` profile from `/usr/share/archiso/configs/releng/`, then
edit `packages.x86_64`, `profiledef.sh` and `airootfs/`. Build with
`mkarchiso`.

**Build inside Docker.** [`omarchy-iso`](https://github.com/omacom/omarchy-iso)
does this and it is worth copying outright: the host stops influencing output,
and CI can cut a release without an Arch machine. It vendors `archiso` as a
submodule, which pins the builder version.

**The install flow**

1. Boot the live ISO straight into a styled TTY — first impression starts here,
   not after reboot.
2. A configurator collects disk, encryption, locale, user. Front-end
   `archinstall` rather than writing a partitioner; LUKS on by default.
3. `archinstall` lays down base Arch.
4. The Khadi installer adds repo and keyring, installs the meta-package,
   enables `greetd`.
5. Reboot into the greeter.

Steps 2 and 4 are where the product identity lives. Step 3 is someone else's
solved problem and should stay that way.

**Config migrations.** Shipped defaults will change and users will have edited
them. Omarchy keeps a `migrations/` directory of versioned scripts that move an
existing install's config forward. Build this in from Phase 4, not after the
first time it breaks someone's machine.

---

## 9. Roadmap

Each phase ends at a gate. If the gate doesn't pass, the next phase is not the
answer.

| Phase | When | Deliverable | Gate |
| --- | --- | --- | --- |
| **0 · Spike** | days | Design tokens extracted; pane layout hand-built; fonts, terminal and compositor locked | **Zellij frames can carry the bracket-tick motif** — or the chrome moves into `khadi-hud` earlier than planned |
| **1 · Dotfiles** ✅ | weeks 1–3 | One config repo, scriptable onto any Arch install. No packaging, no ISO, no hosting | Someone else runs the script and gets your desktop — **passed mechanically (42 checks, clean VM); not yet by a third party** |
| **2 · Theme engine** 🔄 | weeks 3–6 | `khadi-theme` plus base16 templates. Three shipped themes to prove the pipeline generalises | One file change restyles every app in the stack — **passed**, verified live in the VM |
| **3 · khadi-hud** 🔄 | weeks 6–12 | 3a: the ratatui dashboard ✅. 3b: the layer-shell panel that retires Waybar ✅. 3c: the last two A/B items ✅ | The product no longer reads as someone else's desktop with new colours — **A/B faithful count 1 → 13 of 18; nothing in the backlog is still missing** |
| **4 · Packaging** | weeks 10–14 | Signed repo, PKGBUILDs, keyring, meta-package, migrations | `pacman -S khadi` works on a clean Arch install |
| **5 · Installer + ISO** | weeks 14–18 | archiso profile, Docker build, archinstall front-end, styled live TTY | A stranger installs from USB without asking you anything |
| **6 · Operate** | ongoing | Breakage watch, migrations, release cadence, support load, hosting bill | No gate. This is the job from here on, and the part that decides whether Khadi survives |

Phases 0–2 produce something genuinely usable by week six — a desktop you run
daily and share as a config repo. Nothing before Phase 4 needs hosting, money
or a release process, which means the expensive commitments come only after the
idea has proven itself.

The overlap between Phases 3 and 4 is deliberate: packaging can start while
`khadi-hud` is still being written, because a meta-package doesn't care whether
its dependency is finished.

Week numbers assume sustained part-time work by one person. They are
sequencing, not commitments.

---

## 10. Phase 0 — current

**Done**

- [x] Clone eDEX-UI for reference (`edex-ui/`, upstream `GitSquared/edex-ui`, archived)
- [x] Extract the design system to `themes/tron.toml` — palette, flattened alpha
      ramp, semantic roles, the three chrome motifs, type scale
- [x] **Zellij frame test — GATE FAILED.** See below.
- [x] **Motif spike** (`spike/motif.py`) — the eDEX motif survives the cell
      grid at one-row fidelity. See below.
- [x] **Fonts locked** — Rajdhani / Iosevka Term / Orbitron, all vendored.
      See section 3.
- [x] **Clock locked** — seconds dropped. `HH:MM` at 5-cell glyphs is 29 of 34
      columns, legible and closer to the reference's visual weight than the
      cramped `HH:MM:SS` it replaces.

**Next**

- [x] **Reference layout hand-built** — `config/zellij/`, `config/btop/`,
      `bin/khadi-dev`. Runs. See below.
- [x] **A/B against the reference screenshot** — 1 of 18 in-scope elements
      faithful. See below.
- [x] **Terminal locked: foot.** **Compositor locked: Hyprland.** See below.

### Gate result: Zellij cannot carry the bracket tick

**Verdict: no. Confirmed empirically against the installed zellij 0.45.1.**

Method: zellij driven in a pty at 96x24 with a two-pane layout, output parsed
back into a screen grid, box-drawing glyphs extracted per configuration.
Harness and captures are throwaway; the result is below.

| Configuration | Glyphs rendered | Count |
| --- | --- | --- |
| `pane_frame_style "full"` | `─ │ ┌ ┐ └ ┘` | 6 |
| `"full"` + `rounded_corners true` | `─ │ ╭ ╮ ╯ ╰` | 6 |
| `pane_frame_style "titles"` | `│` (pane divider only) | 1 |
| `pane_frame_style "none"` | `│` (divider persists) | 1 |
| `border_style` = single / double / heavy / dashed / heavy_dashed | **all identical to `full`** | 6 |

**The shipped vocabulary is one line weight and two corner sets.** Nothing else.

**Correction — the five line styles are not released.** `LineStyle`
(`Single ─│`, `Double ═║`, `Heavy ━┃`, `Dashed ┄┆`, `HeavyDashed ┅┇`) and
`zellij-server/src/ui/border_glyphs.rs` exist only on `main`. Neither is
present in the `v0.45.1` tag — `border_glyphs.rs` 404s there, and `data.rs`
contains no `LineStyle`. That is why all five `border_style` values rendered
identically: in 0.45.1 the key does not exist. Even once released, the enum is
closed and contains no bracket tick.
[zellij#5640](https://github.com/zellij-org/zellij/issues/5640) (open,
2026-09-21) confirms the glyphs are not exposed through the plugin API either,
so this cannot be solved with a plugin.

**Correction — `titles` mode is NOT closer to eDEX.** An earlier reading of the
source suggested it would be, since it drops the box. Rendered, it is:

```
                      PANEL                    │                    TERMINAL
```

Centred title, no horizontal rule, just a vertical divider between panes. eDEX
headers are label flush-left, value flush-right, hairline beneath. `titles`
gets the alignment wrong and omits the rule entirely.

Ironically `full` is structurally the closer of the two — `┌ PANEL ──────┐`
puts the label left with the rule running right, which is the eDEX header shape
— but it closes the box on all four sides, which eDEX never does.

**Consequence — the mental model corrects.** eDEX has no window-manager chrome
at all. Every panel draws its own frame, because every panel *is* an app. So:

- Zellij runs with `pane_frame_style "none"`. It handles layout, not looks.
  Note the `│` divider survives even then; it is colourable, not removable.
- Chrome is drawn by whatever occupies the pane.
- The bracket tick can therefore only appear where Khadi draws pixels itself:
  `khadi-hud panel` (GTK/CSS — full fidelity) and `khadi-hud dash` (ratatui —
  a two-row approximation, rule then `╷ … ╷`, costing one line).
- Third-party TUIs (btop, Yazi, Helix) get **colour fidelity but not chrome
  fidelity**. Their box drawing is fixed in the same way zellij's is.

**This promotes `khadi-hud`.** It is now the only surface that can carry the
signature motif, which makes it load-bearing for the aesthetic rather than a
Phase 3 differentiator. Candidate correction: bring `khadi-hud dash` forward
into Phase 1 and let it replace btop as the reference panel, so the hand-built
layout has at least one genuinely eDEX-looking pane from the start.

### Hand-built layout: the chassis works, the aesthetic does not come free

`bin/khadi-dev` runs it: zellij with frames off, three columns plus a bottom
strip, btop themed from `themes/tron.toml`, keybinds on `Alt` per section 6.

**What it validates.** Frames-off works. The theming pipeline works — one token
file drove the zellij theme and the btop theme and they agree. The `Alt`-only
keybind layer works. The layout structure matches eDEX. This is a real
chassis.

**What it exposes.** Three measured problems, none of them fixable with config.

**1. btop cannot occupy an eDEX side column.** Minimum widths, measured by
driving btop in a pty and reading its own refusal message:

| btop boxes | minimum |
| --- | --- |
| `cpu` | **60 x 8** |
| `proc` | 44 x 16 |
| `mem` | 36 |
| `net` | 36 |
| `cpu mem` | 60 x 18 |
| all four | 80 x 24 |

eDEX's side columns are ~17% of screen width. At any sane terminal size that
is well under 60 columns, so **btop's CPU box can never live in an eDEX side
panel.** The CPU graph is the most prominent element of eDEX's left column and
it has no off-the-shelf occupant. The layout currently carries `mem net` left
and `proc` right because that is all that fits.

(zellij's `│` divider consumes one column, so pane `size=` must be
minimum + 2.)

**2. The proportions invert.** The side columns are fixed at 38 + 46 = 84
columns by those minimums, while eDEX's are a percentage:

| Terminal width | Sides | Shell |
| --- | --- | --- |
| 120 | 70% | 30% |
| 150 | 56% | 44% |
| 180 | 47% | 53% |
| 200 | 42% | 58% |
| **247** | **34%** | **66%** |

eDEX's ~34% / ~56% split is **first reached at 247 columns**. Below that the
side panels dominate the screen — the opposite of the reference, where the
terminal is the hero. On a 13-inch laptop this layout is unusable.

**3. btop's chrome is loud and nothing like eDEX.** Rendered, the left column
reads `┌─┐²mem┌──────────┬─┐disks┌────┐io┌─┐` — nested boxes, inline widget
labels, `■■■■` meters. Correct colours, completely different design language.
This is the "colour fidelity, not chrome fidelity" prediction made visible.

**Conclusion: composition yields a themed TUI desktop, not eDEX.** That is not
a failure of the strategy — the chassis is exactly what Phase 1 needs to
deliver, and it works. But it settles the remaining question about
`khadi-hud`'s scope: the side panels are not "btop with better colours", they
are `khadi-hud` or they are nothing. Specifically `khadi-hud dash` must own
the CPU meter, the clock, and the label-pair header, because no installed tool
can render any of them at eDEX's proportions.

**Revised view of the roadmap.** Phase 1 ships this chassis and should be
honest that it looks like a themed Omarchy. Phase 3 is where Khadi starts
looking like Khadi, and its scope is now specified by what this layout cannot
do rather than by guesswork.

### A/B against screenshot_default.png

Element-by-element against the reference, at 200x50 — a fair width, since the
proportions only work above ~180 columns.

| # | Reference element | Khadi hand-build | Status |
| --- | --- | --- | --- |
| 1 | `PANEL / SYSTEM` header, hairline + ticks | btop's own box border | missing |
| 2 | Clock `20:27:46`, ~10vh | — | missing |
| 3 | Date / uptime / type / power grid | — | missing |
| 4 | Manufacturer / model / chassis | — | missing |
| 5 | `CPU USAGE` + dual line graphs + Avg | — (btop cpu needs 60 cols) | missing |
| 6 | Temp / min / max / tasks | — | missing |
| 7 | `MEMORY` + dot-matrix grid | btop mem box | approximated |
| 8 | `SWAP` bar | btop swap meter | approximated |
| 9 | `TOP PROCESSES` + PID/NAME/CPU/MEM | btop proc, but in the *right* column | approximated, displaced |
| 10 | `TERMINAL / MAIN SHELL` header | — | missing |
| 11 | Angled tab bar (`skewX(35deg)`) | — | **impossible in a cell grid** |
| 12 | Terminal content | shell | **faithful** |
| 13 | Prompt pill, angled edge | plain prompt | missing |
| 14 | `PANEL / NETWORK` header | — | missing |
| 15 | Network status / IPv4 / ping | — | missing |
| 16 | `NETWORK TRAFFIC` dual graph | btop net box, in the *left* column | approximated, displaced |
| 17 | `FILESYSTEM` header + path | — | missing |
| 18 | Icon-grid file browser | — (yazi absent; yazi is a list anyway) | missing |
| — | World-view globe | dropped | non-goal |
| — | On-screen keyboard | dropped | non-goal |

**Score: 1 faithful, 4 approximated, 13 missing, of 18 in-scope elements.**

**Design language, dimension by dimension:**

| Dimension | Verdict |
| --- | --- |
| Colour | **Matches.** The one-colour palette and flattened ramp transfer cleanly. The theming pipeline is proven. |
| Hairline rules at 0.3 alpha | Absent. btop draws solid single-weight box borders. |
| Bracket ticks | Absent, as the gate predicted. |
| Label pairs (left/right) | Absent. btop centres labels *inside* the border run. |
| ALL CAPS, letter-spaced | Absent. |
| Thin strokes, negative space | Inverted. btop is dense with nested boxes and `■■■■` meters. |
| Angled tabs | Impossible — a cell grid cannot skew. |

**Verdict: colour transfers, structure approximates, design language does not
transfer at all.** The hand-build reads as a competently themed TUI desktop. It
does not read as eDEX, and no amount of further configuration will change that.

**Fourth motif found during this pass.** The shell tab bar skews each tab into
a parallelogram — `transform: skewX(35deg)`, with `skewX(-35deg)` on the label
to keep the text upright, and `scale(1.2)` on the active tab. The first token
extraction missed it. Now in `themes/tron.toml` under `[chrome]`. Like the
two-row tick, it is GTK/CSS only.

**What this is actually for.** The A/B is not a verdict on strategy B — it is
`khadi-hud`'s specification. The 13 missing elements are its backlog, and they
are now enumerated rather than guessed at. Items 1–6 and 10 and 13–18 are
exactly what Phase 3 has to build, and items 11 and 2 tell us which parts must
land on the GTK panel rather than the ratatui surface.

### Terminal and compositor locked

**Terminal: foot.** Both are in `extra`, so packaging does not decide it.

| | foot | Ghostty |
| --- | --- | --- |
| Installed size | **812 KiB** | 29.4 MiB |
| Rendering | CPU | GPU |
| Ligatures | never (architectural) | yes |
| Box-drawing glyphs | **drawn by foot, stroke weight tunable** | from the font |

Three reasons, in order of weight:

1. **No GPU dependency.** PLAN.md already lists NVIDIA on Wayland as the top
   support-load risk. In a terminal-first distro the terminal *is* the product,
   so it must be the most reliable component on the system. With foot, a broken
   GPU stack still leaves a working desktop.
2. **foot draws box-drawing characters itself** rather than taking them from
   the font, and exposes `tweak.box-drawing-base-thickness` (default 0.04).
   That is the only hairline-weight control any terminal gives us — the nearest
   thing to eDEX's `0.092vh` rule, which is the motif the whole design rests
   on. Set to 0.03 in `config/foot/foot.ini`, to retune against a real display.
3. **Ghostty's distinguishing features are ones Khadi does not want.** Tabs and
   splits belong to zellij; the keyboard model gives the terminal only
   `Ctrl+Shift`. Khadi needs a fast correct grid that gets out of the way.

Cost: no ligatures, ever — foot has
[decided against them](https://codeberg.org/dnkl/foot/issues/57) and its own
shipped changelog says so plainly. Acceptable, because eDEX's terminal slot was
`"Fira Mono"`, which has none either. We lose a nice-to-have, not fidelity.

**Compositor: Hyprland.** The A/B settled this by accident. The layout needs
180–247 columns to hold eDEX's proportions, which is a *window width*
requirement.

- **niri** is scrollable tiling: windows live in an infinite horizontal strip,
  each normally a fraction of screen width. Its core value is moving between
  many windows by scrolling — and Khadi is pane-centric, so that value does not
  apply. Its core constraint, windows as columns in a strip, fights a terminal
  that needs 200+ columns.
- **Hyprland** puts one window fullscreen and gets out of the way, which is the
  entire job here. It is also proven, has the deeper ecosystem, and already
  runs on the development machine.

Differentiation should come from `khadi-hud` and the pane-centric workflow, not
from a compositor that argues with the layout.

**Config written.** `config/foot/foot.ini`, generated from `themes/tron.toml`
and validated with `foot --check-config`. The theming pipeline now reaches
terminal, multiplexer and system monitor from one token file.

### Motif spike result: the aesthetic survives, at one-row fidelity

`spike/motif.py` — throwaway Python, reads `themes/tron.toml`, paints the full
eDEX panel layout as literal cells with live `/proc` data. Not `khadi-hud`, not
a design for it: a ruler.

```
python3 spike/motif.py --variant two-row    # faithful to the CSS
python3 spike/motif.py --variant one-row    # ┬──┬ stubs
python3 spike/motif.py --variant plain      # control
```

**Chrome cost, measured at 150x44 with 9 panel headers:**

| Variant | Chrome rows | Share of height |
| --- | --- | --- |
| `two-row` — rule then `╷ … ╷` | 19 | **43%** |
| `one-row` — `┬───┬` | 10 | 23% |
| `plain` — no ticks | 10 | 23% |

**Verdict: the motif works, the two-row version does not pay for itself.**

It reads as eDEX — the label pair, the hairline and the ticks survive the cell
grid and are recognisably the same design. But 43% of vertical space carrying
no information is not the same cost eDEX pays. In CSS the tick is sub-pixel
decoration; in a terminal it is a whole row that could have held a process.
The screenshot is also ~40% chrome, but that chrome *decorates* rather than
*consumes*.

**Decision — fidelity is tiered by surface:**

| Surface | Motif | Cost |
| --- | --- | --- |
| `khadi-hud panel` (GTK/CSS) | Full two-row, exact CSS geometry | Sub-pixel |
| `khadi-hud dash`, TUI surfaces | One-row `┬───┬` | 23% chrome |
| Third-party TUIs (btop, Yazi, Helix) | None available | Colour only |

**Second finding — the clock does not fit.** eDEX's clock is `10vh`, visually
dominant. At 4-cell-wide block digits `HH:MM:SS` needs 39 columns; the left
panel is 34. The spike had to drop to 3-wide glyphs to stay legible. Either
drop seconds, widen the left column past 34, or accept a smaller clock than the
reference. Open.

**Consequence for the roadmap: none.** The hypothesis that a composed chassis
plus one original component can carry the eDEX identity is now supported rather
than assumed. `khadi-hud` stays at Phase 3 — there is no longer a reason to pay
for it early, because the thing early delivery would have de-risked is already
de-risked. Phase 1 can stay short and plumbing-focused as written.

---

## 10b. Phase 1 — current

Deliverable: one config repo, scriptable onto any Arch install. No packaging,
no ISO, no hosting. Gate: **someone else runs the script and gets your
desktop.**

**Done**

- [x] **Relocatability fixed.** Phase 0 baked three absolute paths into the
      zellij layout. Solved with `bin/khadi-panel`, a wrapper that resolves
      btop's `-c` and `--themes-dir` at runtime, so the layout names a command
      instead of a path. `grep` for absolute paths in `config/` now returns
      nothing.
- [x] **Compositor layer written** — `config/hypr/hyprland.conf`. Every bind is
      `Super`-scoped; verified mechanically. Gaps are small on purpose: the
      layout needs ≥180 usable columns, and at 1920x1080 with Iosevka Term
      size 10 the budget is 288 raw, 284 after this chrome.
- [x] **Cheatsheet** — `bin/khadi-cheatsheet`, bound to `Super + /`. Parses the
      live config files rather than carrying its own copy, so it cannot drift.
      It immediately earned its keep by catching a real collision: `Super+Shift+L`
      was bound to both `movewindow r` and `hyprlock`. Lock moved to `Super+Shift+X`.
- [x] **Package manifest** — `packages.txt`, 16 packages, every one verified
      present in core/extra. The no-AUR constraint holds for the whole Phase 1
      chassis.
- [x] **Installer** — `bin/khadi-install`. Dry run by default; `--apply`
      required. Backs up any existing target to `<target>.khadi-backup-<stamp>`
      and never deletes. `--uninstall` restores. This matters because Khadi's
      paths collide with any existing Hyprland setup, Omarchy included.
- [x] **README** — the gate artifact. Someone else has to be able to read it
      and get a desktop.

**Next**

- [x] **yazi and fuzzel configs** — `config/yazi/{theme,yazi}.toml`,
      `config/fuzzel/fuzzel.ini`. Neither package is installed here, so they
      were written against the upstream schema at the exact packaged version
      (yazi v26.9.1, fuzzel 1.15.0) and cross-checked key by key. See below.
- [x] **Configs for the remaining uninstalled pieces: mako, helix** — both
      written and rendering; mako's urgency variants were finally photographed
      in section 10q, which is what this item was really waiting on.
      *(waybar retired in Phase 3b; see section 10e)*
- [x] **VM tooling written** — `vm/khadi-vm`, `vm/provision.sh`,
      `vm/README.md`. Plain QEMU, no libvirt: one script, no daemon, nothing
      added to host system state beyond two packages. UEFI via OVMF because
      the real installer plan is archinstall + LUKS and the VM should fail the
      same way hardware would. SSH forwarded to :2222 so the guest can be
      driven without the GUI, which makes failures inspectable rather than
      screenshot-only. Nothing is shared with the host — the repo goes in as a
      tar over ssh, because a test machine that can reach back into the host
      is not a test machine.
- [x] **Gate run in the VM: 35 passed, 0 failed.** Khadi installs and runs on
      a machine that has never seen it. See below.
- [x] **fuzzel, mako, helix and the greeter all seen**, not merely validated.
- [x] **mako and helix configs** written and verified rendering.
- [x] **Editor locked: Helix.** Zero-config LSP and tree-sitter means Khadi
      ships a working editor without maintaining a plugin set, and its base16
      theme format is the first real use of the tinted-theming substrate from
      section 4. Cost: base16 reserves `base08`–`base0F` for syntax hues and
      tron has none, so syntax reads by brightness and weight rather than
      colour. Keywords and strings differ less than in a normal theme.
- [x] **greetd + tuigreet.** Process tree confirms
      `greetd → start-hyprland → Hyprland`, which also resolves the
      *"started without start-hyprland"* warning — current journal mentions: 0.
      Login was driven end-to-end by typing credentials over QMP.

**Still open**

- [ ] **A third party has not run it.** The gate passes *mechanically* on a
      clean machine — 75 checks now, including the loaded-state half that had
      never run at all until section 10l. It has not been passed *socially*,
      which is what the gate actually says. That distinction is the honest
      reading, and it is the oldest open item in this document.
- [ ] Hyprland 0.57 config migration: the `.conf` format is removed and the
      window-rule syntax changes again. **A second reason surfaced on
      2026-10-07**: when hyprlock dies while the session is locked, Hyprland
      shows a recovery screen whose documented escape is
      `hyprctl eval 'hl.clear_crashed_lockscreen()'` — and `eval` answers
      *"eval is only supported with the lua config manager"*. On a classic
      `.conf` config, which is what Khadi ships, there is no way out of that
      state from inside the session; the only recovery is a reboot. Measured
      on 0.56.2 in the VM, twice, by killing hyprlock while locked. So the
      migration is not only about a format deadline — it is the difference
      between a recoverable lock screen and an unrecoverable one.
- [x] **mako urgency variants confirmed** — all three photographed together in
      a real session, visibly distinct on the one-colour ramp. Section 10q.
- [x] **The VT is no longer a limit.** This read "tuigreet is the one surface
      that cannot be truecolor-themed". `khadi-greet` repaints the console's
      sixteen palette entries from the theme ramp, so the quantisation lands on
      Khadi's own colours. Section 10n.
- [x] **The login surface is `khadi-greet`**, written rather than configured.
      Section 10n.
- [ ] Resolve the editor decision (helix vs neovim) and write that config
- [ ] Test on a clean Arch install — the gate. Until someone who is not the
      author runs it, Phase 1 is not done.

**yazi and fuzzel: schema-checked, then checked for real.** They were first
written blind against the upstream schema at the exact packaged version, then
validated against the installed binaries once those were available. Both
stages found things the other would have missed.

*Stage 1 — written without the software installed:*

1. Fetched the upstream config at the **exact packaged version** — yazi
   `v26.9.1`, fuzzel `1.15.0` — not from `main`. The zellij gate already
   taught this lesson: `main` had five border styles the released tag did not.
2. Cross-checked **every key** in the written config against that upstream
   file. This caught four keys that do not exist: `confirm.content`,
   `help.on`, `help.run`, `help.footer`. The real names are `confirm.body`,
   `help.chord`, `help.action`.
3. Confirmed hex colours are legal. yazi's presets use named ANSI colours
   almost throughout, which would have made a hex theme silently wrong; its
   official JSON schema settles it with `^#([0-9A-Fa-f]{6})$`.
4. Validated `theme.toml` against that JSON schema. It passes.

fuzzel's own format has keys before any `[section]` header, so a plain INI
parser rejects it; parsing with a synthetic root section confirms all 26 keys
exist upstream.

*Stage 2 — against the real binaries.* `fuzzel --check-config` exits 0. yazi
was driven in a pty with a **negative control** first: a deliberately broken
colour makes it print `Failed to parse Colors`, which proves a clean run
actually means something. The real config runs clean.

Then the rendered output was measured rather than eyeballed, and that found
two things the schema check could not:

1. **Four non-Khadi hues reaching the screen** — `#03a9f4`, `#8bc34a`,
   `#89e051`, `#dddddd`. Source: yazi's `[icon]` section, 725 rules each
   carrying a hardcoded Material Design hex. A one-colour design was quietly
   importing twenty hues.
2. **Two Private-Use-Area glyphs**, `U+E0B4` and `U+E0B6` — Powerline wedges
   wrapping the hovered row, from `[indicator] padding`. They need a Nerd
   Font. Khadi's locked Iosevka Term is not one, so on a clean Arch install
   they would have rendered as tofu — a Phase 1 gate failure, found only
   because the gate is "someone else's clean machine".

Both are now overridden. Measured after: **0 non-Khadi colours, 0 PUA glyphs,
and the only non-ASCII character on screen is `│`** — the hairline, which is
the motif.

Worth recording as method: a `json.dumps` scan for PUA characters reported
none, because it escapes them to the literal text `\ue0b6`. The glyphs were
only found by rendering and inspecting the screen. Checking the config is not
the same as checking the output.

**One-colour design, applied.** yazi colours files by MIME type by default —
images yellow, archives red, directories blue. tron defines no ANSI 16, so
there is no palette for that. Rather than invent hues the rest of the system
does not have, the Khadi rules collapse onto the alpha ramp: brightness
carries importance, not file type. This is the documented cost of the
one-colour theme (section 3), taken deliberately rather than worked around.

**Validated how.** Everything above was checked against the real binaries:
`foot --check-config`, a mechanical duplicate-bind scan, `pacman -Si` for all
16 packages, and the layout re-rendered through a pty harness after the
relocatability change. Configs for packages that are not installed are
deliberately *not* written yet — writing unvalidated config is how dotfile
repos rot.

---

### VM gate run: 35 passed, 0 failed

Tooling in `vm/`: `khadi-vm` (QEMU wrapper), `bootstrap.sh` (scripted base
Arch), `provision.sh` (packages + install), `session.sh` (autologin +
compositor), `gate.sh` (the checklist, mechanised). A gate checked by eye is a
gate passed by wishful thinking.

**Three install bugs, each invisible on the author's machine.** This is exactly
what the gate is for — "someone else's clean machine" is a different machine.

| Bug | Why it hid |
| --- | --- |
| `ln` failed: `~/.config` does not exist on a fresh account | The author's machine already had it |
| `khadi-panel` unresolvable in a login shell | Arch's `/etc/profile` adds `/usr/local/bin`, **not** `~/.local/bin` |
| Writing the PATH block to `~/.profile` did nothing | **bash ignores `~/.profile` when `~/.bash_profile` exists** — and `/etc/skel` ships one |

The third is the dangerous kind: the installer reported success, the file was
written, and the PATH still did not work. It surfaced only because the check
used `bash -lc` rather than trusting a non-interactive ssh shell, which *did*
have the path. `khadi-install` is now shell-aware (`.bash_profile` →
`.bash_login` → `.profile`, plus `.zprofile` and a fish `conf.d` drop-in),
idempotent, and `--uninstall` strips the block from all of them.

**Four more found once the compositor ran:**

- `dwindle:pseudotile` no longer exists in Hyprland 0.56 — it became a window
  state. Removed.
- `windowrulev2` is deprecated. Its replacement accepts only `tag` and
  `content` as matchers, so matching by class needs a tag and setting a tag
  needs a class — circular. The `.conf` format is removed entirely in 0.57, so
  the three cheatsheet rules were dropped rather than reverse-engineering a
  transitional syntax with a known expiry date. The cheatsheet tiles instead
  of floating, which reads fine.
- **Waybar shipped unthemed** and its defaults are a riot of hues — green,
  blue and purple pills — actively fighting the one-colour design. Now
  generated from `themes/tron.toml`: label pairs, hairline bottom rule, no
  rounded anything.
- Waybar will not start over ssh (`cannot open display`); it needs
  `WAYLAND_DISPLAY`. Launch it through `hyprctl dispatch exec`.

**Three QEMU bugs worth recording**, because each would have failed the gate
for reasons having nothing to do with Khadi:

| Symptom | Cause | Fix |
| --- | --- | --- |
| Guest used `bochs-drm`; virtio-gpu idle with *"Cannot find any crtc"* | QEMU adds its **default VGA** on top of the one you specify | `-vga none` |
| 265 `eglMakeCurrent` failures; `screendump` returned *"no surface"* | glvnd reads `egl_vendor.d` in name order, so `10_nvidia.json` beats `50_mesa.json` and EGL lands on the discrete GPU that is not driving the screen | pin `__EGL_VENDOR_LIBRARY_FILENAMES` to mesa; `gl=off` for capture |
| Resolution stuck at 732x810 (~109 cols, under the 180 minimum) | **The GTK window's size dictates guest resolution**, and a tiling WM sized it | headless mode: `-display none` + VNC, framebuffer fixed at 1920x1080 |

**What the run proves, and what it does not.** The chassis installs and runs
clean: 288 columns, one palette throughout, no multiplexer chrome, no
keybinding collisions. It is **not** eDEX, exactly as the Phase 0 A/B
predicted — btop's nested boxes and `■■■■` meters dominate the side columns,
and there is no clock, no CPU graph and no bracket tick anywhere. That gap is
`khadi-hud`'s specification, now visible rather than theoretical.

### Gate: 42 passed, 0 failed

Three more bugs surfaced once greetd replaced the autologin scaffolding:

| Bug | Why it mattered |
| --- | --- |
| `~/.local/bin` missing from the **session** PATH | `khadi-install`'s shell-profile fix covers login *shells*; a greetd-launched session never sources them. `khadi-panel` was unfindable and the layout's side panels could not spawn. Fixed with `env = PATH,...` in `hyprland.conf`, which Khadi owns. |
| A failing `exec-once` is completely silent | The session just comes up without a terminal and nothing says why. Now logged to `~/.cache/khadi-session.log`. |
| `Control+Shift+plus` in `foot.ini` | `plus` is already the shifted form of `equal`, so foot silently rewrote the binding. Caught by the logging above within a minute of adding it. |

**A correction worth recording.** Several steps were spent chasing "foot will
not start", which was a bug in the *test harness*, not in Khadi. The helper
resolved `HYPRLAND_INSTANCE_SIGNATURE` with `ls | head -1` — alphabetical
rather than newest — so after a session restart every `hyprctl` call silently
targeted a dead compositor that answered `ok` to everything. Both the helper
and `gate.sh` now use `ls -1t`.

**What the gate learned to check, by being wrong twice:**

1. **Loaded state, not files on disk.** It once passed a system where
   `gaps_out` was the default and only 6 of 36 binds were live, because it read
   the config file rather than asking `hyprctl`. (Cause: `tar` replacing files
   under a running Hyprland makes its config watcher fall back to an
   autogenerated config. `khadi-vm push` now reloads.)
2. **A child's environment, not Hyprland's own.** Hyprland's `env =` applies to
   what it launches, never to itself, so reading `/proc/$(pgrep Hyprland)/environ`
   reports a PATH that no application actually sees.

Both are the same mistake in different clothes: checking the thing that is easy
to read instead of the thing that matters.

### Phase 1: what is done

| | |
| --- | --- |
| Config repo | `config/` mirrors `~/.config`; nothing holds an absolute path |
| Installer | dry-run default, backs up, idempotent, shell-aware, `--uninstall`, `--system` for greetd |
| Chassis configs | foot, zellij, btop, hypr, yazi, fuzzel, waybar, helix, mako, greetd — all generated from `themes/tron.toml` |
| Packages | 19, every one verified present in core/extra; no AUR |
| Test harness | `vm/` — QEMU wrapper, scripted Arch install, provisioning, session, and a mechanised gate |
| Gate | 42 checks, 0 failures, on a machine that had never seen Khadi |

Everything the chassis needs now exists and runs. The honest caveat stays at
the top of the Next list: a gate that says *someone else* has only been passed
by the author's own VM.

## 10c. Phase 2 — current

Deliverable: `khadi-theme` plus templates, three themes. Gate: **one file
change restyles every app in the stack.** The gate passes.

**Done**

- [x] **`bin/khadi-theme`** — reads a theme, renders `templates/` into
      `config/`, and emits a resolved theme for runtime consumers.
- [x] **16 templates**, one per generated config.
- [x] **Three themes**: tron, matrix, blade — the last two ported from
      upstream eDEX and verified against its JSON.
- [x] **Wired in**: `khadi-install` builds before linking, so `config/` can
      never be stale; `gate.sh` fails if `config/` has drifted from
      `templates/`.
- [x] **Gate passed live**: switching the running VM to matrix turned waybar,
      both btop panels, the zellij divider and the terminal phosphor green
      from one 22-line file.

### The architectural fix that made it work

Phase 1's `themes/tron.toml` **stored** the alpha ramp, the semantic roles and
the terminal block — 23 values. A new theme meant computing all of them by
hand, which is the opposite of a single source of truth.

`khadi-theme` now **derives** them from four palette colours. A theme file is
inputs only:

| | Phase 1 | Phase 2 |
| --- | --- | --- |
| `themes/tron.toml` | 201 lines | 53 lines (mostly comment) |
| A new theme | ~17 hand-computed values | 4 colours + 4 fonts, ~20 lines |

Proved two ways. The generator reproduces **all 23** previously hand-computed
values exactly; and rendering the templates reproduces all 16 hand-written
configs **byte for byte**, so the conversion lost nothing.

### Decisions

- **Python, not Rust.** `khadi-hud` is Rust because it redraws at interactive
  rates. This runs for about 40ms when a theme changes; Rust would buy nothing
  and cost a build step in the `khadi-theme` package.
- **No template library.** The substitution needed is
  `{{ path.to.value | filter }}` and nothing else, because the generator
  resolves everything to flat values before rendering. Jinja would be larger
  than the feature.
- **Chrome and scale are generator defaults, not theme data.** The hairline
  alpha and the tick geometry are eDEX motifs, identical in every theme. A
  theme may override them; none should need to.
- **Generated files are stamped.** Every output carries a `GENERATED by
  khadi-theme` banner in the right comment syntax. An unmarked generated file
  invites an edit that the next build silently destroys.
- **Theme files are named for the product, not the theme.** `khadi.theme`, not
  `khadi-tron.theme` — the latter becomes a lie the moment another theme is
  built. Caught while switching to matrix.

### tinted-theming: audited, and not adopted

Section 4 said to build on tinted-theming's template library rather than
hand-write everything. Audited against their 23 `base16-*` repos:

| Khadi app | Upstream template |
| --- | --- |
| waybar | yes |
| foot, fuzzel, mako, btop, yazi, zellij, hyprland, helix, greetd | **none** |

**One of ten.** The plan's assumption does not survive contact — adopting a
dependency and a build step to cover a single app is a worse trade than
writing that app's template.

There is a deeper mismatch too. base16 is a sixteen-hue model; Khadi is a
one-colour design where depth comes from alpha over the background. A base16
template would push a one-colour theme into sixteen slots it does not have,
which is backwards. The *concept* is still used where it fits — the Helix
theme is written in base16 keys because Helix speaks base16 natively — but the
templates stay Khadi's own.

### `khadi-check`: the rolling-release mitigation

Section 11 names upstream config-format drift as a standing risk and prescribes
"CI that renders every template and diffs output on each upstream bump".
`bin/khadi-check` is that: it renders **every** theme and runs each app's own
validator — `foot --check-config`, `fuzzel --check-config`, TOML and JSON
parsers — plus a one-colour invariant that fails if any output contains a
colour the theme does not define.

Proved with three negative controls, and **one of them failed first time**.
Injecting `#ff00ff` into waybar's stylesheet passed a checker that looked like
it worked: the comment-stripper treated any line starting with `#` as a
comment, which silently skipped every CSS id selector — exactly where stray
colours hide. Comment syntax is now per format, never guessed.

### ANSI: resolved in the generator, not branched in templates

A theme may declare `[terminal.ansi]`; nine of twenty-one upstream eDEX themes
do. Rather than add conditionals to sixteen templates, the generator resolves
the ANSI block with a ramp fallback, so every template references `ansi.*`
unconditionally and stays flat.

`themes/nord.toml` is the fourth theme and the one that exercises this: it
carries a real sixteen-colour palette, so Helix syntax and Yazi file types get
actual hues instead of collapsing onto the ramp. It also exercises an upstream
inconsistency on purpose — nord spells the bright half `light*` where the other
eight use `bright*` — and is written exactly as upstream has it rather than
tidied on the way in.

The fallback was validated by construction: switching the templates from
hardcoded ramp values to `ansi.*` left tron's output byte-identical.

### `config/` is build output, and the generator owns it

Renaming `khadi-tron.theme` to `khadi.theme` left the old file behind on the
test machine, because `tar` and `cp` add files but never remove them. Every
machine that had ever synced would have kept the orphan — carrying tron's
colours into every other theme.

`khadi-theme build` now prunes any output with no template behind it, and
`khadi-vm push` mirrors deletions. The lesson generalises: a build directory
that is only ever added to is not a build directory.

### Still open

- [ ] `khadi-theme` and `khadi-check` ship as repo scripts, not packages.
      That is Phase 4 work.
- [x] **`khadi-check` now runs automatically** — `.github/workflows/check.yml`,
      daily on a schedule and on every push. That was the remaining half of
      the section 11 mitigation. See section 10g.

---

## 10d. Phase 3a — current

`khadi-hud dash` now occupies the system column, replacing btop there.

### The A/B moved: 1 faithful → 11

| | Phase 0 | Now |
| --- | --- | --- |
| Faithful | 1 | **11** |
| Approximated | 4 | 4 |
| Missing | 13 | 2 |

Newly faithful: the bracket-tick header, the block clock, the hardware block,
per-core sparklines, the memory grid, the swap bar, the process list, the
network header, the traffic graph and the filesystem strip. Every one was
"missing" because no off-the-shelf TUI draws it — which was `khadi-hud`'s
whole specification.

**btop is no longer in the layout at all.** Three `khadi-hud` panels and the
terminal.

Two items remain, and neither is obviously khadi-hud's job:

- **TERMINAL / MAIN SHELL header** sits above the terminal pane, so it is
  probably a zellij layout concern rather than a panel.
- **Prompt pill** is a shell prompt — a starship config, not Rust.

Two more are settled rather than pending: the **icon-grid browser** is
deliberately yazi's (an icon grid needs a Nerd Font Khadi does not ship, and
Phase 1 measured the tofu), and the **skewed tab bar** is impossible in a cell
grid — 3b, where it costs sub-pixel instead of a row.

### It fits where btop would not

The measured reason btop could not hold an eDEX side column was its minimum
widths: 60 for the CPU box, 44 for processes. `khadi-hud` draws clock,
hardware, CPU, memory, swap and processes in **34**, which is the reference
proportion.

The side columns dropped from 84 to 74 columns, so eDEX's ~34% split is now
reached at **217 columns instead of 247** — the proportions problem the Phase 0
A/B identified is materially better.

### Structure

| | |
| --- | --- |
| `crates/khadi-core` | theme + metrics, **no UI dependencies** — the GTK panel in 3b consumes the same code |
| `crates/khadi-hud` | the motif widgets, the dashboard, the binary |
| | 10 tests, 1.5 MB release binary |

`khadi-core` reads the **resolved** `theme.toml` that `khadi-theme` emits
rather than re-deriving the ramp. Two implementations of one derivation is two
chances to disagree, and Phase 2 existed to prevent exactly that.

### Bugs worth recording

- **A UTF-8 panic at exactly 34 columns.** The memory grid byte-sliced a
  `String` of `▀` glyphs. Fixed by building runs from chars, and covered by a
  sweep test over every width 1–40 × height 1–8 for all three glyph widgets —
  that class of bug only appears at specific widths.
- **The clock was right by luck.** A hand-rolled UTC offset matched the system
  clock perfectly *because the development machine is in Africa/Accra, which
  is UTC+0*. It would have been silently wrong for almost every other user.
  Now `chrono::Local`.
- **CPU brand strings do not fit.** `Intel(R) Core(TM) i7-7500U CPU @ 2.70GHz`
  leaves nothing for the label in 34 columns. Stripped to
  `Intel Core i7-7500U`.
- **Truncation now ellipsizes.** `Standard PC (Q35 + ICH9, 2009` reads as a
  complete string and is not one.

### Three failures in the dev loop, not the product

Worth recording because each looked like success:

1. `tar --exclude=target` drops `./target/release/khadi-hud` **even when the
   path is listed explicitly**, so a stale binary shipped silently. The binary
   now transfers separately.
2. `ssh -n` points stdin at `/dev/null`, so piping the binary into `cat`
   delivered an empty file. The push now verifies a checksum, which is what
   caught it.
3. `rm -rf ~/khadi/config` fails with *"Directory not empty"* because
   Hyprland's config watcher regenerates `hyprland.conf` the instant it
   vanishes. `config/` is no longer deleted on push — `khadi-theme` prunes it,
   which is the correct owner anyway.

### Network panel: done, and btop is out of the layout

`khadi-hud net` holds the right column: state, interface, IPv4, a braille
world map, traffic sparklines and totals. **btop no longer appears in the
layout at all** — both side columns are Khadi's own code.

Two details worth recording:

- **The IPv4 lookup sends nothing.** A connected UDP socket only sets a
  destination for later writes; no packet leaves the host, and the kernel then
  reports which local address routing chose. The alternative was parsing
  `/proc/net/fib_trie`, a tree dump, for the same answer.
- **Traffic sparklines scale to their own window peak.** Absolute byte rates
  span six orders of magnitude; a fixed scale is either flat or clipped.

### Filesystem strip: done

`khadi-hud fs` runs full-width along the bottom: mounts, usage bars, sizes and
percentage, degrading from two columns to one when narrow.

eDEX's FILESYSTEM panel is two things under one name — a clickable icon grid
and a mount-usage bar. Khadi splits them. Browsing is yazi on `Super+E`;
only the usage had no home anywhere else.

Two details:

- **btrfs reports every subvolume as a separate mount** with identical totals,
  which filled the panel with the same number six times. Collapsed by keeping
  the shortest path per (device, size).
- **The row is laid out right-to-left from measured text.** The first version
  reserved a guessed 24 columns and the usage bar ran straight through
  `58.27 GiB / 236.46 GiB`, which is 22. Covered by a test that sweeps widths
  24–200 and fails on a bar glyph adjacent to a digit.

### Remaining in 3a
- [x] **Terminal header — item 10.** It belongs to the zellij layout, as
      suspected: a two-row pane above the shell running `khadi-hud header`.
      See section 10f.
- [x] **Prompt pill — item 13.** A starship config, and the angled edge turned
      out to be drawable. See section 10f.
- [x] The angled tab bar (item 11) stays impossible in a cell grid; it is 3b
      work, on the GTK panel — **done**, see section 10e.

---

## 10e. Phase 3b — current

`khadi-bar` is a Wayland layer-shell panel. It exists for the two motifs a
cell grid structurally cannot draw, and both now render.

### The angled tab bar works

eDEX skews each tab into a parallelogram and skews the label back so the text
stays upright — `skewX(35deg)` on the container, `skewX(-35deg)` on the label.
A terminal cannot skew anything. **GTK4's CSS parser accepts `skewX` with zero
errors**, verified against the generated stylesheet before any of the panel was
written, and the tabs render as parallelograms in the VM.

That motif was found during the Phase 0 A/B and logged as *impossible* — it is
the one element of the eighteen that could only ever live here.

### The bracket tick, at full fidelity

In a terminal the tick costs a whole row: Phase 0 measured 43% of screen height
across nine headers, against 23% for the one-row compromise the TUI uses. In
CSS it is sub-pixel decoration, exactly as eDEX had it. The bar uses the
faithful geometry.

### Decisions

- **A separate binary, not `khadi-hud panel`.** The plan sketched a subcommand.
  Three `khadi-hud` processes are resident in the layout at once; linking all
  of GTK into each of them to serve a fourth, different surface is a cost paid
  three times for nothing. `khadi-core` carries no UI dependencies precisely
  so this split is free.
- **Named `khadi-bar`, not `khadi-panel`.** `bin/khadi-panel` is already the
  zellij pane wrapper, and two different things with one name is how confusion
  starts.
- **The stylesheet is generated like every other config.**
  `templates/khadi/bar.css.tmpl` → `config/khadi/bar.css`, so the bar cannot
  drift from the palette the rest of the system uses.
- **GTK4 and gtk4-layer-shell are new runtime dependencies.** waybar used GTK3,
  so this is genuinely new weight, caught by the VM refusing to start the
  binary. Both are in `extra`; the manifest is 21 packages, all verified.

### Tested on real hardware, and waybar is retired

`khadi-bar` was run on a real Hyprland session with a real GPU — not the VM's
software rendering. The angled tabs render, the hairlines are crisp, nothing
broke. waybar is now gone from `packages.txt`, the installer, the checker and
the templates; `khadi-theme` pruned its generated configs as orphans.

Two things made that test safe to run on a live desktop, and both are worth
keeping:

- **`KHADI_BAR_OVERLAY=1`** floats the bar instead of claiming an exclusive
  zone. Without it the compositor reserves the strip and reshuffles every
  window on screen, which is correct in a Khadi session and hostile when
  testing on a desktop that already has a bar.
- Fonts were loaded from `XDG_DATA_HOME=/tmp/...` rather than installed, so
  nothing persisted to the host.

### The typography had never been seen

**`Rajdhani` and `Orbitron` are installed nowhere** — not on the development
machine, not in the VM. Every screenshot of the bar until this test was
rendering in *fallback* faces. The fonts locked in Phase 0 had never actually
appeared on a screen, and nothing in the gate or `khadi-check` noticed, because
a missing font is not an error: fontconfig silently substitutes.

With them loaded the difference is obvious — Orbitron's wide geometric clock
and Rajdhani's squared labels are the eDEX character the substitution was
chosen for. The Phase 0 lock holds up. But it was being reported on without
ever having been looked at.

**This promotes `khadi-fonts` out of Phase 4.** It was filed as packaging work,
which assumed it only mattered at distribution time. It does not: without it
Khadi renders in the wrong faces on every machine including the author's, and
the failure is invisible. Vendoring the OFL files into the repo and installing
them is now Phase 3b work, ahead of packaging.

A check belongs with it: the gate should fail if a font the theme names is not
resolvable, because silent substitution is exactly the failure mode here.

### The tabs are Hyprland's workspaces

eDEX's tabs are shells: tab 0 reads `MAIN SHELL`, the other four read `EMPTY`
until something runs in them. Khadi's equivalent of a shell is a workspace, and
`hyprland.conf` binds exactly five to `Super+1..5` — so the mapping is one tab
per bound workspace, and the count lands on eDEX's five without being chosen to.

**Event-driven, not polled.** A thread blocks on `.socket2.sock` and re-queries
`.socket.sock` only on the events that can move a tab; the UI collects the
newest snapshot every 100ms. An idle session does one mutex lock per tick and no
IPC at all. Polling the query socket instead would either lag a workspace switch
or spend two roundtrips a second discovering nothing had changed. Snapshots are
not queued — a stale one has no value — so a burst of events costs the bar one
update, not one per event.

**The label vocabulary.** A workspace the user has named keeps its name; that is
information the numeric fallback does not have. Hyprland names unnamed ones
after their own id, and a bare "3" is not a label, so those get `MAIN SHELL` and
`SHELL n`. Empty reads `EMPTY` — and so does absent, because **Hyprland drops a
workspace from `j/workspaces` the moment its last window closes**. Those are the
same state to a user and must not look different on the bar.

A sixth tab, hidden by default, appears when a workspace outside `1..5` is
focused. `Super+1..5` cannot reach one, but a scratchpad or a scripted dispatch
can, and a strip showing no active tab at all reads as a broken strip.

**Verified on real hardware**, not in the VM: the active tab tracked a focus
change between workspaces, and renaming a workspace from outside the process
repainted its label within the tick. Workspace 3 was empty throughout and read
`EMPTY`, which is the absent-vs-empty case above, observed rather than reasoned
about. Two ignored tests (`cargo test -p khadi-core hypr -- --ignored`) keep the
live check runnable, since the unit tests prove the parsing and the event filter
and nothing about whether the socket path and JSON shape still match upstream.

### Hyprland 0.56 changed `hyprctl dispatch`, and the bar did not notice

Testing this turned up the section 11 risk in the wild. Hyprland 0.56 added a
Lua config manager, and `hyprctl dispatch` follows whichever config manager is
active:

```
hyprctl dispatch workspace 2                       # classic config
hyprctl dispatch 'hl.dsp.focus{ workspace = 2 }'   # Lua config
```

**Corrected later.** This section first recorded it as "0.56 replaced string
dispatch with a Lua API", full stop. That is what it looks like from the
development machine, which runs Omarchy and therefore a Lua config — but it is
the config manager that decides, not the version. Khadi ships a classic
`hyprland.conf`, so classic dispatch is the correct form on a Khadi machine.
Measured both ways in the VM when hypridle needed `dpms` (section 10p).

Nothing in Khadi broke, for two reasons worth keeping. The keybindings are
`bind` lines in `hyprland.conf`, which the config parser still accepts; and
`khadi-bar` talks to the IPC socket directly rather than shelling out to
`hyprctl`. **The sockets, the request strings and the JSON shape did not
change** — only the CLI wrapper did. A bar built on `hyprctl` output would have
gone blank on this upgrade.

That is an argument for the general rule: read the protocol, not the tool that
prints it.

### Not yet done
- [x] **`khadi-fonts` vendored** — `fonts/` carries Rajdhani (Light, Regular,
      Medium) and Orbitron with their OFL licences, 1.2 MB. `khadi-install`
      links them into `~/.local/share/fonts/khadi`, rebuilds the fontconfig
      cache and verifies the result; `--uninstall` removes the link. Iosevka
      Term stays a package dependency until the Phase 4 subset replaces the
      446 MB `ttc-iosevka`.
- [x] **`bin/khadi-fontcheck`** — the check that was missing. It asks
      fontconfig what it *would actually use* for each family the theme names
      and fails when the answer is something else, because "it rendered" says
      nothing about what it rendered with.

      Two modes, because there are two different questions. The **gate** asks
      whether this machine can render the theme. **khadi-check** asks whether
      the font is obtainable at all — vendored here or available from a
      package — since nothing is installed in a repo and asking the first
      question there would only report that fact.

      The first version had a false negative: it compared family alone, so
      `Rajdhani Light` (family `Rajdhani`, style `Light`) failed against a
      correctly installed font. It now compares family and style.
- [x] **Workspace tabs are live** — `khadi-core/src/hypr.rs` reads Hyprland's
      IPC and `khadi-bar/src/tabs.rs` renders it. See above.
- [ ] No click handling. eDEX's tabs are clickable; Khadi is keyboard-driven,
      so this may stay as it is deliberately.
- [ ] The bar duplicates readouts `khadi-hud` already shows. eDEX puts chrome
      along the top, so this is faithful, but it is worth asking whether both
      should exist on a small screen.

---

## 10f. Phase 3c — the last two A/B items

Items 10 and 13 were the only two the Phase 0 backlog still listed as missing,
and neither belonged to `khadi-hud` or to `khadi-bar`. Both are now done, which
takes the A/B to **13 faithful of 18** with nothing left in the backlog.

| | Phase 0 | 3a | Now |
| --- | --- | --- | --- |
| Faithful | 1 | 11 | **13** |
| Missing | 13 | 2 | **0** |

### Item 10 — the terminal header is a pane

`TERMINAL / MAIN SHELL` is chrome for the shell pane, and neither candidate
owner would draw it. The Phase 0 gate ruled out zellij's frame glyphs, and
anything the shell itself prints scrolls away with the first screenful of
output. So it gets a pane of its own, two rows tall, running `khadi-hud
header` — the same answer the three panels got, and the correction the gate
forced: **every pane draws its own chrome.**

**The pane is `size=3` for a two-row header.** zellij's horizontal divider eats
a row the way its `│` eats a column, which the layout already allowed for in
the side columns and nobody had carried over. At `size=2` the pane gets a
one-row pty, `Header`'s height guard fires and the header ships **blank** — not
degraded, blank. That shipped once before it was caught, which is why there is
now a test pinning the two-row minimum: a widget that silently draws nothing
reads as a broken binary, not as a sizing mistake.

Focus now starts on the shell. It had been starting on the SYSTEM readout,
because zellij focuses the first pane in the layout and nothing said otherwise.

### Item 13 — eDEX ships no prompt

**The pill in `screenshot_default.png` is the screenshot author's own PS1.**
There is nothing in the eDEX source to copy — no CSS, no shell integration,
nothing. So the A/B had been scoring Khadi against a stranger's dotfiles, and
item 13 is Khadi's to define rather than to reproduce.

**The angled edge is drawable, which the A/B assumed it was not.** U+E0B0 is
present in Iosevka Term — measured with fontconfig, not hoped for. The skew
that is structurally impossible for a tab bar across a cell grid is available
for a one-glyph prompt terminator, because one glyph is all it needs.

The pill is a **recess, not a chip**: the reference fill is darker than the
terminal ground, which is what `role.ground_deep` is for. The path is fish-style
abbreviated — leaf in full, every ancestor at one character — which is what the
reference shows and what starship's `fish_style_pwd_dir_length` already does.

**starship, not a shell function.** Khadi does not own the login shell; `fish`
is in the manifest as "interactive default only". A fish-only prompt would be
absent for most users on the day they install, so the prompt is cross-shell and
the init block goes in the **interactive rc** — `.bashrc`, `.zshrc`,
`fish/conf.d` — not the login profile the PATH block uses. On every shell those
are different files.

The written block guards itself with `command -v starship`. Khadi does not
install packages, so starship may be missing at install time or removed later,
and an unguarded init line turns that into an error on every shell start.

### Two checks, because both failures are silent

- **`khadi-check` now validates the starship config.** starship **always exits
  0**, even on a file it could not parse — it warns on stderr and carries on
  with its defaults, so a prompt that silently lost its pill looks like a
  working command. stderr is the exit status here. Verified against two
  negative controls: an unknown key warns, a malformed file errors.
- **`khadi-fontcheck` now checks glyph coverage, not just family resolution.**
  A family that resolves can still lack the glyph, which is the same silent
  substitution one level down — the terminal draws tofu and reports success.
  Phase 4 replaces the 446 MiB `ttc-iosevka` with a two-face subset, and a
  subset is exactly where a private-use codepoint goes missing without anyone
  deciding to drop it. Negative control: Liberation Mono resolves as a family
  and fails the glyph check.

### A bug the wiring turned up

`khadi-install` called `return 0` at top level to end the fish branch of the
PATH block. `return` outside a function fails, `set -e` sees it, and the
installer **exits 2 right there** — so a fish user got the PATH block and then
no fonts, no prompt, and a non-zero exit. Reproduced in six lines of bash
before fixing it. Both install paths and the uninstall round trip are now
exercised against a throwaway `$HOME`.

---

## 10g. The rolling-release watch

Section 11's top risk had half a mitigation. `khadi-check` renders every theme
and validates the output with each app's own checker, but nothing ran it except
a human, and a check nobody runs is documentation.

`.github/workflows/check.yml` runs it daily on real Arch.

### Daily, because there is no event to subscribe to

A rolling release publishes no "upstream bumped" signal. There is no release
feed to hook, no dependabot equivalent, nothing to be notified by — so the
schedule *is* the trigger. Daily rather than weekly because the stated goal is
"a failing build, not a user report", and a week-old break has already been a
user report.

### On Arch, in a container, or it proves nothing

Every validator `khadi-check` runs is the app's own: `foot --check-config`,
`fuzzel --check-config`, `starship prompt`. None of them exist on a stock
GitHub runner, where the whole suite would report **skipped** and the build
would go green having checked nothing.

That is what `--strict` is for. A skipped validator is a failure, because on a
box that has just installed the entire manifest, "not installed" can only mean
the install did not do what it said.

### Installing the manifest is itself a test

`khadi-check` asks whether each package name still *resolves*. The workflow
asks the harder question of whether they still *install together*, which is
where a new conflict or a dropped package shows up. A name that moved to the
AUR is a break Khadi cannot fix at install time — the manifest promises
official repos only — and it should fail a build the day it happens.

The cost is `ttc-iosevka`, 446 MiB for 189 faces, pulled on every run. That is
the price of asking the font and glyph questions for real rather than skipping
them, and Phase 4's two-face subset removes it.

### Verified, not just written

The workflow was run step for step in an `archlinux` container before being
committed as working — a CI file that has never executed is a guess, and the
first thing it would do is fail on a push.

| Step | Result |
| --- | --- |
| Install the manifest | all 21 packages, together, on fresh Arch |
| `cargo build --release` | khadi-bar links GTK4 and gtk4-layer-shell from it, 2m44s |
| `cargo test` | 24 passed, 2 ignored — the live Hyprland tests, correctly skipped with no compositor |
| `khadi-check --drift --strict` | **27 passed, 0 failed, 0 skipped** |

The zero in that last column is the one that matters. Under `--strict` a skip
is a failure, so zero skips means every validator actually ran: `foot`,
`fuzzel` and `starship` against all four themes. That is the difference between
a green build and a green build that checked something.

### What this would have caught

This session found Hyprland 0.56 replacing hyprctl's string dispatch with a Lua
API (section 10e). Nothing in Khadi broke, because the keybinds are `bind`
lines the config parser still accepts and `khadi-bar` reads the IPC socket
rather than shelling out. But that was luck confirmed after the fact by hand,
on one developer's machine, a month after the release. It is exactly the class
of change the schedule exists to surface on day one.

**What it does not cover.** `vm/gate.sh` needs a running compositor and still
runs only locally, so "loaded state, not just the file on disk" remains a
manual gate. CI checks that the configs are *valid*; the VM checks that they
are *in effect*.

The gate itself grew. Its config-link and binary lists are now derived from
`khadi-install` instead of being a second copy — which found six config links
and three binaries it had never been checking, a gate drifting in the direction
that passes — and it has a prompt section. **Re-measured in the VM: 53 passed,
0 failed**, up from 42, with the four loaded-state checks excluded because that
run was headless.

That re-run also found a trap in the gate itself. `ssh host 'bash gate.sh'` is
neither a login nor an interactive shell, so it sources neither `/etc/profile`
nor the login profile the PATH block goes into, and all six binaries report
missing on a correct install. The gate now distinguishes "not installed" from
"installed, and this shell never read the profile", because the first is a
broken install and the second is a broken invocation.

---

## 10h. Focus mode

Section 11's answer to "chrome eats the screen" was a `Super+f` focus layout.
The key is `Alt+f`, not `Super+f`, and the reason is the keyboard model: **each
layer owns exactly one modifier**, and the chrome being dropped is zellij's
panes. `Super` reaching into zellij would be the first exception in a model
whose whole value is having none. The note in section 11 predated the model.

It needed no new zellij code — `ToggleFocusFullscreen` was already bound — but
it was labelled "zoom pane" in the cheatsheet, which is what it does rather
than what it is for. A dismissibility feature nobody can find is not a
mitigation. It now reads `FOCUS MODE — bare terminal`.

### The bar cannot see it, so it watches something else

A zellij pane zoom is internal to zellij. The compositor never hears about it,
so `khadi-bar` has no signal to react to and no amount of wiring will give it
one. The bar instead hides when **Hyprland** reports a fullscreen window, which
it learns from the same event socket the workspace tabs already use.

Two levels, each owned by the layer that can actually see the thing it drops:

| | drops | owner |
| --- | --- | --- |
| `Alt+f` | panels, terminal header, dividers — the ~40% | zellij |
| `Super+F` | the bar, with its exclusive zone | Hyprland |

Hiding releases the exclusive zone with it, so the window below grows into the
strip instead of leaving a band of wallpaper where the bar was.

Measured rather than eyeballed: the bar's strip reads mean luminance 0.1119
normally, 0.0367 with a window fullscreen, and 0.1119 again afterwards —
identical to before, so nothing is lost on the way back.

---

## 10i. The globe turns

The Phase 0 A/B listed the world-view globe as **dropped, non-goal**, and
section 12 said so too. That conflated two things: WebGL, which a cell grid
genuinely cannot do, and *a turning sphere*, which it can. What reads on screen
in eDEX is not the shading or the bloom — it is a dot matrix rotating. A
braille canvas has 2x4 subpixels per cell, which is enough.

It does not move the A/B count: the globe was never one of the eighteen
in-scope elements. It was listed below the line as dropped, and it is not.

### The land is eDEX's, not an approximation of it

`ratatui` ships coastline data but keeps it in a private module, so it is not
reachable. The better source was already in the tree: eDEX's own
`src/assets/misc/grid.json`, the 3937-tile mesh encom-globe draws its
continents from. Khadi is GPL-3.0 and so is eDEX, so the authentic grid is
available rather than a lookalike.

**Its `lat`/`lon` fields are not geographic.** Taken at face value they put
land in the Indian Ocean and none in Antarctica. The usable position is each
tile's 3D boundary — a real point on a sphere of radius 500 — and the frame was
then *solved for* against known coordinates rather than guessed:

    polar axis = Y,  lon = atan2(x, z) + 85 degrees,  not mirrored

Eight land probes and six ocean probes, 14 of 14. `spike/globe-land.py`
regenerates the data and refuses to write a file that fails its own check; a
unit test pins the same probes so the committed data cannot quietly rot.

### Two things that had to be measured

**Thinning, not quantising.** The source mesh is ~1.6 subpixels apart at panel
size, which braille renders as a solid blob, so the points have to be thinned.
Snapping them to a lattice was tried first and aliased against the mesh into
**moiré stripes** that read as a rendering defect. The thinning is a stable
hash per point instead — no lattice, nothing to beat against, and keyed on the
coordinates so the pattern holds still while the globe moves under it.

**It shipped as an egg.** Braille packs 2 subpixels across a cell and 4 down,
so a subpixel is square only if the cell is exactly 2:1. Iosevka Term has a
0.5 em advance and the cell lands nearer 2.75:1, which makes a subpixel
1 : 1.375 — and the first render measured **0.72 as wide as it was tall**. The
canvas is now scaled in subpixel *widths* on both axes, so a circle is a
circle. A test measures the rendered bounding box, because this is exactly the
kind of thing that looks fine until someone sees it.

### It is the cheapest panel, not the most expensive

eDEX's reputation was for burning a CPU, so the animation was measured rather
than assumed. Ten seconds each, release build, 38x44:

| panel | cost |
| --- | --- |
| `net` — spinning globe at 10 fps | **0.6%** of one core |
| `dash` — no animation at all | 3.1% |
| `fs` — no animation at all | 0.1% |

The animated one is five times cheaper than the static one. Projecting 2400
points is nothing; `dash` enumerates every process once a second, and that is
what costs. Worth remembering before optimising anything here on instinct.

---

## 10j. GUI apps

Section 6 has always said it — *"windows exist for browsers and graphical
apps, not for terminals"* — but nothing in the stack acted on it. Khadi is
terminal-**first**, not terminal-only, and GUI apps were supported only in the
sense that Hyprland will run anything you launch.

### The functional half: portals, and the mistake everyone makes

A portal is how an app asks the desktop to open a file or share a screen.
There were none in the manifest, so a browser's upload button opened nothing
and screen sharing failed — silently, with nothing in a log a user would think
to read.

**Two backends are required.** Read from the installed `.portal` files rather
than assumed:

| backend | implements |
| --- | --- |
| `hyprland.portal` | Screenshot, ScreenCast, GlobalShortcuts, InputCapture |
| `gtk.portal` | FileChooser, AppChooser, Print, Notification, Settings, … |

The Hyprland backend does **not** implement FileChooser. Installing it alone —
the obvious reading of "the portal for Hyprland" — gets you screen sharing and
still no file picker. `khadi-check` now verifies that every backend named in
the config exists *and* implements what it was handed; both negative controls
fire, including routing FileChooser to `hyprland`.

**The portal is D-Bus activated**, so it inherits the activation environment,
not the session's. Without `XDG_CURRENT_DESKTOP` in it the portal never matches
`hyprland-portals.conf` and quietly picks its own backend, which is why the
session now runs `dbus-update-activation-environment`.

### The aesthetic half: settings, not a theme

Phase 2's claim is "one file change restyles every app in the stack", and GUI
apps were not in the stack — so the first one you opened came up in default
Adwaita, light grey and rounded. That reads worse than no theme at all, because
it is a seam rather than an absence.

The restraint matters more than the coverage. A full GTK theme is thousands of
lines chasing libadwaita, which is section 11's rolling-release risk multiplied
by the largest config surface in the project, for apps Khadi does not own. What
ships is the dark preference, the UI font, and a `gtk.css` of colour
definitions — **recolouring only**. `bar.css` squares off every corner it owns
because it owns those widgets; this file does not, and a stylesheet that fights
someone else's toolkit loses slowly, in ways that surface months later as "this
app looks broken".

**libadwaita ignores `gtk-application-prefer-dark-theme`.** It asks the Settings
portal, which reads gsettings — so `settings.ini` alone leaves every modern
GNOME app blazing white on a near-black desktop. The session sets
`color-scheme: prefer-dark` and the gate checks it.

### A dangling keybind, and the check for the class

`Super+B` launched `chromium`, which was never in the manifest. On a clean
install that key did nothing, silently, and no check noticed because each half
was internally consistent on its own — the same shape as the gate keeping its
own copy of the installer's lists (section 10g).

`khadi-check` now requires every app a keybind launches to be in `packages.txt`
or in `bin/`. `chromium` is in the manifest because the keybind already
committed to it; if Khadi should not ship a browser, the fix is to drop the
bind instead, and the check holds either way.

### Ask the portal, not a parser

The first version of the check parsed `hyprland-portals.conf` with Python and
asserted things about it. That is the wrong authority. Every other validator
here is the app's own — `foot --check-config`, `fuzzel --check-config`,
`starship prompt` — and `xdg-desktop-portal` is the only thing that knows how
it reads its own config.

So `khadi-check` runs the real portal under `dbus-run-session`, on a private
bus where it cannot collide with or take over from the one the user is running,
and reads its verdict back:

```
XDP: Using portal configuration file '…/config/xdg-desktop-portal/hyprland-portals.conf'
XDP: Preferred portals for interface 'org.freedesktop.impl.portal.FileChooser': gtk
XDP: Using gtk.portal for org.freedesktop.impl.portal.Lockdown (default config)
```

That third line is the `default=hyprland;gtk` ordering being exercised:
interfaces Hyprland does not implement fall through to GTK instead of going
unanswered. Routing FileChooser to `hyprland` makes the real portal say
`FileChooser: hyprland`, and the check fails with the portal's own output as
the diagnostic.

**What is still unverified** is the last step only: that a file picker visibly
opens in a running Khadi session. The routing, the backend capabilities and the
config parsing are now all confirmed by upstream's own implementation; the
loaded-state gate checks exist but have been exercised only against the host's
compositor.

---

## 10k. The installer never shipped the two components Khadi writes

Found while adding the check for the `chromium` keybind, by generalising it
from "what a keybind launches" to "what the config launches":

**`khadi-install` installed neither `khadi-hud` nor `khadi-bar`.** `BINS` held
six shell scripts from `bin/`; the Rust binaries live in `target/release/` and
nothing linked them. On a clean install:

- `khadi-panel` printed *"khadi-hud not found. Build it: cargo build --release"*,
  so the layout had no system, network or filesystem panel
- `exec-once = khadi-bar` failed silently, so the session had no bar

That is Phase 3a and 3b — the entire original half of the product — absent from
an install that reported success.

**The gate did not catch it, and the reason matters more than the bug.**
`vm/khadi-vm push` builds and installs both binaries into the guest as part of
getting the repo there. So the gate ran against a machine provisioned by a
*different route* than `khadi-install`, and had been passing on a state the
installer could not produce. Phase 1's gate is "someone else runs the script
and gets your desktop"; the script was never the thing being tested.

The installer now links both from `target/release/`, and says so plainly when
they are not built rather than leaving `exec-once` to fail without a word —
Khadi does not build for you any more than it installs packages for you, but
it does have to say which it is. The gate derives `RUSTBINS` from the installer
alongside `CONFIGS` and `BINS`, so the two cannot drift again.

### The check that found it

"Every program the config launches must be something the install provides" —
from a keybind *or* from `exec-once`, because both fail the same way and the
second fails more quietly. A program counts as provided if it is in
`packages.txt`, in `bin/`, in `crates/`, or **owned by a manifest package**:
a binary's name is often not its package's, and asking `pacman -Qo` resolved
`dbus-update-activation-environment` to `dbus` and `gsettings` to `glib2`.

Which immediately found two more gaps. Neither `dbus` nor `glib2` was in the
manifest, and the session had just started calling both — so the portal
environment fix and the colour-scheme fix would each have depended on a package
nothing guaranteed. Both are now listed.

---

## 10l. The gate finally ran against a session

Phase 1's gate is *"someone else runs the script and gets your desktop"*, and
its loaded-state half had **never run**. Those checks need a compositor, a
compositor needs a seat, and ssh does not give you one — so `gate.sh` reported
*"Hyprland not running — loaded-state checks skipped"* and the other fifty-odd
passed, which looks exactly like a pass.

`khadi-vm autologin` fixes that: it writes a getty autologin override and a
marked block into the guest's profile so a session starts on tty1. Scaffolding,
written into the guest and nowhere near the repo — Khadi's own session manager
is greetd, which asks for a password on purpose.

**Result: 75 passed, 0 failed**, in a real Khadi session built by
`khadi-install`. Binds loaded 36/36, `gaps_out` in effect, the portal on the
session bus, `XDG_CURRENT_DESKTOP` in the activation environment, and
`prefer-dark` live — the three GUI fixes from section 10j confirmed working
rather than merely configured.

### The file picker opens

The thing section 10j listed as unverified. Asked over D-Bus in the live
session:

```
before: ['foot']
after:  [('foot', '… | MAIN SHELL'), ('xdg-desktop-portal-gtk', 'Khadi verification')]
```

A real GTK file chooser mapped as a window in Hyprland. Routing, backend
capability, config parsing and now the dialog itself.

### Looking at the screen found what 71 passing checks did not

The first screenshot of that session showed three of the four panels dead:

```
khadi-hud: unknown command "header" (want: dash)
khadi-hud: unknown command "net"    (want: dash)
khadi-hud: unknown command "fs"     (want: dash)
```

`(want: dash)` is an error string from two phases ago — a stale binary. **And
the gate said 71/71 while this was on screen**, because it checked that
`khadi-hud` was on PATH and never asked it to draw anything.

**The cause was a bug introduced one commit earlier.** `khadi-vm push` copied
the current binary to `~/.local/bin/khadi-hud`; the new `RUSTBINS` code in
`khadi-install` then replaced that file with a symlink to
`~/khadi/target/release/`, which the tar excludes and had never updated. Two
installers owning one path, the second silently winning and pointing at
something old — the same shape as the gate keeping its own copy of the
installer's lists (10g), created while fixing exactly that class of bug.

`push` now delivers to `target/release/` and leaves installing to
`khadi-install`. One installer.

And the gate now asks each panel to render instead of trusting PATH. Negative
control: a stub emitting the old error string fails all four.

**On PATH is not the same as working** — the general lesson, and the reason
this section exists rather than a one-line fix.

---

## 10m. The boot splash

PLAN.md section 8 has always said the first impression starts at boot, not
after reboot. Until now nothing acted on it: a Khadi machine booted to the
stock Arch console.

`templates/plymouth/khadi/` is a Plymouth **script** theme generated from the
theme file like every other config, installed by `khadi-boot` — invoked from
`khadi-install --system`, which was already the root-level opt-in, so there is
one root entry point rather than two.

### Composed after Omarchy's, not eDEX's

A centred wordmark over a thin progress bar on the theme ground. eDEX's boot
screen is `assets/misc/boot_log.txt`: 85 lines of invented macOS kernel
messages replayed with a delay and a sound per line, opening *"Welcome to
eDEX-UI!"*. A fake log is theatre; a wordmark is just a wordmark.

**One real status line** sits under the bar, which the reference does not have.
It is the one addition worth making — a boot screen that says nothing while it
hangs is a support ticket — and systemd feeds plymouth genuine unit status
through `SetUpdateStatusFunction`, so it costs nothing and it is true.

### No image assets, which forced the wordmark's shape

Plymouth script has no rectangle primitive, so themes ship PNGs for their logos
and bars; Omarchy's ships five. Khadi's wordmark is drawn from characters, so
the splash has no image assets at all and restyles from `themes/*.toml` like
every other config. One generator filter was added, `rgbf`, because plymouth
wants 0..1 floats.

**There is only one font size available.** `Image.Text` takes its size from a
font string, and a font string cannot be resolved inside an initramfs (below),
so the wordmark cannot be set larger than the status line under it — it can
only be built from *more marks*. It is a 5x7 block face with each pixel two
characters wide and the row spacing closed to 13px against a ~16px line height,
which is what turns a sparse dot grid into letters. The leftover texture
between rows reads as dot-matrix, which is the same language as the globe and
the memory grid.

### Three failures, none of which appears in any log

**It came up black.** `Image.Text`'s font argument resolves fine on a running
system and cannot inside an initramfs: mkinitcpio's hook resolves the font with
`fc-match` **on the host** and copies the file in as
`/usr/share/fonts/Plymouth-monospace.ttf` — renamed, with no fontconfig in
there to look a family up by name. Every label came back empty. Omitting the
argument uses the font plymouth actually copied.

**Then the rule came up as tofu.** An initramfs carries only
`label-freetype.so`; `label-pango.so` would drag in pango, harfbuzz and
fontconfig. That renderer draws Latin and returns `.notdef` for everything
multi-byte. **The font was never at fault** — Iosevka Term is correctly baked
in and carries U+2500, verified by extracting the initramfs and querying it.
Everything the splash draws is ASCII for that reason and no other.

**Then the wordmark was a sparse dot grid**, because the first version used one
character per pixel at the default leading. See above.

`plymouth-start` reported active and the journal had no parse error in all
three cases. The only way to find any of them was to screenshot a real boot —
`khadi-vm shot` captures the guest framebuffer over QMP, which works before any
compositor exists. Six VM reboots with a timed capture loop. Worth recording,
because the obvious alternative is rebooting the machine you are working on,
each time, with no way to capture what you saw.

**Glyph coverage moved earlier too.** `khadi-fontcheck` now checks U+2500,
U+252C, U+2588 and U+00B7 alongside U+E0B0, and `khadi-boot` runs it before
baking an initramfs — `fc-match` always answers, and a substitution baked into
an initramfs is a font failure with no console to report it on.

---

## 10n. khadi-greet — the login screen

The last surface wearing someone else's design. tuigreet themes its colours and
draws a box; it cannot draw the bracket-tick header or the block clock, and the
login screen is what you see most often after the splash.

`crates/khadi-greet` speaks greetd's IPC and draws with **khadi-hud's own
widgets** — the clock here and the clock on the system panel are the same code.
That is why `khadi-hud` is now a lib as well as a bin: a second implementation
of either motif would be a second thing to keep in step with the design.

**greetd owns authentication.** This process collects a string, hands it over
and is told yes or no; PAM is never touched here, the string is never logged,
and the buffer is dropped as soon as it is sent. That boundary is what makes a
hand-written greeter a reasonable thing to own at all.

The protocol was read from `greetd-ipc(7)` rather than remembered. Two details
earn their tests: the length prefix is **native** byte order, not network — get
it wrong and the greeter hangs rather than erroring — and the spec is explicit
that *"there are no limits on the number and type of messages that may be
required and a greeter should not make any assumptions"*, so the flow is driven
by what arrives rather than a fixed username-then-password script.

### The VT has sixteen colours. So repaint them.

The greeter runs on a bare Linux console. The shipped greetd config said so and
concluded *"this is the one Khadi surface that cannot be truecolor-themed"* —
and the first working version proved it, coming up plain white on black with
the whole flattened ramp collapsed to one step.

The palette is not fixed, though. OSC `P<n><rrggbb>` redefines a console
palette entry, so `khadi-greet` paints **all sixteen** from the theme's ramp on
startup and lets the console quantise into Khadi's own colours. The widgets
never learn they are on a VT. `\e]R` hands the console back on exit, because
the session that follows is not the greeter's to restyle.

Only on `TERM=linux`. Under a terminal emulator the truecolor is used directly
and repainting someone's palette would be vandalism.

### Two installs, because the greeter is not a user

`greetd` runs the greeter as the `greeter` user, which cannot read anyone's
home directory. A link into `~/.local/bin` is therefore useless to it, and so
is a theme under `$XDG_CONFIG_HOME`. `khadi-install --system` installs the
binary to `/usr/local/bin` and a copy of the resolved theme to
`/etc/khadi/theme.toml`, which `Theme::load` now falls back to.

### A third copy of the same list

`khadi-vm push` kept its own list of which Rust binaries to deliver, so adding
`khadi-greet` to the installer shipped a VM with **greetd enabled and the
greeter binary absent** — the one failure mode that locks you out rather than
looking wrong. It now derives the list from `khadi-install`, like the gate
does. That is the third instance of this exact bug in two days (10g, 10l); the
pattern is worth naming: *any list of what Khadi installs belongs in
khadi-install and nowhere else.*

### Driven for real

`khadi-vm type` sends keystrokes to the guest console over QMP, because the
greeter is the one surface that cannot be driven over ssh — it owns a VT and
reads a keyboard. Without it, "can anyone actually log in" stays untested,
which for a login screen is the only question that matters.

Verified end to end in the VM: username accepted, greetd returned its auth
message, the password masked to its length and nothing else, and
`pam_unix(greetd:session): session opened for user khadi(uid=1000)` — followed
by Hyprland, four panels, the bar and mako. Splash, greeter, desktop.

`greetd-tuigreet` leaves the manifest, as waybar did when khadi-bar landed.

---

## 10o. The lock screen

`Super+Shift+X` ran `hyprlock` with **no config at all** — stock hyprlock,
which is a blurred screenshot and a round input field, and nothing like Khadi.
The keybind had been there since Phase 1 and the check added in 10j did not
catch it, because `hyprlock` really is in the manifest. What was missing was
not the program but the theme.

### Themed, not replaced — unlike the greeter

`khadi-greet` exists because nothing off the shelf could draw the design
language on a login screen. The lock screen is a different calculation and
lands the other way:

- it is what stands between a locked machine and its data
- `hyprlock` is purpose-built for `ext-session-lock-v1` and handles that
  protocol's failure modes, including the one where the locker dies and the
  compositor must stay locked
- section 12 says to resist every feature that moves work from upstream to
  Khadi, and that applies hardest here

So this is a config. hyprlock turns out to carry the motifs fine: it renders
labels through pango with a real font, so the ticked hairline is just text —
the same trick the TUI uses, and the one the boot splash could **not** use
because an initramfs has no pango.

Flat ground rather than the default blurred screenshot. A blur is a different
design language, and it leaves a readable impression of whatever was on screen
when you walked away.

### A third aspect ratio, and the block clock does not survive it

The panel and the greeter draw the clock one glyph per pixel, which works where
a cell is exactly half as wide as it is tall. hyprlock renders through pango,
which adds leading between lines and exposes no way to remove it — so each
pixel becomes a wide rectangle with a gap under it and the digits come out as
crude bars. Doubling the width to compensate trades one distortion for another;
both were tried and photographed before `$TIME` at a large size won.

That is three media with three different constraints on the same motif:

| surface | constraint | result |
| --- | --- | --- |
| `khadi-hud`, `khadi-greet` | cell grid, 1:2 | block digits, exact |
| boot splash | no pango, ASCII only, one font size | block wordmark from `#` |
| `hyprlock` | pango leading, not removable | large `$TIME`, no block digits |

The `khadi-hud clock` subcommand written to feed hyprlock was removed with it
rather than left as a second way to draw a clock that nothing calls.

### Verified by locking and unlocking

The lock was raised in a real session, photographed, and released by typing the
password through `khadi-vm type` — the same QMP keystroke path the greeter
needed, for the same reason: a lock screen cannot be driven over ssh.

One thing learned the hard way: `pkill hyprlock` while the session is locked
leaves Hyprland locked with no locker, and it shows a recovery screen for
exactly that. Not a Khadi bug, and worth knowing before anyone tries to iterate
on this config the obvious way.

**Not done:** nothing locks automatically. There is no idle daemon in the
manifest, so the lock is keybind-only. `hypridle` is the matching piece and is
its own decision about timeouts.

---

## 10p. hypridle — what happens when you walk away

The lock screen in 10o protected a machine you remembered to lock. `hypridle`
is the piece that locks it for you.

### The timeouts, and the one everybody ships that is wrong here

| | |
| --- | --- |
| 10 min | `loginctl lock-session` |
| 11 min | screen off, back on at any input |
| before suspend | lock |

**No automatic suspend**, which is the default nearly every dotfiles repo
ships. A terminal-first machine is routinely left running a build, a VM or a
long ssh session; suspending that because nobody touched the keyboard for half
an hour loses work, and loses it silently. The config carries the stanza
commented out with a note about when to want it.

No backlight dimming either: that needs `brightnessctl`, which is not in the
manifest, and a config calling a program Khadi does not install is the
`chromium` bug from 10j.

### The lock path goes through logind on purpose

A listener calls `loginctl lock-session`, logind emits `Lock`, hypridle catches
it and runs `lock_cmd`. The indirection means `loginctl lock-session` from
anywhere — a script, a lid switch, another tool — locks the same way the idle
timeout does, instead of there being one path that works and one that quietly
does nothing. `lock_cmd` is guarded with `pidof hyprlock ||` because without it
a second lock screen stacks on the first and dismissing one leaves you looking
at the other.

### hypridle's own example is wrong on a Khadi machine

`/usr/share/hypr/hypridle.conf` (hypridle 0.1.8) uses
`hyprctl dispatch 'hl.dsp.dpms({action = "off"})'`. On Khadi that returns
**"Invalid dispatcher"** and the screen simply never turns off, with nothing in
any log.

**This corrected section 10e.** That section recorded Hyprland 0.56 as having
"replaced string dispatch with a Lua API". It has not: 0.56 added a Lua *config
manager*, and `hyprctl dispatch` follows whichever config manager is active.
The development machine runs Omarchy, which uses a Lua config — so from there
the change looks total. Khadi ships a classic `hyprland.conf`, so classic
dispatch is correct on a Khadi machine, and the finding was a measurement taken
on the wrong desktop.

Both forms were measured in the guest:

```
hyprctl dispatch 'hl.dsp.dpms({action = "off"})'   -> Invalid dispatcher, screen stays on
hyprctl dispatch dpms off                          -> ok, framebuffer mean 0.0002
hyprctl dispatch dpms on                           -> ok, framebuffer mean 0.0369
```

### Verified by waiting

A copy of the shipped config with the minutes turned into seconds, run in a real
session: locked at ~7s against a 6s timeout, framebuffer at mean 0.0002 after
the screen-off listener, no errors in hypridle's log. The `pidof` guard was
caught working in that log too — it found an existing hyprlock and declined to
start a second.

---

## 10q. The notifications were already themed. Nobody had looked.

The answer to "add a notification daemon theme" turned out to be that there has
been one since Phase 1: `templates/mako/config.tmpl` sets the ground, the
hairline border, `border-radius=0`, the UI face and three urgency variants off
the ramp.

What was missing was evidence. Section 10b has carried this since Phase 1:

> mako urgency variants untested — the test passed `urgency: <byte 2>` for all
> three levels, so only the base styling is confirmed.

So the variants were written, shipped, and never seen — the same shape as the
fonts in 10e, which rendered in substitutes for a whole phase because nothing
looked.

### Seen, finally

All three sent into a real session and photographed together. They are visibly
distinct and the hierarchy is the one the config describes:

| urgency | border | text |
| --- | --- | --- |
| critical | `ramp.a90`, brightest | `role.text` |
| normal | `role.rule` | `role.text` |
| low | `role.rule_faint`, faintest | `role.text_muted` |

Holding them still needed a trick: the first attempts photographed one
notification, because `default-timeout=6000` expired the other two inside the
ssh round trip. The capture uses the shipped config with only the timeout
changed, so what is in the picture is the real styling and not a mock-up.

The stack also sits clear of `khadi-bar` rather than over it, which was worth
checking: mako is on the `overlay` layer and the bar is on `top`, so nothing
structurally stops a notification covering the clock.

### mako had no validator

Nine apps are themed and `khadi-check` asked eight of them. mako parses its
config **before** it touches Wayland, so it can be asked on a headless box and
in CI — and the two failures are distinguishable: a bad file says "Failed to
parse config", a good one on a headless box complains about Wayland or the
service name instead. It exits 0 either way, so the signal is the message.

Negative control: a colour mako cannot parse, injected into the *template* —
the first attempt edited `config/` and proved nothing, because `khadi-check`
regenerates from templates before validating.

---

## 10r. The terminal was the ceiling, so the UI left the terminal

`khadi-hud` scored well against the reference and still did not look like it,
and the reason was not taste. A terminal cell is about 8x20 px and indivisible.
Everything that follows is a consequence:

| eDEX | what a cell grid can do | measured |
| --- | --- | --- |
| `0.092vh` hairline | `─`, a 2 px line adrift in a 20 px row | 10j |
| United Sans, proportional | one monospace, one weight | 3 |
| SVG file icons | block characters in the shape of a folder | 10s |
| WebGL globe | braille dots | 10i |
| `2.04vh` lattice over the whole UI | nothing — there is no sub-cell | — |

The last row is the giveaway. `grid = "#262828"` has been in `themes/*.toml`
marked `PORTED` since Phase 2 and was never drawn once, because a cell grid has
nowhere to put it.

Section 12 already named the way out — *"no Electron, anywhere, which is why
the Tauri rewrite is the only acceptable eDEX descendant"* — written as a fence
and read here as a plan. `shell/` is that rewrite: React, TypeScript and
Tailwind in a webview, Rust underneath, and every number in `src/index.css`
read out of `edex-ui/src/assets/css` rather than estimated. Both are GPL-3.0,
so it is a port.

**What it cost nothing to build.** `khadi-core` did not change. It was kept
free of UI dependencies in Phase 2 on the grounds that two very different
surfaces consumed it; it now feeds three, and the third is a browser. The same
goes for `themes/*.toml` — the shell reads the resolved theme at runtime and
re-derives nothing, which is the rule from section 4.

**What it costs to run.** A webview is ~100-150 MB RSS against `khadi-hud`'s
~5 MB. xterm.js is not foot: it is slower on heavy output, which is why eDEX
had a reputation for lag. `Super+Return` still opens a real foot window, and
that is the answer for builds and log tails.

**What it ended, 2026-10-08.** `khadi-hud` is deleted — 3,842 lines, the whole
ratatui panel set. `exec-once` starts `khadi-shell`, the zellij `khadi` layout
is gone and zellij is a multiplexer again. `khadi-greet` keeps the clock and
the ticked header in `src/chrome.rs`: it runs on a VT before a session exists,
where there is no compositor to put a webview on, so it is the one surface that
cannot follow. There is nothing left for it to drift against.

**`khadi-bar` is gone too, 2026-10-08.** It drew a second tab strip above a
window that already had one. Its own module doc had the verdict written down
from the day it was built: *"Everything else it shows (clock, CPU, memory,
network) duplicates what khadi-hud already draws, and is here because eDEX's
chrome runs along the top of the screen, not because the data had nowhere else
to go."* The two things it alone could do were Hyprland's workspace tabs, which
moved into the shell's existing tab strip, and the angled `skewX(35deg)` tab —
which it existed to provide *because a cell grid cannot skew*. A webview can.
That argument expired with the TUI, and `gtk4` and `gtk4-layer-shell` leave the
package manifest with it.

**btop goes with them.** It has not been in the layout since khadi-hud replaced
it in Phase 3a, and it was still a package dependency and still had a themed
config installed into `~/.config` for a program nothing ran.

**One trap, paid for with a VM boot.** `cargo build --release` produces a
`khadi-shell` that compiles, links, runs, opens a window — and shows *"Could
not connect to localhost: Connection refused"*, because without the Tauri CLI
the frontend is never embedded and the binary falls back to the dev server. It
has to be `npm run tauri build`. `bin/khadi-install` now says so by name.

---

## 11. Risks and open decisions

| Risk | Why it bites | Mitigation |
| --- | --- | --- |
| **"Omarchy with a theme"** | The chassis is public config. Without something original, Khadi is a fork of someone else's taste | `khadi-hud` plus the pane-centric workflow. Both architectural, not cosmetic |
| **Rolling-release theme breakage** | Any upstream can change its config format with no notice. Ten apps means ten chances a week | **Done.** `khadi-check` renders every theme and runs each app's own validator; `.github/workflows/check.yml` runs it daily on real Arch. Breakage is a failing build, not a user report. Section 10g |
| **Chrome eats the screen** | ~40% of the eDEX screenshot is decoration. Beautiful in a screenshot, cramped on a 13-inch laptop | **Done.** `Alt+f` drops to bare terminal; `Super+F` takes the bar with it. Section 10h |
| **Solo maintenance** | Distros die when one person burns out. eDEX is the cautionary tale — three years, then archived | The whole architecture minimises this. Resist every feature that moves work from upstream to Khadi |
| **Hosting cost and uptime** | A repo users depend on cannot go down. The bill grows with adoption | Cheap object storage plus CDN from day one. Know the per-GB number before launch |
| **NVIDIA on Hyprland** | Still the top source of support load on Wayland distros | Support it, document it, do not contort the product for it |

**Open decisions**

- [ ] Editor default: Helix or Neovim?
- [ ] Name and licence. Check "Khadi" for collisions. GPL-3.0 is the path of
      least friction given eDEX's licence, if any eDEX code or assets are reused.
- [ ] Is this a product or a project? Commercial, donation-backed community, or
      portfolio piece — the answer changes how much the hosting bill and
      support load matter.

---

## 12. Non-goals

The fences that keep this finishable. Each is something a reasonable person
will ask for, and the answer is no.

- **No forking Arch.** A repo and a config layer on top of upstream. The moment
  it maintains its own kernel or base packages, it needs a team.
- **No writing a terminal emulator.** Ghostty, foot, kitty and wezterm are each
  multi-year efforts by serious people. Khadi uses one.
- **No Electron, anywhere.** Including in the optional kiosk package, which is
  why the Tauri rewrite is the only acceptable eDEX descendant.
- **No WebGL globe, no GeoIP.** Two separate reasons, worth separating.
  eDEX used [encom-globe](https://github.com/arscan/encom-globe) — MIT, but
  WebGL + three.js, 975 KB of JS and 940 KB of grid data. A cell grid cannot
  render WebGL at any price, and putting a WebView on the GTK panel to get one
  would reintroduce a browser engine, which is the thing *"no Electron
  anywhere"* is actually about. Separately, eDEX resolved endpoints with
  MaxMind (`netstat.class.js`: *"Prevent geoip lookup attempt until maxminddb
  is loaded"*); GeoLite2 now needs an account and carries redistribution terms
  an ISO should not take on.
  **Both fences still stand, and the globe got built anyway.** What the first
  reading got wrong was treating "no WebGL" as "no globe". WebGL is how eDEX
  drew a turning sphere, not what a turning sphere *is* — and a braille canvas
  has 2x4 subpixels per cell to spend on one. `khadi-hud net` turns a dotted
  globe with no browser engine, no new dependency and no geolocation; it is
  labelled `NO GEOIP` because a pin would be a claim the data does not
  support. See section 10i.
- **No on-screen keyboard.** ~15% of the eDEX screen, useless without a
  touchscreen, on a system whose entire premise is the physical keyboard.
  Already dropped upstream. This fence came down for one commit, on the
  reasoning that an exact replica needs it and that in a webview it is at least
  live rather than inert — and went straight back up on seeing it: a quarter of
  the screen spent on a picture of the thing under your hands. The panels fold
  away now instead, which is the need it was standing in for.
- **No X11.** Wayland only. Supporting both doubles the compositor, screenshot,
  clipboard and screen-share surface for a shrinking audience.
- **No desktop environment features.** No settings GUI, no file manager beyond
  Yazi, no tray-icon ecosystem.
- **No touchscreen support in v1.** The one place eDEX's design genuinely made
  sense, and a distraction from the keyboard-driven premise.

---

## Appendix — reference material

| What | Where |
| --- | --- |
| eDEX-UI source (archived upstream) | `edex-ui/` |
| Reference screenshot | `edex-ui/media/screenshot_default.png` |
| Upstream theme schema | `edex-ui/src/assets/themes/*.json` (21 themes) |
| The chrome motifs | `edex-ui/src/assets/css/main.css`, `mod_cpuinfo.css` |
| Extracted design tokens | `themes/tron.toml` |
| Maintained Tauri rewrite | https://github.com/zluo01/edex-ui |
| Distro model to copy | https://github.com/omacom/omarchy, https://github.com/omacom/omarchy-iso |
