#!/usr/bin/env python3
"""Update the crate version across project metadata."""

from __future__ import annotations

import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
CARGO_TOML = REPO_ROOT / "Cargo.toml"
CARGO_LOCK = REPO_ROOT / "Cargo.lock"

VERSION_PATTERN = re.compile(r'^(version\s*=\s*")([0-9A-Za-z\.-]+)("\s*)$', re.MULTILINE)


def update_file(path: Path, version: str) -> None:
    if not path.exists():
        return

    original = path.read_text(encoding="utf-8")

    def replace(match: re.Match[str]) -> str:
        return f"{match.group(1)}{version}{match.group(3)}"

    updated, count = VERSION_PATTERN.subn(replace, original, count=1)
    if count == 0:
        raise RuntimeError(f"version field not found in {path}")

    path.write_text(updated, encoding="utf-8")
    print(f"Updated {path.relative_to(REPO_ROOT)} to version {version}")


def main(args: list[str]) -> int:
    if len(args) != 2:
        print("Usage: scripts/update_version.py <new-version>", file=sys.stderr)
        return 1

    version = args[1]
    update_file(CARGO_TOML, version)
    update_file(CARGO_LOCK, version)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
