"""Crop safe monitor regions from the inspected historical native screenshot.

Requires Pillow for this optional documentation-only operation. No UI automation.
Usage: python assets/screenshots/prepare-preview.py <original-monitor-screenshot.png>
"""

import argparse
from pathlib import Path

from PIL import Image


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    args = parser.parse_args()
    destination = Path(__file__).resolve().parent
    with Image.open(args.source) as image:
        if image.size != (1366, 768):
            parser.error("expected the reviewed historical 1366x768 monitor screenshot")
        # Exclude the entire terminal pane: it contains local commands and paths.
        image.crop((688, 113, 1344, 728)).save(destination / "monitor-preview.png")
        image.crop((702, 456, 1320, 715)).save(destination / "charts-preview.png")


if __name__ == "__main__":
    main()
