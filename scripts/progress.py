#!/usr/bin/env python3
"""Recount status emoji in docs/FEATURES.md tables and rewrite the progress line."""
import re, pathlib, sys

STATES = ["✅", "🟨", "🎨", "⬜", "➖"]
p = pathlib.Path(__file__).resolve().parent.parent / "docs" / "FEATURES.md"
text = p.read_text(encoding="utf-8")

counts = dict.fromkeys(STATES, 0)
for line in text.splitlines():
    if line.startswith("|"):
        last = line.strip().strip("|").split("|")[-1].strip()  # status is always the last column
        if last in counts:
            counts[last] += 1

total = sum(counts.values())
done = counts["✅"] / max(total - counts["➖"], 1) * 100
summary = " · ".join(f"{counts[s]} {s}" for s in STATES)
block = f"<!-- progress:start -->\n**Progress:** {summary} ({total} items, {done:.0f}% done)\n<!-- progress:end -->"
text = re.sub(r"<!-- progress:start -->.*?<!-- progress:end -->", block, text, flags=re.S)
p.write_bytes(text.encode("utf-8"))  # bytes: Windows text mode would turn LF into CRLF
sys.stdout.reconfigure(encoding="utf-8")  # Windows consoles default to cp1252
print(block.splitlines()[1])
