#!/usr/bin/env python3
"""Generate the exact five-target release manifest."""

import hashlib
import json
import pathlib
import sys

from release_targets import load_targets, matrix_sha256


def main() -> int:
    if len(sys.argv) != 2:
        raise SystemExit("usage: release-manifest.py VERSION")

    version = sys.argv[1]
    release_dir = pathlib.Path("release")
    targets = load_targets()
    expected_archives = sorted(target["archive"] for target in targets)
    archives = sorted(
        path.name
        for path in release_dir.iterdir()
        if path.is_file() and (path.name.endswith(".tar.gz") or path.name.endswith(".zip"))
    )
    if archives != expected_archives:
        raise SystemExit(f"release archives do not match expected set: {archives!r}")

    targets_by_archive = {target["archive"]: target for target in targets}
    manifest = {
        "version": version,
        "target_matrix_sha256": matrix_sha256(),
        "archives": [
            {
                "target": targets_by_archive[name]["target"],
                "file": name,
                "size": (release_dir / name).stat().st_size,
                "sha256": hashlib.sha256((release_dir / name).read_bytes()).hexdigest(),
            }
            for name in expected_archives
        ],
    }
    (release_dir / "release-manifest.json").write_text(
        json.dumps(manifest, indent=2) + "\n", encoding="utf-8"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
