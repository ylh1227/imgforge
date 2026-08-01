#!/usr/bin/env python3
"""Extract Mermaid blocks from REVIEW_PIPELINE_FLOW.md and render PNGs via mmdc."""

from __future__ import annotations

import os
import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MD = ROOT / "docs" / "REVIEW_PIPELINE_FLOW.md"
OUT_DIR = ROOT / "docs" / "flowcharts"
MMDC = ROOT / "tools" / "mermaid" / "node_modules" / ".bin" / "mmdc"
NODE_BIN = ROOT / "tools" / "node" / "bin"
CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"

# Ordered (prefix, slug); longer prefixes must be checked first.
SLUG_RULES: list[tuple[str, str]] = [
    ("图 11b", "11b-server-match"),
    ("图 2a", "02a-online-load"),
    ("图 2b", "02b-offline-load"),
    ("图 15", "15-roles"),
    ("图 14", "14-exceptions"),
    ("图 13 · 开发", "13-dev-download"),
    ("图 13 · 角色", "15-roles"),
    ("图 12 · ⑦", "12-assign-submit"),
    ("图 12 · 异常", "14-exceptions"),
    ("图 11 ·", "11-review-defect"),
    ("图 10", "10-readiness"),
    ("图 9", "09-channel-sink"),
    ("图 8", "08-channel-b2"),
    ("图 7", "07-channel-b1"),
    ("图 6", "06-channel-a"),
    ("图 5", "05-upload-decide"),
    ("图 4", "04-capture"),
    ("图 3", "03-device-role"),
    ("图 2 ·", "02-load-mode"),
    ("一眼对照", "02-load-mode-compare"),
    ("图 1 ·", "01-end-to-end"),
    ("图 1", "01-end-to-end"),
]


def slug_for_heading(heading: str, diagram_index_in_section: int) -> str:
    for key, slug in SLUG_RULES:
        if heading.startswith(key) or heading == key or key in heading[: max(12, len(key) + 2)]:
            if key == "一眼对照" and diagram_index_in_section > 1:
                return "02-load-mode-decide"
            if key.startswith("图 2 ·") and diagram_index_in_section > 1:
                return "02-load-mode-decide"
            return slug
    return f"diagram-{diagram_index_in_section:02d}"


def extract_blocks(text: str) -> list[tuple[str, str]]:
    blocks: list[tuple[str, str]] = []
    heading = "diagram"
    lines = text.splitlines()
    i = 0
    while i < len(lines):
        line = lines[i]
        if line.startswith("## "):
            heading = line[3:].strip()
        elif line.startswith("### "):
            sub = line[4:].strip()
            if "对照" in sub or "并排" in sub:
                heading = sub
        if line.strip() == "```mermaid":
            i += 1
            body: list[str] = []
            while i < len(lines) and lines[i].strip() != "```":
                body.append(lines[i])
                i += 1
            src = "\n".join(body).strip() + "\n"
            blocks.append((heading, src))
        i += 1
    return blocks


COMPACT_INIT = (
    "%%{init: {"
    "'flowchart': {"
    "  'nodeSpacing': 36, 'rankSpacing': 40, 'padding': 14, "
    "  'htmlLabels': true, 'wrappingWidth': 260"
    "}, "
    "'themeVariables': {"
    "  'fontSize': '20px', "
    "  'fontFamily': 'PingFang SC, Microsoft YaHei, sans-serif', "
    "  'lineColor': '#333333'"
    "}"
    "}}%%\n"
)


def render(slug: str, source: str) -> Path:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    mmd = OUT_DIR / f"{slug}.mmd"
    png = OUT_DIR / f"{slug}.png"
    body = source if source.lstrip().startswith("%%{") else COMPACT_INIT + source
    mmd.write_text(body, encoding="utf-8")

    env = os.environ.copy()
    env["PATH"] = f"{NODE_BIN}:{env.get('PATH', '')}"
    env["PUPPETEER_EXECUTABLE_PATH"] = CHROME

    # High-DPI + ample width so Chinese labels stay sharp in Feishu/Word.
    cmd = [
        str(MMDC),
        "-i",
        str(mmd),
        "-o",
        str(png),
        "-b",
        "white",
        "-s",
        "3",
        "-w",
        "1400",
    ]
    print("render", slug, flush=True)
    subprocess.run(cmd, check=True, env=env, cwd=str(ROOT / "tools" / "mermaid"))
    if not png.exists():
        raise SystemExit(f"missing output {png}")
    return png


def main() -> None:
    if not MMDC.exists():
        raise SystemExit(f"mmdc not found: {MMDC}")

    # Clear previous misnamed outputs
    if OUT_DIR.exists():
        for p in OUT_DIR.glob("*"):
            p.unlink()

    text = MD.read_text(encoding="utf-8")
    blocks = extract_blocks(text)
    if not blocks:
        raise SystemExit("no mermaid blocks found")

    section_counts: dict[str, int] = {}
    used: dict[str, int] = {}
    manifests: list[tuple[str, str, Path]] = []

    for heading, src in blocks:
        section_counts[heading] = section_counts.get(heading, 0) + 1
        slug = slug_for_heading(heading, section_counts[heading])
        if slug in used:
            used[slug] += 1
            slug = f"{slug}-{used[slug]}"
        else:
            used[slug] = 1
        png = render(slug, src)
        manifests.append((heading, slug, png))
        print(f"  ok {png.name} ({png.stat().st_size} bytes) <- {heading}")

    (OUT_DIR / "manifest.txt").write_text(
        "\n".join(f"{slug}\t{heading}\t{png.name}" for heading, slug, png in manifests) + "\n",
        encoding="utf-8",
    )
    print(f"wrote {len(manifests)} diagrams -> {OUT_DIR}")


if __name__ == "__main__":
    main()
