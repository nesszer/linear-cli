#!/usr/bin/env python3
"""Focused tests for the release target and publication safety helpers."""

from __future__ import annotations

import importlib.util
import pathlib
import unittest

import release_targets


def load_publisher():
    path = pathlib.Path(__file__).with_name("publish-release.py")
    spec = importlib.util.spec_from_file_location("publish_release", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"could not load {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class ReleaseTargetTests(unittest.TestCase):
    def test_manifest_matches_circleci_build_jobs(self) -> None:
        targets = release_targets.load_targets()
        release_targets.check_circleci_config(
            pathlib.Path(__file__).with_name("config.yml"), targets
        )
        self.assertEqual(len(targets), 5)


class PublisherTests(unittest.TestCase):
    def setUp(self) -> None:
        self.publisher = load_publisher()
        self.expected = {
            name: {"size": len(name), "digest": f"sha256:{name}"}
            for name in (
                "SHA256SUMS",
                "release-manifest.json",
                "linear-cli-x86_64-unknown-linux-gnu.tar.gz",
                "linear-cli-aarch64-unknown-linux-gnu.tar.gz",
                "linear-cli-x86_64-pc-windows-msvc.zip",
                "linear-cli-x86_64-apple-darwin.tar.gz",
                "linear-cli-aarch64-apple-darwin.tar.gz",
            )
        }

    def test_missing_assets_are_upload_candidates(self) -> None:
        self.assertEqual(self.publisher.check_existing({}, self.expected), set(self.expected))

    def test_exact_assets_are_a_noop(self) -> None:
        self.assertEqual(
            self.publisher.check_existing(self.expected, self.expected), set()
        )

    def test_unexpected_assets_fail_closed(self) -> None:
        with self.assertRaises(RuntimeError):
            self.publisher.check_existing({"unexpected.bin": {}}, self.expected)

    def test_mismatched_assets_are_never_overwritten(self) -> None:
        existing = dict(self.expected)
        existing["SHA256SUMS"] = {"size": 1, "digest": "sha256:wrong"}
        with self.assertRaises(RuntimeError):
            self.publisher.check_existing(existing, self.expected)


if __name__ == "__main__":
    unittest.main()
