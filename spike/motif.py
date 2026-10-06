#!/usr/bin/env python3
"""
Khadi Phase 0 motif spike — THROWAWAY.

Paints the eDEX panel layout as literal terminal cells using themes/tron.toml,
so the bracket-tick motif can be judged at real terminal density before any
Rust is written. Not khadi-hud. Not a design for khadi-hud. A ruler.

  python3 spike/motif.py                      # two-row motif (true to the CSS)
  python3 spike/motif.py --variant one-row    # ┬──┬ stubs, saves a line
  python3 spike/motif.py --variant plain      # no ticks, control group
  python3 spike/motif.py --cols 160 --rows 48

Compare against edex-ui/media/screenshot_default.png
"""
import argparse, os, re, shutil, socket, sys, time, tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# ---------------------------------------------------------------- theme ----
def load_theme():
    t = tomllib.load(open(ROOT / 'themes' / 'tron.toml', 'rb'))
    def rgb(h):
        h = h.lstrip('#')
        return tuple(int(h[i:i+2], 16) for i in (0, 2, 4))
    r = t['role']
    return {k: rgb(r[k]) for k in
            ('text', 'text_dim', 'text_muted', 'rule', 'rule_faint', 'ground')}

# ---------------------------------------------------------------- screen ---
class Screen:
    def __init__(self, w, h, bg):
        self.w, self.h, self.bg = w, h, bg
        self.ch = [[' '] * w for _ in range(h)]
        self.fg = [[None] * w for _ in range(h)]
        self.chrome = [False] * h          # rows that are pure chrome

    def put(self, x, y, s, fg, clip=None):
        if not (0 <= y < self.h):
            return
        if clip is not None:
            s = s[:max(0, clip)]
        for i, c in enumerate(s):
            if 0 <= x + i < self.w:
                self.ch[y][x + i] = c
                self.fg[y][x + i] = fg

    def hline(self, x, y, n, fg, ch='─'):
        self.put(x, y, ch * n, fg)

    def mark_chrome(self, y):
        if 0 <= y < self.h:
            self.chrome[y] = True

    def render(self):
        br, bg_, bb = self.bg
        out = [f'\033[48;2;{br};{bg_};{bb}m']
        for y in range(self.h):
            cur = None
            row = []
            for x in range(self.w):
                f = self.fg[y][x]
                if f != cur:
                    row.append('\033[38;2;%d;%d;%dm' % f if f else '\033[39m')
                    cur = f
                row.append(self.ch[y][x])
            out.append(''.join(row) + '\033[K\n')
        out.append('\033[0m')
        return ''.join(out)

# ----------------------------------------------------------------- motif ---
def header(sc, x, y, w, left, right, C, variant):
    """The eDEX panel header: label pair, hairline, bracket ticks.
    Returns the number of rows consumed."""
    sc.put(x, y, left, C['text'])
    sc.put(x + w - len(right), y, right, C['text_dim'])
    if variant == 'one-row':
        sc.put(x, y + 1, '┬' + '─' * (w - 2) + '┬', C['rule'])
        sc.mark_chrome(y + 1)
        return 2
    sc.hline(x, y + 1, w, C['rule'])
    sc.mark_chrome(y + 1)
    if variant == 'two-row':
        sc.put(x, y + 2, '╷' + ' ' * (w - 2) + '╷', C['rule'])
        sc.mark_chrome(y + 2)
        return 3
    return 2

# ------------------------------------------------------------------ data ---
def read(p, default=''):
    try:
        return Path(p).read_text().strip()
    except Exception:
        return default

def cpu_sample():
    vals = []
    for ln in read('/proc/stat').splitlines():
        if ln.startswith('cpu') and ln[3].isdigit():
            f = [int(v) for v in ln.split()[1:8]]
            vals.append((sum(f), f[3]))
    return vals

