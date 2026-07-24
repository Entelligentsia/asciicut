#!/usr/bin/env python3
"""
castcut compose — prototype of the M1 compose engine.

Reads a .castcut.json edit project and an immutable source .cast, and emits a
composed .cast: keep-segments in order, per-segment speed, global idle cap, and
per-segment end-holds. This is the reference the TypeScript compose engine will
port (see SPEC.md §4.3).

Usage:
    python3 compose.py project.castcut.json > edited.cast

Project shape (see SPEC.md §4.2):
{
  "source": "sample.cast",
  "output": {"width": 120, "height": 40},
  "idleCap": 0.4,
  "segments": [
    {"srcStart": 25.0, "srcEnd": 31.0,  "speed": 1.0, "holdEnd": 0.0},
    {"srcStart": 472.0,"srcEnd": 504.0, "speed": 2.5, "holdEnd": 1.5},
    {"srcStart": 916.0,"srcEnd": 947.0, "speed": 1.0, "holdEnd": 3.0}
  ]
}
"""
import json
import sys
from pathlib import Path

BEAT = 0.5  # inserted pause between segments


def load_cast(path):
    lines = Path(path).read_text().splitlines()
    header = json.loads(lines[0])
    events = []
    for ln in lines[1:]:
        ln = ln.strip()
        if ln.startswith("["):
            try:
                events.append(json.loads(ln))
            except json.JSONDecodeError:
                pass
    return header, events


def compose(project_path):
    proj = json.loads(Path(project_path).read_text())
    proj_dir = Path(project_path).parent
    header, events = load_cast(proj_dir / proj["source"])

    out_cfg = proj.get("output", {})
    header["width"] = out_cfg.get("width", header.get("width", 80))
    header["height"] = out_cfg.get("height", header.get("height", 24))
    header.pop("idle_time_limit", None)

    idle_cap = float(proj.get("idleCap", 0.4))
    out = []
    t = 0.0
    last_out_payload = None

    for i, seg in enumerate(proj["segments"]):
        s, e = float(seg["srcStart"]), float(seg["srcEnd"])
        speed = float(seg.get("speed", 1.0)) or 1.0
        hold = float(seg.get("holdEnd", 0.0))

        window = [ev for ev in events if ev[1] == "o" and s <= ev[0] <= e]
        if not window:
            continue
        if i > 0:
            t += BEAT

        prev = window[0][0]
        for ev in window:
            dt = ev[0] - prev
            prev = ev[0]
            if dt < 0:
                dt = 0.0
            dt = min(dt, idle_cap) / speed
            t += dt
            out.append([round(t, 4), "o", ev[2]])
            last_out_payload = ev[2]

        # end-hold: keep the last screen on-frame `hold` seconds
        if hold > 0 and last_out_payload is not None:
            t += hold
            out.append([round(t, 4), "o", ""])  # zero-width tick extends dwell

    sys.stdout.write(json.dumps(header) + "\n")
    for ev in out:
        sys.stdout.write(json.dumps(ev, ensure_ascii=False) + "\n")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit("usage: compose.py project.castcut.json > edited.cast")
    compose(sys.argv[1])
