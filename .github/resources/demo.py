#!/usr/bin/env python3
"""Record and render the README demo gif.

Capture: runs `target/release/mtui` under a PTY against the built-in mock
device (fresh XDG_CONFIG_HOME, so the auto-created "demo" config is used),
scripts a keyboard tour of the main features and snapshots the terminal
screen (via pyte) with timestamps into `target/gifwork/frames.json.gz`.

Render: dedups identical screens, renders each frame as absolutely-
positioned text in HTML, screenshots the frames in batches with headless
Chromium, then slices and assembles `.github/resources/demo.gif` with
ImageMagick, preserving the captured timing. Text is laid out on an exact
character grid: DejaVu Sans Mono covers everything the TUI prints except
braille (graph) and U+27F3, which fall back to FreeMono in per-cell pinned
spans so the grid never drifts.

Usage:
    cargo build --release
    python3 .github/resources/demo.py                # capture + render
    python3 .github/resources/demo.py --render-only  # reuse cached frames
    python3 .github/resources/demo.py --debug        # dump scene screens

Needs: pip install pyte; chromium; imagemagick.
"""

import argparse
import gzip
import html
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
BIN = REPO / 'target' / 'release' / 'mtui'
WORK = REPO / 'target' / 'gifwork'
FRAMES = WORK / 'frames.json.gz'
OUT = REPO / '.github' / 'resources' / 'demo.gif'

COLS, ROWS = 110, 30
FONT = 16
CW = FONT * 1233 / 2048          # DejaVu Sans Mono glyph advance at 16px
LH = 20                          # line height
PAD = 8                          # frame padding
FW = round(2 * PAD + COLS * CW)  # frame pixel size
FH = 2 * PAD + ROWS * LH
BATCH = 20                       # frames per chromium screenshot (height cap)
LAST_HOLD = 1.6                  # seconds to hold the final frame

# xterm-256 colors as resolved by pyte -> the gif's palette
PALETTE = {
    'default_fg': '#c9cdd1',
    'default_bg': '#1e2021',
    '000000': '#111415',
    '262626': '#282b2d',   # zebra stripe (color 235)
    '7f7f7f': '#5a6165',   # dim
    '00ff00': '#48e63a',   # accent green
    'ffffff': '#eceff1',
    'cdcd00': '#dfcd71',   # yellow
    'ffff00': '#f2df7a',   # bright yellow
    'ff0000': '#ef5b5b',   # red
    '5c5cff': '#5f9bff',   # bright blue
}

UP, DOWN, LEFT, RIGHT = '\x1b[A', '\x1b[B', '\x1b[D', '\x1b[C'
ESC, ENTER = '\x1b', '\r'


# ---------------------------------------------------------------- capture

