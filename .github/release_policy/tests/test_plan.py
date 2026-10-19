# SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
# SPDX-License-Identifier: Apache-2.0
import json
import os
from pathlib import Path
import subprocess
import sys
from unittest.mock import patch

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from plan import git, plan, rejections
from preview import preview

# Candidate processes never inherit developer credentials.
SAFE_ENV = {key: value for key, value in os.environ.items() if key in {
    'PATH', 'HOME', 'TMPDIR', 'TMP', 'TEMP', 'CARGO_HOME', 'RUSTUP_HOME',
    'CARGO_TARGET_DIR', 'LANG', 'SSH_AUTH_SOCK',
}}


def command(root, *args):
    return subprocess.check_output(args, cwd=root, env=SAFE_ENV, text=True).strip()


def commit(root, title):
    command(root, 'git', 'add', '.')
    command(root, 'git', 'commit', '-m', title)
    return git(root, 'rev-parse', 'HEAD')


def fixture(tmp_path, version='1.2.3', dependent=False, tag=True):
    root = tmp_path / 'source'
    root.mkdir()
    command(root, 'git', 'init', '-b', 'main')
    command(root, 'git', 'config', 'commit.gpgsign', 'false')
    command(root, 'git', 'config', 'tag.gpgsign', 'false')
    command(root, 'git', 'config', 'user.name', 'Release Test')
    command(root, 'git', 'config', 'user.email', 'release-test@invalid')
    command(root, 'git', 'remote', 'add', 'origin', 'https://github.com/example/release-fixture')
    members = ['alpha', 'beta'] if dependent else ['alpha']
    (root / 'Cargo.toml').write_text('[workspace]\nresolver = "2"\nmembers = ' + json.dumps(members) + '\n')
    (root / 'release-plz.toml').write_text('[workspace]\nsemver_check = false\nrelease_commits = "^(feat|fix|perf|refactor)(\\\\(.+\\\\))?!?:"\n')
    for name in members:
        directory = root / name
        (directory / 'src').mkdir(parents=True)
        dependency = '\n[dependencies]\nalpha = {path = "../alpha", version = "' + version + '"}\n' if name == 'beta' else ''
        (directory / 'Cargo.toml').write_text(f'[package]\nname = "{name}"\nversion = "{version}"\nedition = "2021"\n' + dependency)
        (directory / 'src/lib.rs').write_text('pub fn value() -> u32 { 1 }\n')
    command(root, 'cargo', 'generate-lockfile')
    base = commit(root, 'chore: initial')
    if tag:
        for name in members:
            command(root, 'git', 'tag', f'{name}-v{version}')
    baseline = tmp_path / 'baseline'
    command(tmp_path, 'git', 'clone', '--no-local', str(root), str(baseline))
    return root, baseline, base


def run_plan(root, baseline):
    with patch.dict(os.environ, SAFE_ENV, clear=True):
        return plan(root, ['--registry-manifest-path', str(baseline / 'Cargo.toml'), '--no-changelog'])


@pytest.mark.parametrize('title,version,expected', [
    ('fix: correction', '1.2.3', '1.2.4'),
    ('feat: addition', '1.2.3', '1.3.0'),
    ('feat!: change', '1.2.3', '2.0.0'),
    ('feat!: change', '0.2.3', '0.3.0'),
    ('feat: addition', '0.2.3', '0.2.4'),
])
def test_real_release_plz(tmp_path, title, version, expected):
    root, baseline, _ = fixture(tmp_path, version)
    (root / 'alpha/src/lib.rs').write_text('pub fn value() -> u32 { 2 }\n')
    commit(root, title)
    result = run_plan(root, baseline)
    assert f'version = "{expected}"' in (root / 'alpha/Cargo.toml').read_text()
    assert bool(result['rejections']) == (expected == '2.0.0')
    if result['rejections']:
        assert result['rejections'] == [{'crate': 'alpha', 'committed': version, 'proposed': expected, 'manifest': 'alpha/Cargo.toml'}]
        assert 'dependency requirements' in result['summary']


