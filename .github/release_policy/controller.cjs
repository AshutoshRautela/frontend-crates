// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0
const squashSettings = repo => ({titleMode: repo.squash_merge_commit_title, messageMode: repo.squash_merge_commit_message,
  allowSquash: repo.allow_squash_merge, allowMerge: repo.allow_merge_commit, allowRebase: repo.allow_rebase_merge});

async function snapshot(github, owner, repo, number) {
  const {data: pr} = await github.rest.pulls.get({owner, repo, pull_number: number});
  const {data: repository} = await github.rest.repos.get({owner, repo});
  const {data: base} = await github.rest.repos.getBranch({owner, repo, branch: pr.base.ref});
  let mirror = null;
  try {
    const {data} = await github.rest.git.getRef({owner, repo, ref: `heads/pull-request/${number}`});
    mirror = data.object.sha;
  } catch (error) {
    if (error.status !== 404) throw error;
  }
  return {number, head: pr.head.sha, base: base.commit.sha, title: pr.title, body: pr.body || '',
    baseRef: pr.base.ref, state: pr.state, mirror, ...squashSettings(repository)};
}

function authorized(value) {
  return value.state === 'open' && value.mirror === value.head &&
    value.titleMode === 'PR_TITLE' && value.messageMode === 'BLANK' &&
    value.allowSquash === true && value.allowMerge === false && value.allowRebase === false;
}

function sameInputs(before, after) {
  return JSON.stringify(before) === JSON.stringify(after);
}

module.exports = async ({github, context, core, phase}) => {
  const {owner, repo} = context.repo;
  if (phase === 'start') {
    const event = context.payload;
    let number = event.pull_request?.number;
    if (!number) {
      const run = event.workflow_run;
      // workflow_run is loaded from the default branch, including mirror pushes.
      if (run.event !== 'push' || run.head_repository.full_name !== `${owner}/${repo}`) return;
      const match = /^pull-request\/(\d+)$/.exec(run.head_branch);
      if (!match) return;
      number = Number(match[1]);
    }
    const value = await snapshot(github, owner, repo, number);
    const allowed = authorized(value);
    const {data: check} = await github.rest.checks.create({owner, repo, name: 'release-policy', head_sha: value.head,
      status: allowed ? 'in_progress' : 'completed', ...(allowed ? {} : {conclusion: 'failure'}),
      output: {title: allowed ? 'Previewing release' : 'Release preview needs authorization',
        summary: allowed ? 'Checking the current base and PR title.' :
          'Ask a maintainer to run `/ok to test <current-head-sha>`. The approved pull-request mirror must equal the current PR head. Repository merge settings must permit only squash merges, using PR_TITLE and BLANK.'}});
    core.setOutput('snapshot', JSON.stringify(value));
    core.setOutput('check', String(check.id));
    core.setOutput('allowed', String(allowed));
    core.setOutput('head', value.head);
    return;
  }
  const before = JSON.parse(process.env.RELEASE_SNAPSHOT);
  const after = await snapshot(github, owner, repo, before.number);
  const fresh = sameInputs(before, after) && authorized(after);
  const passed = fresh && process.env.PREVIEW_RESULT === 'success';
  // Only update our own check. A superseded run cannot overwrite its successor.
  await github.rest.checks.update({owner, repo, check_run_id: Number(process.env.RELEASE_CHECK),
    status: 'completed', conclusion: passed ? 'success' : 'failure',
    output: {title: passed ? 'Release policy passed' : 'Release policy failed',
      summary: !fresh ? 'PR metadata, authorization, base, or squash settings changed. Run a fresh release preview.' :
        process.env.RELEASE_SUMMARY || 'Preview execution failed. Inspect the preview job logs.'}});
};
module.exports.authorized = authorized;
module.exports.sameInputs = sameInputs;