def capture(debug):
    import fcntl
    import pty
    import select
    import signal
    import struct
    import termios
    import time

    try:
        import pyte
    except ImportError:
        sys.exit('missing dependency: pip install pyte')

    if not BIN.exists():
        sys.exit(f'{BIN} not found — run `cargo build --release` first')

    WORK.mkdir(parents=True, exist_ok=True)
    config_dir = WORK / 'cfg'
    shutil.rmtree(config_dir, ignore_errors=True)
    config_dir.mkdir()

    pid, master = pty.fork()
    if pid == 0:  # child: become the TUI
        os.chdir(WORK)  # away from any config.json in the repo root
        os.environ['XDG_CONFIG_HOME'] = str(config_dir)
        os.environ['TERM'] = 'xterm-256color'
        os.environ['COLORTERM'] = 'truecolor'
        os.execv(str(BIN), [str(BIN)])

    fcntl.ioctl(master, termios.TIOCSWINSZ,
                struct.pack('HHHH', ROWS, COLS, 0, 0))

    screen = pyte.Screen(COLS, ROWS)
    stream = pyte.ByteStream(screen)
    start = time.time()
    frames = []
    raw_tail = b''

    def feed(data):
        """Feed TUI output to pyte, answering the cursor-position query
        (the app blocks before its first frame until it gets a reply)."""
        nonlocal raw_tail
        stream.feed(data)
        raw_tail += data
        if b'\x1b[6n' in raw_tail:
            os.write(master, b'\x1b[15;55R')
        raw_tail = raw_tail[-8:]

    def pump(dur):
        end = time.time() + dur
        while True:
            left = end - time.time()
            if left <= 0:
                return
            r, _, _ = select.select([master], [], [], left)
            if master in r:
                try:
                    data = os.read(master, 65536)
                except OSError:
                    return
                if not data:
                    return
                feed(data)

    def snap():
        """Record the screen as a grid of [char, fg, bg, bold, reverse]."""
        grid = []
        for y in range(ROWS):
            row = screen.buffer[y]
            grid.append([[c.data or ' ', c.fg, c.bg,
                          1 if c.bold else 0, 1 if c.reverse else 0]
                         for c in (row[x] for x in range(COLS))])
        frames.append({'t': round(time.time() - start, 3), 'grid': grid})

    def watch(dur, interval=0.34):
        """Let the app run for `dur` seconds, snapping every `interval`."""
        n = max(1, round(dur / interval))
        for _ in range(n):
            pump(dur / n)
            snap()

    def key(s, settle=0.30):
        os.write(master, s.encode())
        pump(settle)
        snap()

    def checkpoint(name):
        print(f'scene: {name} (t={time.time() - start:.1f}s)')
        if debug:
            for line in screen.display:
                print('|' + line + '|')
        sys.stdout.flush()

    # ---- Scene 1: startup ----
    watch(2.2)
    checkpoint('startup')

    # ---- Scene 2: read a few registers ----
    for _ in range(5):
        key(DOWN, 0.22)
    watch(1.4)
    checkpoint('live values')

    # ---- Scene 3: help popup (brief) ----
    key('h', 0.4)
    watch(1.0)
    key(DOWN, 0.30)
    watch(0.5)
    checkpoint('help')
    key(ESC, 0.4)

    # ---- Scene 4: graph on input 0 "voltage L1" ----
    for _ in range(10):
        key(UP, 0.10)
    watch(0.4)
    key('g', 0.4)
    watch(3.0)
    checkpoint('graph')
    key('g', 0.4)
    watch(0.4)

    # ---- Scene 5: jump to the "seconds" register by label ----
    key('j', 0.45)
    for c in 'sec':
        key(c, 0.15)
    watch(0.8)
    checkpoint('jump')
    key(ENTER, 0.45)
    watch(1.2)

    # ---- Scene 6: custom rule on "seconds": /60, 1 decimal, " min" ----
    key('m', 0.45)
    watch(0.9)
    key(DOWN, 0.25)          # -> Operations
    for c in '/60':
        key(c, 0.14)
    key(ENTER, 0.45)         # add op
    for _ in range(3):
        key(DOWN, 0.25)      # -> Decimals
    key('1', 0.3)
    for _ in range(2):
        key(DOWN, 0.25)      # -> Suffix
    for c in ' min':
        key(c, 0.14)
    watch(0.7)
    checkpoint('custom rule')
    key(ENTER, 0.45)         # save & close
    watch(1.6)

    # ---- Scene 7: settings, toggle Display -> "Show frame render time" ----
    key('s', 0.5)
    watch(0.7)
    for _ in range(3):
        key(DOWN, 0.25)      # -> Display
    key(ENTER, 0.4)
    watch(0.5)
    key(DOWN, 0.3)           # -> Show frame render time
    key(RIGHT, 0.5)          # toggle on
    watch(0.7)
    checkpoint('settings toggle')
    key(ESC, 0.35)
    key(ESC, 0.45)
    watch(2.0)

    # ---- Scene 8: about popup as closing shot ----
    key('a', 0.45)
    watch(2.0)
    checkpoint('about finale')

    FRAMES.write_bytes(gzip.compress(json.dumps(frames).encode()))
    print(f'captured {len(frames)} snapshots'
          f' over {frames[-1]["t"]:.1f}s -> {FRAMES}')

    os.kill(pid, signal.SIGKILL)


# ----------------------------------------------------------------- render

def resolve(cell):
    """Map a [char, fg, bg, bold, reverse] cell to (#fg, #bg-or-None)."""
    _, fg, bg, _, reverse = cell
    if reverse:
        fg, bg = bg, fg
        fg_res = PALETTE['default_bg'] if fg == 'default' \
            else PALETTE.get(fg, '#' + fg)
        bg_res = PALETTE['default_fg'] if bg == 'default' \
            else PALETTE.get(bg, '#' + bg)
        return fg_res, bg_res
    fg_res = PALETTE['default_fg'] if fg == 'default' \
        else PALETTE.get(fg, '#' + fg)
    bg_res = None if bg == 'default' else PALETTE.get(bg, '#' + bg)
    return fg_res, bg_res


def needs_fallback_font(ch):
    """True for glyphs missing from DejaVu Sans Mono (drawn via FreeMono)."""
    o = ord(ch)
    return 0x2800 <= o <= 0x28ff or o == 0x27f3