@pytest.mark.parametrize('version,manual', [('1.2.3', '2.0.0'), ('1.2.3', '1.3.0'), ('0.9.0', '1.0.0')])
def test_manual_version(tmp_path, version, manual):
    root, baseline, _ = fixture(tmp_path, version=version)
    path = root / 'alpha/Cargo.toml'
    path.write_text(path.read_text().replace(version, manual))
    command(root, 'cargo', 'generate-lockfile')
    commit(root, 'feat!: manual version')
    result = run_plan(root, baseline)
    assert not result['rejections']
    assert git(root, 'status', '--porcelain') == ''


@pytest.mark.parametrize('tag', [False, True])
def test_first_release_and_unchanged(tmp_path, tag):
    root, baseline, _ = fixture(tmp_path, tag=tag)
    assert not run_plan(root, baseline)['rejections']
    assert git(root, 'status', '--porcelain') == ''


def test_root_manifest_change(tmp_path):
    root, baseline, _ = fixture(tmp_path)
    with (root / 'Cargo.toml').open('a') as output:
        output.write('\n[workspace.package]\nlicense = "Apache-2.0"\n')
    commit(root, 'fix: workspace metadata')
    calls = []
    original = subprocess.run

    def observe(args, **kwargs):
        calls.append(args)
        return original(args, **kwargs)

    with patch('subprocess.run', side_effect=observe):
        run_plan(root, baseline)
    assert any(args[:2] == ['release-plz', 'update'] for args in calls)


def test_dependent_major(tmp_path):
    root, baseline, _ = fixture(tmp_path, dependent=True)
    (root / 'alpha/src/lib.rs').write_text('pub fn value() -> u32 { 2 }\n')
    (root / 'beta/src/lib.rs').write_text('pub use alpha::value;\n')
    commit(root, 'feat!: change')
    result = run_plan(root, baseline)
    assert {item['crate'] for item in result['rejections']} == {'alpha', 'beta'}


def test_zero_to_one_guard():
    assert rejections({'a/Cargo.toml': {'crate': 'a', 'version': '0.9.0'}}, {'a/Cargo.toml': {'crate': 'a', 'version': '1.0.0'}})


def test_squash_title_and_isolation(tmp_path):
    root, baseline, base = fixture(tmp_path)
    (root / 'alpha/src/lib.rs').write_text('pub fn value() -> u32 { 2 }\n')
    head = commit(root, 'feat!: branch message must be ignored\n\nBREAKING CHANGE: ignored body')
    refs = git(root, 'show-ref')
    files = {str(path.relative_to(root)): path.read_bytes() for path in root.rglob('*') if path.is_file() and '.git' not in path.parts}
    args = ['--registry-manifest-path', str(baseline / 'Cargo.toml'), '--no-changelog']
    calls = []
    original = subprocess.run

    def observe(command_args, **kwargs):
        calls.append(command_args)
        return original(command_args, **kwargs)

    with patch.dict(os.environ, SAFE_ENV, clear=True), patch('subprocess.run', side_effect=observe):
        assert not preview(root, base, head, 'fix: squash subject', args)['rejections']
        assert preview(root, base, head, 'feat!: squash subject', args)['rejections']
    assert git(root, 'show-ref') == refs
    assert git(root, 'status', '--porcelain') == ''
    assert files == {str(path.relative_to(root)): path.read_bytes() for path in root.rglob('*') if path.is_file() and '.git' not in path.parts}
    assert all('push' not in args and 'publish' not in args and not args[:2] == ['release-plz', 'release'] for args in calls)


def test_guard_includes_unselected_dependent(tmp_path):
    root, baseline, _ = fixture(tmp_path, dependent=True)
    (root / 'alpha/src/lib.rs').write_text('pub fn value() -> u32 { 2 }\n')
    commit(root, 'fix: change')
    original = subprocess.run
    selected = []

    def update_with_dependent(args, **kwargs):
        result = original(args, **kwargs)
        if args[:2] == ['release-plz', 'update']:
            selected.append(args[4])
            # Model a generated dependency edit outside the selected package.
            path = root / 'beta/Cargo.toml'
            path.write_text(path.read_text().replace('version = "1.2.3"\nedition', 'version = "2.0.0"\nedition'))
        return result

    with patch('subprocess.run', side_effect=update_with_dependent):
        result = run_plan(root, baseline)
    assert selected == ['alpha']
    assert result['rejections'] == [{'crate': 'beta', 'committed': '1.2.3', 'proposed': '2.0.0', 'manifest': 'beta/Cargo.toml'}]
