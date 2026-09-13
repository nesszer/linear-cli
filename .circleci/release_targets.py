#!/usr/bin/env python3
"""Load and validate the repository-owned release target matrix."""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import re
import sys
from typing import Any


TARGETS_PATH = pathlib.Path(__file__).with_name("release-targets.json")
REQUIRED_FIELDS = {
    "circleci_job",
    "target",
    "archive",
    "archive_type",
    "binary",
    "file_pattern",
    "checks_version",
}


def load_targets() -> list[dict[str, Any]]:
    payload = json.loads(TARGETS_PATH.read_text(encoding="utf-8"))
    targets = payload.get("targets")
    if not isinstance(targets, list) or not targets:
        raise ValueError("release-targets.json must contain a non-empty targets list")

    normalized: list[dict[str, Any]] = []
    seen_targets: set[str] = set()
    seen_archives: set[str] = set()
    seen_jobs: set[str] = set()
    for target in targets:
        if not isinstance(target, dict) or set(target) != REQUIRED_FIELDS:
            raise ValueError(f"invalid release target entry: {target!r}")
        if any(not isinstance(target[field], str) or not target[field] for field in REQUIRED_FIELDS - {"checks_version"}):
            raise ValueError(f"release target has an empty or non-string field: {target!r}")
        if not isinstance(target["checks_version"], bool):
            raise ValueError(f"checks_version must be boolean: {target!r}")
        for key, seen in (
            ("target", seen_targets),
            ("archive", seen_archives),
            ("circleci_job", seen_jobs),
        ):
            value = target[key]
            if value in seen:
                raise ValueError(f"duplicate release target {key}: {value}")
            seen.add(value)
        if target["archive_type"] not in {"tar.gz", "zip"}:
            raise ValueError(f"unsupported archive type: {target['archive_type']}")
        if not target["archive"].endswith(f".{target['archive_type']}"):
            raise ValueError(f"archive suffix does not match its type: {target!r}")
        normalized.append(target)

    return normalized


def print_tsv(targets: list[dict[str, Any]]) -> None:
    for target in targets:
        print(
            "\t".join(
                [
                    target["target"],
                    target["archive"],
                    target["archive_type"],
                    target["binary"],
                    target["file_pattern"],
                    str(target["checks_version"]).lower(),
                ]
            )
        )


def matrix_sha256() -> str:
    return hashlib.sha256(TARGETS_PATH.read_bytes()).hexdigest()


def circleci_job_block(config: str, job: str) -> str:
    start_match = re.search(rf"(?m)^  {re.escape(job)}:\s*$", config)
    if start_match is None:
        raise ValueError(f"CircleCI config is missing release job {job!r}")
    remainder = config[start_match.end() :]
    end_match = re.search(r"(?m)^  \S", remainder)
    end = start_match.end() + (end_match.start() if end_match else len(remainder))
    return config[start_match.start() : end]


def check_circleci_config(config_path: pathlib.Path, targets: list[dict[str, Any]]) -> None:
    config = config_path.read_text(encoding="utf-8")
    for target in targets:
        block = circleci_job_block(config, target["circleci_job"])
        if target["target"] not in block:
            raise ValueError(
                f"CircleCI job {target['circleci_job']!r} does not use target {target['target']!r}"
            )

    release_jobs = {target["circleci_job"] for target in targets}
    for job in release_jobs:
        circleci_job_block(config, job)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--tsv", action="store_true", help="print target fields for shell callers")
    parser.add_argument("--sha256", action="store_true", help="print the matrix file digest")
    parser.add_argument(
        "--check-circleci-config",
        type=pathlib.Path,
        metavar="PATH",
        help="verify that each release build job uses the manifest target",
    )
    args = parser.parse_args()

    try:
        targets = load_targets()
        if args.tsv:
            print_tsv(targets)
        if args.sha256:
            print(matrix_sha256())
        if args.check_circleci_config:
            check_circleci_config(args.check_circleci_config, targets)
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"release target validation failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