def frame_html(fr, top):
    """One frame: a background-rect layer, then a text-run layer, all
    absolutely positioned on the character grid."""
    grid = fr['grid']
    out = [f'<div class="fr" style="top:{top}px">']

    for y, row in enumerate(grid):
        x = 0
        while x < COLS:
            bg = resolve(row[x])[1]
            if bg is None:
                x += 1
                continue
            x0 = x
            while x < COLS and resolve(row[x])[1] == bg:
                x += 1
            left = round(PAD + x0 * CW)
            width = round(PAD + x * CW) - left
            out.append(f'<i style="left:{left}px;top:{PAD + y * LH}px;'
                       f'width:{width}px;background:{bg}"></i>')

    for y, row in enumerate(grid):
        x = 0
        while x < COLS:
            cell = row[x]
            if cell[0] == ' ':
                x += 1
                continue
            fg = resolve(cell)[0]
            bold = cell[3]
            fallback = needs_fallback_font(cell[0])
            x0, text = x, ''
            while x < COLS:
                c = row[x]
                if (c[0] == ' ' or resolve(c)[0] != fg or c[3] != bold
                        or needs_fallback_font(c[0]) != fallback):
                    break
                text += c[0]
                x += 1
                if fallback and len(text) >= 8:
                    break  # re-pin long fallback runs so widths can't drift
            style = (f'left:{PAD + x0 * CW:.2f}px;'
                     f'top:{PAD + y * LH}px;color:{fg}')
            if bold:
                style += ';font-weight:bold'
            if fallback:
                style += ";font-family:'FreeMono',monospace"
            out.append(f'<b style="{style}">{html.escape(text)}</b>')

    out.append('</div>')
    return ''.join(out)


def render():
    chromium = next((c for c in ('chromium-browser', 'chromium',
                                 'google-chrome')
                     if shutil.which(c)), None)
    if not chromium:
        sys.exit('chromium not found')
    if not shutil.which('convert'):
        sys.exit('imagemagick (convert) not found')
    if not FRAMES.exists():
        sys.exit(f'{FRAMES} not found — capture first')

    frames = json.loads(gzip.decompress(FRAMES.read_bytes()))

    # durations from capture timestamps; merge consecutive identical screens
    seq = []
    for i, fr in enumerate(frames):
        dur = frames[i + 1]['t'] - fr['t'] if i + 1 < len(frames) \
            else LAST_HOLD
        key = json.dumps(fr['grid'])
        if seq and seq[-1]['key'] == key:
            seq[-1]['dur'] += dur
            continue
        seq.append({'grid': fr['grid'], 'dur': dur, 'key': key})
    seq[-1]['dur'] = LAST_HOLD
    print(f'{len(seq)} unique frames, {sum(f["dur"] for f in seq):.1f}s')

    css = f"""<meta charset="utf-8"><style>
* {{ margin:0; padding:0; }}
body {{ background:{PALETTE['default_bg']}; }}
.fr {{ position:absolute; left:0; width:{FW}px; height:{FH}px;
      overflow:hidden; background:{PALETTE['default_bg']};
      font:{FONT}px/{LH}px 'DejaVu Sans Mono',monospace; }}
.fr i {{ position:absolute; height:{LH}px; }}
.fr b {{ position:absolute; white-space:pre; font-weight:normal; }}
</style>"""

    png_dir = WORK / 'png'
    shutil.rmtree(png_dir, ignore_errors=True)
    png_dir.mkdir(parents=True)

    batches = [seq[i:i + BATCH] for i in range(0, len(seq), BATCH)]
    for bi, batch in enumerate(batches):
        page = css + ''.join(frame_html(fr, j * FH)
                             for j, fr in enumerate(batch))
        hpath = WORK / f'batch{bi}.html'
        hpath.write_text(page)
        shot = WORK / f'batch{bi}.png'
        r = subprocess.run(
            [chromium, '--headless=new', '--disable-gpu', '--hide-scrollbars',
             '--force-device-scale-factor=1',
             '--default-background-color=FF1E2021',
             f'--window-size={FW},{len(batch) * FH}',
             f'--screenshot={shot}', hpath.as_uri()],
            capture_output=True, text=True, timeout=120)
        if not shot.exists():
            sys.exit(f'chromium failed: {r.stderr[-2000:]}')
        subprocess.run(['convert', str(shot), '-crop', f'{FW}x{FH}',
                        '+repage', str(png_dir / f'b{bi}_%02d.png')],
                       check=True)
        print(f'batch {bi + 1}/{len(batches)} rendered')

    args = ['convert', '-loop', '0']
    for bi, batch in enumerate(batches):
        for j, fr in enumerate(batch):
            args += ['-delay', str(max(3, round(fr['dur'] * 100))),
                     str(png_dir / f'b{bi}_{j:02d}.png')]
    args += ['-layers', 'Optimize', str(OUT)]
    subprocess.run(args, check=True)
    print(f'{OUT}  {OUT.stat().st_size / 1e6:.2f} MB, {len(seq)} frames')


if __name__ == '__main__':
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument('--render-only', action='store_true',
                    help='skip capture, reuse cached frames.json.gz')
    ap.add_argument('--debug', action='store_true',
                    help='dump the screen at each scene checkpoint')
    opts = ap.parse_args()
    if not opts.render_only:
        capture(opts.debug)
    render()
