"""Render an exported Ratatui test buffer as a dependency-free SVG preview."""
import argparse
import html
import json
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("input", type=Path)
parser.add_argument("output", type=Path)
args = parser.parse_args()
snapshot = json.loads(args.input.read_text())
cols, rows, cells = snapshot["width"], snapshot["height"], snapshot["cells"]
assert len(cells) == cols * rows
cell_width, cell_height, margin, top = 10, 22, 24, 68
width, height = cols * cell_width + margin * 2, rows * cell_height + top + margin
names = {"reset": None, "black": "#282a36", "red": "#ff5555",
         "green": "#50fa7b", "yellow": "#f1fa8c", "blue": "#bd93f9",
         "magenta": "#ff79c6", "cyan": "#8be9fd", "gray": "#f8f8f2",
         "darkgray": "#6272a4", "white": "#f8f8f2"}

def color(value, default):
    if value.startswith("#"):
        return value
    if value.lower() not in names:
        raise ValueError(f"Unsupported preview color: {value}")
    return names[value.lower()] or default

background = color(snapshot.get("background", "reset"), "#282a36")
foreground = color(snapshot.get("foreground", "reset"), "#f8f8f2")
muted = color(snapshot.get("muted", "reset"), foreground)
selection = color(snapshot.get("selection_bg", "reset"), "#44475a")
red = color(snapshot.get("critical", "red"), foreground)
yellow = color(snapshot.get("warning", "yellow"), foreground)
green = color(snapshot.get("cpu", "green"), foreground)
pieces = [f'''<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}" role="img" aria-label="Fictional ures TUI: configurable tabs, semantic metric colors and a continuous selected row">
<rect width="{width}" height="{height}" rx="14" fill="{background}"/>
<circle cx="28" cy="26" r="5" fill="{red}"/><circle cx="48" cy="26" r="5" fill="{yellow}"/><circle cx="68" cy="26" r="5" fill="{green}"/>
<text x="{width-24}" y="31" text-anchor="end" font-family="monospace" font-size="13" fill="{muted}">ures · illustrative user snapshot</text>
<path d="M0 48H{width}" stroke="{selection}"/>''']
# Background runs preserve the exact cell coverage, including separator spaces.
for row in range(rows):
    start = 0
    while start < cols:
        bg = color(cells[row * cols + start]["bg"], background)
        end = start + 1
        while end < cols and color(cells[row * cols + end]["bg"], background) == bg:
            end += 1
        if bg != background:
            pieces.append(f'<rect x="{margin+start*cell_width}" y="{top+row*cell_height}" width="{(end-start)*cell_width}" height="{cell_height}" fill="{bg}"/>')
        start = end
pieces.append('<g font-family="DejaVu Sans Mono,monospace" font-size="16" xml:space="preserve">')
for index, cell in enumerate(cells):
    if not cell["text"].strip():
        continue
    x = margin + (index % cols) * cell_width
    y = top + (index // cols) * cell_height + 17
    ink = color(cell["fg"], foreground)
    weight = ' font-weight="bold"' if cell["bold"] else ""
    pieces.append(f'<text x="{x}" y="{y}" fill="{ink}"{weight}>{html.escape(cell["text"])}</text>')
pieces.append('</g></svg>\n')
args.output.parent.mkdir(parents=True, exist_ok=True)
args.output.write_text("\n".join(pieces))
