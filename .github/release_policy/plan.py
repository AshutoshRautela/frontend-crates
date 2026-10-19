# SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
# SPDX-License-Identifier: Apache-2.0
"""Shared release selection and generated-major guard; never publishes."""
import argparse
import json
from pathlib import Path
import re
import subprocess
import tomllib

RELEASE_PLZ_VERSION = "0.3.158"


def git(root, *args):
    return subprocess.check_output(["git", *args], cwd=root, text=True).strip()


def manifests(root, committed=False):
    paths = git(root, "ls-files", "-z").split("\0")
    root_doc = tomllib.loads(git(root, "show", "HEAD:Cargo.toml")) if committed else tomllib.loads((root / "Cargo.toml").read_text())
    packages = {}
    for path in paths:
        if path != "Cargo.toml" and not path.endswith("/Cargo.toml"):
            continue
        doc = tomllib.loads(git(root, "show", f"HEAD:{path}")) if committed else tomllib.loads((root / path).read_text())
        package = doc.get("package")
        if not package:
            continue
        version = package["version"]
        if isinstance(version, dict):
            version = root_doc["workspace"]["package"]["version"]
        packages[path] = {"crate": package["name"], "version": version}
    return packages


def rejections(before, after):
    return [dict(crate=old["crate"], committed=old["version"], proposed=after[path]["version"], manifest=path)
            for path, old in before.items()
            if old["version"].split(".")[0] != after[path]["version"].split(".")[0]]


def plan(root, update_args=()):
    if git(root, "status", "--porcelain"):
        raise RuntimeError("Release checkout must start clean")
    version = subprocess.check_output(["release-plz", "--version"], text=True).strip()
    if version != f"release-plz {RELEASE_PLZ_VERSION}":
        raise RuntimeError(f"Expected release-plz {RELEASE_PLZ_VERSION}, got {version}")
    before = manifests(root, committed=True)
    metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--no-deps", "--format-version=1"], cwd=root, text=True))
    members = set(metadata["workspace_members"])
    tags = git(root, "tag", "--list").splitlines()
    # Freeze selection before any update can modify dependent manifests.
    selected = []
    for package in metadata["packages"]:
        if package["id"] not in members or package["publish"] == []:
            continue
        name, version = package["name"], package["version"]
        path = Path(package["manifest_path"]).relative_to(root)
        matching = [tag for tag in tags if re.fullmatch(re.escape(name) + r"-v\d+\.\d+\.\d+", tag)]
        last = max(matching, key=lambda tag: tuple(map(int, tag[len(name) + 2:].split("."))), default=None)
        if last is None:
            print(f"{name}: no release tag yet -> first release is manual, skipping", flush=True)
            continue
        changed = subprocess.run(["git", "diff", "--quiet", f"{last}..HEAD", "--", str(path.parent), "Cargo.toml"], cwd=root)
        if changed.returncode not in (0, 1):
            changed.check_returncode()
        if changed.returncode == 1:
            advisory = subprocess.run(["cargo", "semver-checks", "check-release", "--package", name, "--baseline-rev", last], cwd=root)
            if advisory.returncode:
                print(f"::warning::{name}: API compatibility check failed; version policy is maintainer-controlled.", flush=True)
        if version != last[len(name) + 2:]:
            print(f"{name}: manually pegged {version}; publish as-is", flush=True)
        elif changed.returncode == 1:
            selected.append(name)
        else:
            print(f"{name}: unchanged", flush=True)
    for name in selected:
        subprocess.run(["release-plz", "update", "--allow-dirty", "-p", name, *update_args], cwd=root, check=True)
    rejected = rejections(before, manifests(root))
    lines = [f"{item['crate']}: {item['committed']} -> {item['proposed']} ({item['manifest']}). Explicitly edit the version and affected dependency requirements in the PR." for item in rejected]
    return {"rejections": rejected, "summary": "\n\n".join(lines) if lines else "Release preview passed: no generated major-number changes."}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    result = plan(args.root.resolve())
    args.report.write_text(json.dumps(result))
    print(result["summary"])
    return bool(result["rejections"])


if __name__ == "__main__":
    raise SystemExit(main())
