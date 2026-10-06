# khadi-fonts

The faces Khadi's themes name, vendored because they are not in Arch's
official repositories and a Khadi install cannot depend on the AUR.

| File | Family | Role | Licence |
| --- | --- | --- | --- |
| `Rajdhani-Light.ttf` | Rajdhani Light | `font.ui_light` | OFL 1.1 |
| `Rajdhani-Regular.ttf` | Rajdhani | — | OFL 1.1 |
| `Rajdhani-Medium.ttf` | Rajdhani Medium | `font.ui` | OFL 1.1 |
| `Orbitron[wght].ttf` | Orbitron | `font.display` — the clock | OFL 1.1 |

`font.mono` is **Iosevka Term**, which ships as `extra/ttc-iosevka` and is a
plain package dependency. Phase 0 measured that package at 446 MiB for 189
faces when Khadi needs two; a 20.7 MiB subset replaces it in Phase 4.

## Why these are here rather than in Phase 4

They were filed as packaging work, on the assumption fonts only matter at
distribution time. They do not.

eDEX's own two slots were United Sans Medium and Light — a commercial House
Industries face, fine inside a GPL source tree and not fine on installable
media. Khadi substitutes OFL equivalents, and Phase 0 locked them.

Then Phase 3b ran `khadi-bar` on real hardware and found that **neither
Rajdhani nor Orbitron was installed anywhere** — not on the development
machine, not in the test VM. Every screenshot until that point had rendered in
fallback faces, and nothing noticed, because **a missing font is not an
error**: fontconfig silently substitutes the nearest match and reports
success. The gate passed, `khadi-check` passed, and the typography was wrong
everywhere.

So the vendoring is half the fix. The other half is `khadi-check`'s font
resolution test, which asks fontconfig what it would actually use for each
family the theme names and fails when the answer is something else.

## Licences

OFL 1.1, reproduced in full alongside the files. Both permit redistribution
and bundling; neither permits selling the fonts on their own.

- Rajdhani — Indian Type Foundry
- Orbitron — The Orbitron Project Authors
