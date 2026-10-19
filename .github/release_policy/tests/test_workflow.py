# SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
# SPDX-License-Identifier: Apache-2.0
from pathlib import Path

import yaml

WORKFLOWS = Path(__file__).resolve().parents[2] / 'workflows'


def test_privilege_boundary():
    source = (WORKFLOWS / 'release-policy.yml').read_text()
    workflow = yaml.load(source, Loader=yaml.BaseLoader)
    jobs = workflow['jobs']
    assert set(workflow['on']) == {'pull_request_target', 'workflow_run'}
    assert workflow['on']['workflow_run']['workflows'] == ['ci']
    assert jobs['preview']['permissions'] == {'contents': 'read'}
    assert jobs['preview']['env']['PYTHONDONTWRITEBYTECODE'] == '1'
    assert jobs['preview']['if'] == "needs.controller.outputs.allowed == 'true'"
    assert 'secrets.' not in source
    assert all('environment' not in job for job in jobs.values())
    assert all('id-token' not in job['permissions'] for job in jobs.values())
    for name in ('controller', 'publish-status'):
        job = jobs[name]
        assert job['permissions']['checks'] == 'write'
        for step in job['steps']:
            assert 'run' not in step
            if step['uses'].startswith('actions/checkout@'):
                assert step['with']['ref'] == '${{ github.workflow_sha }}'
                assert step['with']['persist-credentials'] == 'false'
    candidate = next(step for step in jobs['preview']['steps'] if step.get('with', {}).get('path') == 'candidate')
    assert candidate['with']['persist-credentials'] == 'false'
    assert candidate['with']['fetch-depth'] == '0'
    assert candidate['with']['ref'] == '${{ needs.controller.outputs.head }}'


def test_publishing_uses_same_guard_before_push_or_publish():
    source = (WORKFLOWS / 'release.yml').read_text()
    assert source.index('release_policy/plan.py') < source.index('git push') < source.index('run: release-plz release')
    assert 'release_policy/install.py' in source
    workflow = yaml.load(source, Loader=yaml.BaseLoader)
    assert workflow['jobs']['release']['env']['PYTHONDONTWRITEBYTECODE'] == '1'
    assert 'release-plz update' not in source
