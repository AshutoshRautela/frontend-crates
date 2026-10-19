# SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
# SPDX-License-Identifier: Apache-2.0
"""Construct the repository's squash merge in an isolated full-history clone."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile

from plan import git, plan


def preview(source, base, head, title, update_args=()):
    # Use a separate object store and no inherited credentials or Git hooks.
    with tempfile.TemporaryDirectory(prefix="release-preview-") as directory:
        root = Path(directory) / "repo"
        subprocess.run(["git", "clone", "--no-local", "--no-checkout", str(source), str(root)], check=True)
        subprocess.run(["git", "fetch", "origin", base, head], cwd=root, check=True)
        subprocess.run(["git", "checkout", "--detach", base], cwd=root, check=True)
        subprocess.run(["git", "merge", "--squash", "--no-commit", head], cwd=root, check=True)
        tree = git(root, "write-tree")
        env = dict(os.environ, GIT_AUTHOR_NAME="Release preview", GIT_AUTHOR_EMAIL="preview@invalid", GIT_COMMITTER_NAME="Release preview", GIT_COMMITTER_EMAIL="preview@invalid")
        commit = subprocess.check_output(["git", "commit-tree", tree, "-p", base], input=title + "\n", cwd=root, env=env, text=True).strip()
        subprocess.run(["git", "checkout", "-b", "release-preview", commit], cwd=root, check=True)
        return plan(root, update_args)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    snapshot = json.loads(os.environ["RELEASE_SNAPSHOT"])
    result = preview(args.source, snapshot["base"], snapshot["head"], snapshot["title"])
    args.report.write_text(json.dumps(result))
    print(result["summary"])
    return bool(result["rejections"])


if __name__ == "__main__":
    raise SystemExit(main())
