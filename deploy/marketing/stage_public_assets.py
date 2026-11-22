#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Stage the verified public catalog reported by the linked marketing build."""

import argparse
from pathlib import Path
import re
import shutil


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("build_log", type=Path)
    parser.add_argument("destination", type=Path)
    args = parser.parse_args()

    published = re.findall(
        r"^published [0-9]+ public assets to (.+)$",
        args.build_log.read_text(encoding="utf-8", errors="replace"),
        re.MULTILINE,
    )
    if len(published) != 1:
        parser.error("expected one published public asset catalog in the linked build log")

    source = Path(published[0])
    if not (source / "manifest.json").is_file():
        parser.error(f"no public asset manifest at {source}")

    shutil.copytree(source, args.destination)


if __name__ == "__main__":
    main()