def sysdata():
    d = {}
    d['host'] = socket.gethostname()
    up = float(read('/proc/uptime', '0 0').split()[0])
    d['uptime'] = '%dd %dh %dm' % (up // 86400, (up % 86400) // 3600, (up % 3600) // 60)
    mi = {}
    for ln in read('/proc/meminfo').splitlines():
        k, _, v = ln.partition(':')
        mi[k] = int(v.split()[0]) if v.split() else 0
    d['mem_total'] = mi.get('MemTotal', 0) / 1048576
    d['mem_used'] = (mi.get('MemTotal', 0) - mi.get('MemAvailable', 0)) / 1048576
    d['swap_total'] = mi.get('SwapTotal', 0) / 1048576
    d['swap_used'] = (mi.get('SwapTotal', 0) - mi.get('SwapFree', 0)) / 1048576
    model = ''
    for ln in read('/proc/cpuinfo').splitlines():
        if 'model name' in ln:
            model = ln.split(':', 1)[1].strip(); break
    d['cpu_model'] = re.sub(r'\s*\(R\)|\s*\(TM\)|\s*CPU|\s*@.*', '', model)[:30]
    a, b = cpu_sample(), (time.sleep(0.12), cpu_sample())[1]
    d['cores'] = []
    for (t0, i0), (t1, i1) in zip(a, b):
        dt, di = t1 - t0, i1 - i0
        d['cores'].append(100.0 * (dt - di) / dt if dt else 0.0)
    d['vendor'] = read('/sys/class/dmi/id/sys_vendor', 'UNKNOWN')[:24]
    d['model'] = read('/sys/class/dmi/id/product_name', 'UNKNOWN')[:16]
    d['chassis'] = read('/sys/class/dmi/id/chassis_type', '—')
    temps = []
    for z in sorted(Path('/sys/class/thermal').glob('thermal_zone*/temp')):
        try: temps.append(int(z.read_text()) / 1000)
        except Exception: pass
    d['temp'] = max(temps) if temps else 0
    procs = []
    for pd in Path('/proc').iterdir():
        if pd.name.isdigit():
            st = read(pd / 'stat')
            if not st: continue
            try:
                nm = st[st.index('(') + 1:st.rindex(')')]
                f = st[st.rindex(')') + 2:].split()
                procs.append((int(f[11]) + int(f[12]), pd.name, nm))
            except Exception: pass
    procs.sort(reverse=True)
    d['procs'] = procs[:6]
    d['ntasks'] = len([p for p in Path('/proc').iterdir() if p.name.isdigit()])
    rx = tx = 0
    for ln in read('/proc/net/dev').splitlines()[2:]:
        name, _, rest = ln.partition(':')
        if name.strip() == 'lo': continue
        f = rest.split()
        if len(f) >= 9: rx += int(f[0]); tx += int(f[8])
    d['rx'], d['tx'] = rx / 1048576, tx / 1048576
    return d

# ----------------------------------------------------------------- parts ---
DIG = {
 '0': ["█████","█   █","█   █","█   █","█████"], '1': ["   ██","   ██","   ██","   ██","   ██"],
 '2': ["█████","    █","█████","█    ","█████"], '3': ["█████","    █","█████","    █","█████"],
 '4': ["█   █","█   █","█████","    █","    █"], '5': ["█████","█    ","█████","    █","█████"],
 '6': ["█████","█    ","█████","█   █","█████"], '7': ["█████","    █","    █","    █","    █"],
 '8': ["█████","█   █","█████","█   █","█████"], '9': ["█████","█   █","█████","    █","█████"],
 ':': ["     ","  █  ","     ","  █  ","     "],
}
SPARK = '▁▂▃▄▅▆▇█'

def spark(vals, n):
    vals = vals[-n:] or [0]
    return ''.join(SPARK[min(7, int(v / 12.5))] for v in vals).rjust(n)

def bar(frac, n):
    filled = int(round(frac * n))
    return '█' * filled + '░' * (n - filled)

# ------------------------------------------------------------------ main ---
def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--variant', default='two-row',
                    choices=['two-row', 'one-row', 'plain'])
    ap.add_argument('--cols', type=int, default=0)
    ap.add_argument('--rows', type=int, default=0)
    a = ap.parse_args()

    ts = shutil.get_terminal_size((160, 48))
    W = a.cols or max(120, ts.columns)
    H = a.rows or max(40, ts.lines - 1)
    C = load_theme()
    d = sysdata()
    sc = Screen(W, H, C['ground'])
    V = a.variant

    FS_H = 7                     # bottom filesystem strip
    LW = 34                      # left column width
    RW = 34                      # right column width
    CX = LW + 3                  # centre x
    CW = W - LW - RW - 6         # centre width
    RX = W - RW - 1
    y0 = 1

    # ---- LEFT COLUMN ----
    y = y0
    y += header(sc, 1, y, LW, 'PANEL', 'SYSTEM', C, V)
    clock = time.strftime('%H:%M')   # seconds dropped: see PLAN.md Phase 0
    for r in range(5):
        sc.put(1, y + r, ' '.join(DIG[c][r] for c in clock), C['text'], clip=LW)
    y += 6
    sc.put(1, y, time.strftime('%Y'), C['text_dim'])
    sc.put(9, y, 'UPTIME', C['text_dim']); sc.put(21, y, 'TYPE', C['text_dim'])
    sc.put(28, y, 'POWER', C['text_dim']); y += 1
    sc.put(1, y, time.strftime('%b %d').upper(), C['text'])
    sc.put(9, y, d['uptime'], C['text']); sc.put(21, y, 'linux', C['text'])
    sc.put(28, y, 'CHARGE', C['text']); y += 2

    y += header(sc, 1, y, LW, 'MANUFACTURER', 'MODEL', C, V)
    sc.put(1, y, d['vendor'][:20], C['text'], clip=LW)
    sc.put(1 + LW - len(d['model']), y, d['model'], C['text']); y += 2

    y += header(sc, 1, y, LW, 'CPU USAGE', d['cpu_model'][:18], C, V)
    hist = [[max(0, c - 18 + (i * 7) % 30) for i in range(26)] for c in d['cores']]
    for i in range(0, min(4, len(d['cores'])), 2):
        sc.put(1, y, f'#{i+1}-{i+2}', C['text'])
        avg = (d['cores'][i] + d['cores'][min(i+1, len(d['cores'])-1)]) / 2
        sc.put(1, y + 1, f'{avg:4.1f}%', C['text_muted'])
        sc.put(10, y, spark(hist[i], 24), C['text'])
        sc.put(10, y + 1, spark(hist[min(i+1, len(hist)-1)], 24), C['text'])
        y += 2
    y += 1

    y += header(sc, 1, y, LW, 'TEMP', 'TASKS', C, V)
    sc.put(1, y, f"{d['temp']:.0f}°C" if d['temp'] else '—', C['text'])
    sc.put(12, y, f"{len(d['cores'])} CORES", C['text'])
    sc.put(LW - 5, y, f"{d['ntasks']:>4}", C['text']); y += 2

    y += header(sc, 1, y, LW, 'MEMORY',
                f"{d['mem_used']:.1f}/{d['mem_total']:.1f} GiB", C, V)
    cells = LW * 4
    used_cells = int(cells * d['mem_used'] / max(d['mem_total'], 0.001))
    for r in range(4):
        row = ''.join('▀' if r * LW + i < used_cells else '·' for i in range(LW))
        sc.put(1, y + r, row, C['text'] if r * LW < used_cells else C['rule'])
    y += 5
    sc.put(1, y, 'SWAP', C['text_dim'])
    sc.put(7, y, bar(d['swap_used'] / max(d['swap_total'], 0.001), LW - 14), C['rule'])
    sc.put(LW - 6, y, f"{d['swap_used']:.1f} GiB", C['text_muted']); y += 2

    y += header(sc, 1, y, LW, 'TOP PROCESSES', 'PID | NAME | CPU', C, V)
    for _, pid, nm in d['procs']:
        if y >= H - FS_H - 1: break
        sc.put(1, y, f'{pid:>6}', C['text_muted'])
        sc.put(8, y, nm[:16], C['text'])
        sc.put(LW - 5, y, f'{(hash(nm) % 90) / 10:4.1f}%', C['text_dim'])
        y += 1

    # ---- CENTRE ----
    y = y0
    y += header(sc, CX, y, CW, 'TERMINAL', 'MAIN SHELL', C, V)
    tabw = CW // 5
    for i in range(5):
        lbl = 'MAIN SHELL' if i == 0 else 'EMPTY'
        col = C['text'] if i == 0 else C['text_muted']
        sc.put(CX + i * tabw + (tabw - len(lbl)) // 2, y, lbl, col)
    sc.hline(CX, y + 1, CW, C['rule']); sc.mark_chrome(y + 1)
    y += 3
    lines = [
        (f"{d['host']}@khadi", C['text']),
        ('─' * 28, C['rule']),
        (f"OS: Arch Linux x86_64", C['text_dim']),
        (f"Kernel: {os.uname().release}", C['text_dim']),
        (f"Uptime: {d['uptime']}", C['text_dim']),
        (f"Shell: {os.environ.get('SHELL','sh').split('/')[-1]}", C['text_dim']),
        (f"WM: Hyprland", C['text_dim']),
        (f"Terminal: khadi-spike", C['text_dim']),
        (f"CPU: {d['cpu_model']} ({len(d['cores'])})", C['text_dim']),
        (f"Memory: {d['mem_used']*1024:.0f}MiB / {d['mem_total']*1024:.0f}MiB", C['text_dim']),
        ('', C['text']),
    ]
    for s, col in lines:
        sc.put(CX + 2, y, s, col, clip=CW - 2); y += 1
    sc.put(CX, y + 1, '~/software/personal/khadi', C['text'])
    sc.put(CX + 26, y + 1, '▊', C['text'])

    # ---- RIGHT COLUMN ----
    y = y0
    y += header(sc, RX, y, RW, 'PANEL', 'NETWORK', C, V)
    sc.put(RX, y, 'NETWORK STATUS', C['text'])
    sc.put(RX + RW - 9, y, 'Iface wlan', C['text_muted']); y += 1
    sc.put(RX, y, 'STATE', C['text_dim']); sc.put(RX + 12, y, 'IPv4', C['text_dim'])
    sc.put(RX + RW - 4, y, 'PING', C['text_dim']); y += 1
    sc.put(RX, y, 'ONLINE', C['text']); sc.put(RX + 12, y, '10.0.0.14', C['text'])
    sc.put(RX + RW - 5, y, '16ms', C['text']); y += 2

    y += header(sc, RX, y, RW, 'NETWORK TRAFFIC', 'UP / DOWN', C, V)
    sc.put(RX, y, 'TOTAL', C['text_dim'])
    sc.put(RX + 8, y, f"{d['tx']:.0f} MB OUT  {d['rx']:.0f} MB IN", C['text_muted'], clip=RW - 8)
    y += 2
    up = [abs(hash((i, 'u'))) % 100 for i in range(RW)]
    dn = [abs(hash((i, 'd'))) % 100 for i in range(RW)]
    sc.put(RX, y, spark(up, RW), C['text']); y += 1
    sc.put(RX, y, spark(dn, RW), C['text_dim']); y += 2

    y += header(sc, RX, y, RW, 'WORLD VIEW', 'GLOBAL MAP', C, V)
    sc.put(RX, y, '(dropped — see non-goals)', C['text_muted'], clip=RW); y += 2

    # ---- BOTTOM: FILESYSTEM ----
    fy = H - FS_H
    fy += header(sc, 1, fy, W - 2, 'FILESYSTEM', str(ROOT), C, V)
    try:
        entries = sorted(p for p in ROOT.iterdir())[:8]
    except Exception:
        entries = []
    for i, p in enumerate(entries):
        x = 1 + i * 18
        if x + 16 > W: break
        sc.put(x, fy, '▓▓' if p.is_dir() else '▒ ', C['text'])
        sc.put(x + 3, fy, p.name[:14], C['text_dim'])

    sys.stdout.write(sc.render())

    chrome_rows = sum(sc.chrome)
    print(f"\n  variant={V}  grid={W}x{H}  "
          f"chrome rows={chrome_rows} ({100*chrome_rows/H:.0f}% of height)  "
          f"headers={sum(sc.chrome)//(2 if V=='two-row' else 1)}")
    print("  compare: edex-ui/media/screenshot_default.png")

if __name__ == '__main__':
    main()
