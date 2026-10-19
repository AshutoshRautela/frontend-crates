// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0
const assert = require('node:assert/strict');
const test = require('node:test');
const controller = require('../controller.cjs');

function harness({mirror = 'head', head = 'head', base = 'base', title = 'fix: test', body = '', titleMode = 'PR_TITLE', messageMode = 'BLANK', allowSquash = true, allowMerge = false, allowRebase = false} = {}) {
  const outputs = {};
  const checks = [];
  const updates = [];
  const github = {rest: {
    pulls: {get: async () => ({data: {head: {sha: head}, base: {ref: 'main'}, title, body, state: 'open'}})},
    repos: {
      get: async () => ({data: {squash_merge_commit_title: titleMode, squash_merge_commit_message: messageMode, allow_squash_merge: allowSquash, allow_merge_commit: allowMerge, allow_rebase_merge: allowRebase}}),
      getBranch: async () => ({data: {commit: {sha: base}}}),
    },
    git: {getRef: async () => {
      if (mirror === null) throw Object.assign(new Error('missing'), {status: 404});
      return {data: {object: {sha: mirror}}};
    }},
    checks: {
      create: async value => {checks.push(value); return {data: {id: 123}};},
      update: async value => {updates.push(value);},
    },
  }};
  return {github, context: {repo: {owner: 'example', repo: 'project'}, payload: {pull_request: {number: 42}}},
    core: {setOutput: (key, value) => {outputs[key] = value;}}, outputs, checks, updates};
}

for (const mirror of [null, 'previous-head']) {
  test(`unapproved head (${mirror}) receives failure and cannot schedule preview`, async () => {
    const run = harness({mirror});
    await controller({...run, phase: 'start'});
    assert.equal(run.outputs.allowed, 'false');
    assert.equal(run.checks[0].conclusion, 'failure');
    assert.match(run.checks[0].output.summary, /ok to test/);
  });
}

test('approved exact head schedules preview and publishes success only after recheck', async () => {
  const run = harness();
  await controller({...run, phase: 'start'});
  assert.equal(run.outputs.allowed, 'true');
  assert.equal(run.outputs.head, 'head');
  assert.equal(run.checks[0].status, 'in_progress');
  Object.assign(process.env, {RELEASE_SNAPSHOT: run.outputs.snapshot, RELEASE_CHECK: run.outputs.check, PREVIEW_RESULT: 'success', RELEASE_SUMMARY: 'passed'});
  await controller({...run, phase: 'finish'});
  assert.equal(run.updates[0].conclusion, 'success');
  assert.equal(run.updates[0].check_run_id, 123);
});

for (const change of [{head: 'new-head'}, {base: 'new-base'}, {title: 'feat!: changed'}, {body: 'edited'}, {mirror: 'old'}, {titleMode: 'COMMIT_OR_PR_TITLE'}, {messageMode: 'PR_BODY'}, {allowSquash: false}, {allowMerge: true}, {allowRebase: true}]) {
  test(`stale input ${JSON.stringify(change)} cannot publish success`, async () => {
    const before = harness();
    await controller({...before, phase: 'start'});
    Object.assign(process.env, {RELEASE_SNAPSHOT: before.outputs.snapshot, RELEASE_CHECK: before.outputs.check, PREVIEW_RESULT: 'success'});
    const after = harness(change);
    await controller({...after, phase: 'finish'});
    assert.equal(after.updates[0].conclusion, 'failure');
  });
}

test('failed preview cannot publish success', async () => {
  const run = harness();
  await controller({...run, phase: 'start'});
  Object.assign(process.env, {RELEASE_SNAPSHOT: run.outputs.snapshot, RELEASE_CHECK: run.outputs.check, PREVIEW_RESULT: 'failure', RELEASE_SUMMARY: 'alpha: 1.0.0 -> 2.0.0'});
  await controller({...run, phase: 'finish'});
  assert.equal(run.updates[0].conclusion, 'failure');
  assert.match(run.updates[0].output.summary, /alpha: 1.0.0 -> 2.0.0/);
});

test('approved mirror CI completion uses trusted current PR metadata', async () => {
  const run = harness();
  run.context.payload = {workflow_run: {event: 'push', head_repository: {full_name: 'example/project'}, head_branch: 'pull-request/42'}};
  await controller({...run, phase: 'start'});
  assert.equal(run.outputs.allowed, 'true');
});

for (const [branch, repository] of [['main', 'example/project'], ['pull-request/42', 'outsider/fork']]) {
  test(`unrelated CI completion ${repository}:${branch} starts no preview`, async () => {
    const run = harness();
    run.context.payload = {workflow_run: {event: 'push', head_repository: {full_name: repository}, head_branch: branch}};
    await controller({...run, phase: 'start'});
    assert.equal(run.outputs.allowed, undefined);
    assert.equal(run.checks.length, 0);
  });
}
