"""Render the fictional `cargo run --example preview` output as a portable SVG."""
import argparse
import html
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("input", type=Path)
parser.add_argument("output", type=Path)
args = parser.parse_args()
lines = args.input.read_text().splitlines()
width = 1120
height = 104 + len(lines) * 25
pieces = [f'''<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}" role="img" aria-label="ures fictional terminal preview">
<defs><linearGradient id="bg" x2="1" y2="1"><stop stop-color="#0b1021"/><stop offset="1" stop-color="#17213a"/></linearGradient></defs>
<rect width="{width}" height="{height}" rx="18" fill="url(#bg)"/>
<rect x="15" y="15" width="1090" height="{height-30}" rx="12" fill="#101727" stroke="#2e3d58"/>
<circle cx="40" cy="40" r="6" fill="#fa7186"/><circle cx="62" cy="40" r="6" fill="#f8c86a"/><circle cx="84" cy="40" r="6" fill="#65d7b1"/>
<text x="1076" y="45" text-anchor="end" fill="#8291ab" font-family="monospace" font-size="13">ures · user resources</text>
<path d="M16 62H1104" stroke="#27364f"/>
<g font-family="'DejaVu Sans Mono',monospace" font-size="14.5" xml:space="preserve">''']
for index, line in enumerate(lines):
    color = "#cdd8eb"
    if index == 0:
        color = "#b8a3ff"
    elif line.startswith("CPU "):
        color = "#74dfdf"
    elif line.startswith("MEM "):
        color = "#f6ba70"
    elif line.lstrip().startswith("PID"):
        color = "#9bafff"
    elif line.startswith("Illustrative"):
        color = "#8291ab"
    pieces.append(f'<text x="36" y="{94 + index*25}" fill="{color}">{html.escape(line)}</text>')
pieces.append("</g></svg>\n")
args.output.parent.mkdir(parents=True, exist_ok=True)
args.output.write_text("\n".join(pieces))
