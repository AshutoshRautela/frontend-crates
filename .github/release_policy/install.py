# SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
# SPDX-License-Identifier: Apache-2.0
"""Install the pinned release tools for both publishing and preview runners."""
from pathlib import Path
import subprocess
import tarfile
import tempfile
import urllib.request

from plan import RELEASE_PLZ_VERSION

TOOLS = [
    (f'https://github.com/release-plz/release-plz/releases/download/release-plz-v{RELEASE_PLZ_VERSION}/release-plz-x86_64-unknown-linux-gnu.tar.gz', 'release-plz'),
    ('https://github.com/obi1kenobi/cargo-semver-checks/releases/download/v0.48.0/cargo-semver-checks-x86_64-unknown-linux-gnu.tar.gz', 'cargo-semver-checks'),
]


def main():
    with tempfile.TemporaryDirectory() as directory:
        for url, binary in TOOLS:
            archive = Path(directory) / 'tool.tar.gz'
            urllib.request.urlretrieve(url, archive)
            with tarfile.open(archive) as package:
                member = next(item for item in package.getmembers() if Path(item.name).name == binary and item.isfile())
                target = Path('/usr/local/bin') / binary
                target.write_bytes(package.extractfile(member).read())
                target.chmod(0o755)
            subprocess.run([str(target), '--version'], check=True)


if __name__ == '__main__':
    main()
