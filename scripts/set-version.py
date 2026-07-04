#!/usr/bin/env python3
from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SEMVER = re.compile(r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$")


def replace_once(path: Path, pattern: str, replacement: str) -> None:
    text = path.read_text(encoding="utf-8")
    updated, count = re.subn(pattern, replacement, text, count=1, flags=re.MULTILINE)
    if count != 1:
        raise RuntimeError(f"Could not update {path.relative_to(ROOT)}")
    path.write_text(updated, encoding="utf-8")


def main() -> int:
    if len(sys.argv) != 2:
        print("Usage: scripts/set-version.py <version>", file=sys.stderr)
        return 2

    version = sys.argv[1].strip()
    if version.startswith("v"):
        print("Version must not start with 'v'. Use 1.2.3, not v1.2.3.", file=sys.stderr)
        return 2

    if not SEMVER.match(version):
        print(f"Invalid SemVer version: {version}", file=sys.stderr)
        return 2

    replace_once(ROOT / "Cargo.toml", r'^version\s*=\s*"[^"]+"', f'version = "{version}"')
    replace_once(ROOT / "Cargo.lock", r'(?m)(^name\s*=\s*"lazyvim"\nversion\s*=\s*)"[^"]+"', rf'\1"{version}"')

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
