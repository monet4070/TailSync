#!/usr/bin/env python3
"""Opt-in article consumer acceptance on private macOS pasteboards."""
import argparse
import json
from pathlib import Path
import struct
import subprocess
import uuid

from probe import run


def verify(result):
    assert result["pasted"]
    assert result["text"].rstrip("\n") == "Alpha\ufffcBeta\ufffcGamma\ufffcOmega"
    assert result["image_positions_utf16"] == [5, 10, 16]
    assert result["image_count"] == 3
    colors = result["image_colors_rgb"]
    assert len(colors) == 3
    assert colors[0] == colors[2], "the repeated image must remain the same"
    for color, dominant in zip(colors, [0, 1, 0]):
        assert color[dominant] > 200 and all(c < 80 for i, c in enumerate(color) if i != dominant)
    assert result["general_clipboard_modified"] is False


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--helper", required=True)
    parser.add_argument("--consumer", required=True)
    parser.add_argument("--fixtures", type=Path, required=True)
    parser.add_argument("--browser-board-file", type=Path)
    args = parser.parse_args()
    results = {}
    for name in ["html-only", "with-text-rtf"]:
        board = "tailsync-probe.article." + str(uuid.uuid4())
        receipt = run(args.helper, ["--pasteboard", board, "--write-snapshot"],
                      uuid.uuid4().bytes + struct.pack(">q", -1) +
                      (args.fixtures / (name + "-prepared.cns")).read_bytes())
        assert receipt[:5] == b"CWR1\0"
        result = json.loads(subprocess.check_output([args.consumer, board], timeout=6))
        verify(result)
        results[name] = result
    if args.browser_board_file:
        board = args.browser_board_file.read_text().strip()
        assert board.startswith("tailsync-probe.")
        result = json.loads(subprocess.check_output([args.consumer, board], timeout=6))
        verify(result)
        results["webkit-selection"] = result
    print(json.dumps({"result": "passed", "consumers": results}, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
