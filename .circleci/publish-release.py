#!/usr/bin/env python3
"""Publish verified GitHub release assets without overwriting existing files."""

from __future__ import annotations

import hashlib
import json
import os
import pathlib
import subprocess
import sys
from typing import Any

from release_targets import matrix_sha256


RELEASE_DIR = pathlib.Path("release")


def run_gh(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["gh", *args],
        check=False,
        capture_output=True,
        text=True,
    )


def release_assets(tag: str) -> dict[str, dict[str, Any]] | None:
    result = run_gh("release", "view", tag, "--json", "assets,isDraft,isPrerelease")
    if result.returncode != 0:
        diagnostic = f"{result.stdout}\n{result.stderr}".lower()
        if "not found" in diagnostic or "http 404" in diagnostic:
            return None
        raise RuntimeError(f"could not inspect GitHub release {tag}: {result.stderr.strip()}")

    payload = json.loads(result.stdout)
    if payload.get("isDraft"):
        raise RuntimeError(f"GitHub release {tag} is a draft; refusing to publish into it")
    return {asset["name"]: asset for asset in payload.get("assets", [])}


def expected_assets() -> dict[str, dict[str, Any]]:
    manifest_path = RELEASE_DIR / "release-manifest.json"
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    if manifest.get("target_matrix_sha256") != matrix_sha256():
        raise ValueError("release manifest target matrix does not match the checked-in matrix")
    archive_entries = manifest.get("archives", [])
    if not isinstance(archive_entries, list):
        raise ValueError("release manifest archives must be a list")

    assets: dict[str, dict[str, Any]] = {}
    manifest_entries = [
        *archive_entries,
        {"file": "SHA256SUMS"},
        {"file": "release-manifest.json"},
    ]
    for entry in manifest_entries:
        name = entry.get("file")
        if not isinstance(name, str) or not name:
            raise ValueError(f"invalid release manifest entry: {entry!r}")
        path = RELEASE_DIR / name
        if not path.is_file():
            raise ValueError(f"manifest asset is missing locally: {path}")
        local_digest = hashlib.sha256(path.read_bytes()).hexdigest()
        digest = entry.get("sha256", local_digest)
        if not isinstance(digest, str):
            raise ValueError(f"invalid release manifest digest: {entry!r}")
        if local_digest != digest:
            raise ValueError(f"local asset digest disagrees with release manifest: {name}")
        assets[name] = {
            "path": path,
            "digest": f"sha256:{digest}",
            "size": path.stat().st_size,
        }
    if len(archive_entries) != 5 or len(assets) != 7:
        raise ValueError("release manifest must contain exactly five archives and seven assets")
    return assets


def check_existing(
    existing: dict[str, dict[str, Any]], expected: dict[str, dict[str, Any]]
) -> set[str]:
    extra = sorted(set(existing) - set(expected))
    if extra:
        raise RuntimeError(f"existing GitHub release has unexpected assets: {extra}")

    missing: set[str] = set()
    for name, expected_asset in expected.items():
        asset = existing.get(name)
        if asset is None:
            missing.add(name)
            continue
        if asset.get("size") != expected_asset["size"]:
            raise RuntimeError(f"existing asset has the wrong size: {name}")
        if asset.get("digest") != expected_asset["digest"]:
            raise RuntimeError(
                f"existing asset has the wrong or unavailable digest: {name}; refusing to overwrite"
            )
    return missing


def publish(tag: str) -> None:
    expected = expected_assets()
    existing = release_assets(tag)
    if existing is None:
        create = run_gh(
            "release",
            "create",
            tag,
            "--verify-tag",
            "--title",
            tag,
            "--generate-notes",
        )
        if create.returncode != 0:
            raise RuntimeError(f"could not create GitHub release {tag}: {create.stderr.strip()}")
        existing = release_assets(tag)
        if existing is None:
            raise RuntimeError(f"GitHub release {tag} was not visible after creation")

    missing = check_existing(existing, expected)
    for name in sorted(missing):
        upload = run_gh("release", "upload", tag, str(expected[name]["path"]))
        if upload.returncode == 0:
            continue
        # A concurrent retry may have uploaded this exact asset. Re-read before
        # failing so the normal path remains safe and idempotent.
        refreshed = release_assets(tag)
        if refreshed is not None and name not in check_existing(refreshed, expected):
            continue
        raise RuntimeError(f"could not upload {name}: {upload.stderr.strip()}")

    final = release_assets(tag)
    if final is None:
        raise RuntimeError(f"GitHub release {tag} disappeared after publication")
    remaining = check_existing(final, expected)
    if remaining:
        raise RuntimeError(f"GitHub release is missing assets after publication: {sorted(remaining)}")


def main() -> int:
    tag = os.environ.get("CIRCLE_TAG")
    if not tag:
        print("CIRCLE_TAG is required", file=sys.stderr)
        return 1
    try:
        publish(tag)
    except (OSError, RuntimeError, ValueError, json.JSONDecodeError) as error:
        print(f"release publication failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
