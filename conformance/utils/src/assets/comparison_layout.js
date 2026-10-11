// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0
// Harry's comparison layout, rendered from the report model and existing cell payloads.
(function () {
  'use strict';

  const page = window.__CONFORMANCE_PAGE;
  if (!page || !Array.isArray(page.tabs)) return;

  const engineOrder = ['dynamo', 'sglang', 'vllm'];
  const engineNames = { dynamo: 'Dynamo', sglang: 'SGLang', vllm: 'vLLM' };
  const engineColors = { dynamo: 'var(--engine-dynamo)', sglang: 'var(--engine-sglang)', vllm: 'var(--engine-vllm)' };
  const statusNames = {
    pass: 'Matches reference', documented: 'Documented difference', diverge: 'Disagreement',
    leak: 'Markup leak', error: 'Parser error', expected: 'Expected error',
    unsupported: 'No parser for family', missing: 'No captured result',
  };
  const statusClasses = ['pass', 'documented', 'diverge', 'leak', 'error', 'expected', 'unsupported', 'missing'];

  function escapeHtml(value) {
    return String(value == null ? '' : value).replace(/[&<>"']/g, ch => ({
      '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;',
    })[ch]);
  }

  function engineKey(candidate) {
    const impl = String(candidate.impl || candidate.key || '').toLowerCase();
    if (impl.startsWith('dynamo')) return 'dynamo';
    if (impl.startsWith('sglang')) return 'sglang';
    if (impl.startsWith('vllm')) return 'vllm';
    return impl;
  }

  function candidateVersion(candidate) {
    const mode = candidate.parse_mode || '';
    return [candidate.version, mode].filter(Boolean).join(' · ') || candidate.label || candidate.key;
  }

  function candidatesFor(tab) {
    return (tab.candidates || []).filter(candidate => candidate && candidate.key && candidate.impl !== 'golden');
  }

  function selectedKeys(panel) {
    const ctl = panel.querySelector('.cmpctl');
    if (!ctl) return [];
    return Array.from(ctl.querySelectorAll('input.cmp-on:checked')).map(input => input.value);
  }

  function referenceKey(panel) {
    const selected = panel.querySelector('.cmp-ref:checked');
    return selected ? selected.value : null;
  }

  function tooltipCandidate(cell, key) {
    const candidates = (cell && cell.tooltip && cell.tooltip.candidates) || [];
    return candidates.find(candidate => candidate.key === key) || null;
  }

  function outputStatus(tab, cell, candidate, reference) {
    if (!cell || cell.kind === 'blank') return 'missing';
    if (cell.kind === 'missing') return 'missing';
    const entry = (cell.cmp || {})[candidate.key];
    const candidateTip = tooltipCandidate(cell, candidate.key);
    const block = candidateTip && candidateTip.block;
    if (!entry || entry.na === 1) {
      return block && block.unavailable ? 'unsupported' : 'missing';
    }
    if (entry.err === 1 || (block && (block.exception || (block.error && typeof block.error === 'object')))) return 'error';
    if (block && typeof block.error === 'string') return 'expected';
    if (entry.leak === 1 || (candidateTip && candidateTip.leak)) return 'leak';

    const oracleTab = tab.id === 'tab-unified';
    const compareKey = oracleTab ? 'golden' : reference;
    const compareEntry = (cell.cmp || {})[compareKey];
    if (!compareEntry || compareEntry.na === 1 || compareEntry.err === 1) return 'pass';
    if (entry.sig === compareEntry.sig) return 'pass';
    if (cell.known_divergence || (block && (block.explanation || block.reason))) return 'documented';
    return 'diverge';
  }

  function eligibleData(cell, candidate) {
    const entry = cell && cell.cmp && cell.cmp[candidate.key];
    return !!(entry && entry.na !== 1);
  }

  function makeHeader() {
    const originalTitle = document.querySelector('h1');
    const generated = document.querySelector('p.generated');
    const meta = page.meta || {};
    const shortSha = meta.short_sha || (meta.sha ? meta.sha.slice(0, 12) : '');
    const commitHref = meta.sha ? 'https://github.com/ai-dynamo/frontend-crates/commit/' + encodeURIComponent(meta.sha) : '';
    const header = document.createElement('header');
    header.className = 'report-header';
    header.innerHTML = '<div><div class="report-crumb"><b>ai-dynamo / frontend-crates</b> · conformance</div>'
      + '<h1>Parser Conformance</h1></div><div class="report-meta">'
      + (shortSha ? '<span class="meta-pill">commit <a href="' + escapeHtml(commitHref) + '"><code>' + escapeHtml(shortSha) + '</code></a></span>' : '')
      + (meta.stamp ? '<span class="meta-pill">generated ' + escapeHtml(meta.stamp) + '</span>' : '')
      + '</div>';
    const themeButton = document.querySelector('[data-theme-toggle]');
    if (themeButton) {
      themeButton.classList.add('header-theme-toggle');
      header.querySelector('.report-meta').appendChild(themeButton);
    }
    if (originalTitle && originalTitle.parentNode) originalTitle.remove();
    if (generated && generated.parentNode) generated.remove();
    document.body.insertBefore(header, document.body.firstChild);
  }

  function statusFor(tab, row, column, candidate, reference) {
    return outputStatus(tab, row.cells && row.cells[column.sub], candidate, reference);
  }

  function peerHasData(tab, row, column) {
    return candidatesFor(tab).some(candidate => engineKey(candidate) !== 'dynamo'
      && eligibleData(row.cells && row.cells[column.sub], candidate));
  }

  function createUi(panel, tab) {
    const candidates = candidatesFor(tab);
    const query = new URLSearchParams(window.location.search);
    const hasCompareOverride = query.has('base_' + tab.id) || query.has('cmp_' + tab.id);
    if (!hasCompareOverride) {
      const firstCandidateByEngine = new Map();
      candidates.forEach(candidate => {
        const key = engineKey(candidate);
        if (!firstCandidateByEngine.has(key)) firstCandidateByEngine.set(key, candidate.key);
      });
      const control = panel.querySelector('.cmpctl');
      if (control) {
        candidates.forEach(candidate => {
          const input = Array.from(control.querySelectorAll('input.cmp-on')).find(item => item.value === candidate.key);
          if (!input) return;
          input.checked = candidate.default_bucket === 'A' || candidate.default_bucket === 'B'
            || firstCandidateByEngine.get(engineKey(candidate)) === candidate.key;
        });
      }
    }
    const candidatesByEngine = new Map();
    candidates.forEach(candidate => {
      const key = engineKey(candidate);
      if (!candidatesByEngine.has(key)) candidatesByEngine.set(key, []);
      candidatesByEngine.get(key).push(candidate);
    });
    const state = {
      layout: 'split', focus: 'all', peerOnly: false, search: '',
      groups: new Set((tab.column_groups || []).map(group => group.key)),
      shown: new Set(selectedKeys(panel)),
      chosen: new Map(),
      selectedCell: null,
    };
    candidatesByEngine.forEach((items, key) => {
      const selected = items.find(candidate => state.shown.has(candidate.key)) || items[0];
      if (selected) state.chosen.set(key, selected.key);
    });
    const ui = document.createElement('div');
    ui.className = 'comparison-ui';
    const engines = document.createElement('div');
    engines.className = 'engine-cards';
    engines.setAttribute('aria-label', 'Engine comparisons');
    const toolbar = document.createElement('div');
    toolbar.className = 'comparison-toolbar';
    const legend = document.createElement('div');
    legend.className = 'comparison-legend';
    legend.innerHTML = statusClasses.map(status => '<span><i class="legend-swatch status-' + status + '"></i>'
      + escapeHtml(statusNames[status]) + '</span>').join('');
    const categories = document.createElement('div');
    categories.className = 'category-chips';
    categories.setAttribute('aria-label', 'Case categories');
    const matrixCard = document.createElement('div');
    matrixCard.className = 'comparison-matrix-card';
    const scroll = document.createElement('div');
    scroll.className = 'comparison-matrix-scroll';
    const matrix = document.createElement('table');
    matrix.className = 'comparison-matrix';
    matrix.setAttribute('aria-label', 'Parser conformance comparison matrix');
    scroll.appendChild(matrix);
    matrixCard.appendChild(scroll);
    ui.append(engines, toolbar, legend, categories, matrixCard);

    const legacy = document.createElement('details');
    legacy.className = 'legacy-report-details';
    legacy.innerHTML = '<summary>Detailed comparison controls and case reference table</summary>';
    const legacyRequested = query.get('view') === 'details' || query.has('transpose')
      || query.has('base_' + tab.id) || query.has('cmp_' + tab.id);
    legacy.open = legacyRequested;
    const legacyBody = document.createElement('div');
    legacyBody.className = 'legacy-report-body';
    while (panel.firstChild) legacyBody.appendChild(panel.firstChild);
    legacy.appendChild(legacyBody);
    panel.append(ui, legacy);

    const control = document.createElement('div');
    control.className = 'comparison-controls';
    control.innerHTML = '<span class="control-label">Layout</span><div class="segmented" data-layout-control>'
      + '<button type="button" data-layout="split" aria-pressed="true">Split cells</button>'
      + '<button type="button" data-layout="stacked" aria-pressed="false">Stacked rows</button></div>'
      + '<span class="control-divider"></span><span class="control-label">Focus</span>'
      + '<div class="segmented" data-focus-control><button type="button" data-focus="all" aria-pressed="true">All</button>'
      + '<button type="button" data-focus="disagreements" aria-pressed="false">Disagreements</button>'
      + '<button type="button" data-focus="problems" aria-pressed="false">Problems</button></div>'
      + '<span class="control-divider"></span><label class="peer-filter"><input type="checkbox" data-peer-only> Only cases with peer data</label>'
      + '<label class="model-search"><span aria-hidden="true">⌕</span><input type="search" data-case-search placeholder="Filter models or cases…" aria-label="Filter models or cases"></label>';
    toolbar.appendChild(control);

    const drawer = document.createElement('aside');
    drawer.className = 'comparison-drawer';
    drawer.setAttribute('aria-hidden', 'true');
    drawer.innerHTML = '<div class="drawer-scrim" data-drawer-close></div><section class="drawer-panel" role="dialog" aria-modal="true" aria-labelledby="drawer-title">'
      + '<header class="drawer-header"><div><div class="drawer-case-id" data-drawer-id></div><h2 id="drawer-title" data-drawer-title>Case output</h2><div data-drawer-description></div></div>'
      + '<button type="button" class="drawer-close" data-drawer-close aria-label="Close case output">×</button></header>'
      + '<div class="drawer-content" data-drawer-content></div></section>';
    document.body.appendChild(drawer);

    function renderCards(columns, rows) {
      const reference = referenceKey(panel);
      const enginesInOrder = engineOrder.filter(key => candidatesByEngine.has(key));
      candidatesByEngine.forEach((_items, key) => { if (!enginesInOrder.includes(key)) enginesInOrder.push(key); });
      engines.innerHTML = enginesInOrder.map(key => {
        const items = candidatesByEngine.get(key) || [];
        const selectedKey = state.chosen.get(key);
        const selected = items.find(item => item.key === selectedKey) || items[0];
        const counts = Object.fromEntries(statusClasses.map(status => [status, 0]));
        const totalized = new Set();
        let captured = 0;
        for (const row of rows) {
          for (const column of columns) {
            const cell = row.cells && row.cells[column.sub];
            const identity = (row.model_label || row.family || '') + '|' + (column.case_id || column.sub);
            if (totalized.has(identity)) continue;
            totalized.add(identity);
            const status = statusFor(tab, row, column, selected, reference);
            counts[status] += 1;
            if (!['unsupported', 'missing'].includes(status)) captured += 1;
          }
        }
        const denominator = counts.pass + counts.documented + counts.diverge + counts.leak + counts.error + counts.expected;
        const totalCases = statusClasses.reduce((total, status) => total + counts[status], 0);
        const success = counts.pass;
        const percent = denominator ? Math.round(success / denominator * 100) : null;
        const bar = statusClasses.map(status => totalCases
          ? '<span class="bar-' + status + '" style="width:' + (counts[status] / totalCases * 100) + '%"></span>' : '').join('');
        const engineName = engineNames[key] || items[0].label.split(/\s+/)[0];
        const options = items.map(item => '<option value="' + escapeHtml(item.key) + '"'
          + (item.key === selectedKey ? ' selected' : '') + '>' + escapeHtml(candidateVersion(item)) + '</option>').join('');
        const more = items.length > 1 ? '<details class="engine-version-list"><summary>Compare captured versions (' + items.length + ')</summary>'
          + items.map(item => '<label><input type="checkbox" data-candidate-show="' + escapeHtml(item.key) + '"'
            + (state.shown.has(item.key) ? ' checked' : '') + '> ' + escapeHtml(item.label) + '</label>').join('') + '</details>' : '';
        return '<article class="engine-card" style="--engine-color:' + (engineColors[key] || 'var(--accent)') + '">'
          + '<div class="engine-card-head"><div class="engine-name"><span class="engine-mark">' + escapeHtml(engineName.slice(0, 1)) + '</span>'
          + '<strong>' + escapeHtml(engineName) + '</strong></div><select data-candidate-select="' + escapeHtml(key) + '" aria-label="' + escapeHtml(engineName) + ' capture version">' + options + '</select></div>'
          + '<div class="engine-score"><strong>' + (percent == null ? '—' : percent + '%') + '</strong><span>'
          + escapeHtml(tab.id === 'tab-unified' ? 'matches GOLDEN' : 'no detected leak/error') + ' · ' + success + ' of ' + captured + ' captured</span></div>'
          + '<div class="engine-status-bar" role="img" aria-label="' + statusClasses.map(status => statusNames[status] + ': ' + counts[status]).join(', ') + '">' + bar + '</div>'
          + '<div class="engine-counts">' + statusClasses.filter(status => counts[status] > 0).map(status => '<span><i class="legend-swatch status-' + status + '"></i>'
            + counts[status] + ' ' + escapeHtml(statusNames[status].toLowerCase()) + '</span>').join('') + '</div>'
          + '<div class="engine-card-controls"><label><input type="checkbox" data-engine-show="' + escapeHtml(key) + '"'
          + (items.some(item => state.shown.has(item.key)) ? ' checked' : '') + '> Show</label>'
          + '<label><input type="radio" name="reference-' + escapeHtml(tab.id) + '" data-engine-reference="' + escapeHtml(selected.key) + '"'
          + (reference === selected.key ? ' checked' : '') + '> Reference</label>' + more + '</div></article>';
      }).join('');
    }

    function renderCategories(columns, categoryColumns) {
      categories.innerHTML = (tab.column_groups || []).map(group => {
        const count = categoryColumns.filter(column => column.group_key === group.key).length;
        return '<button type="button" class="category-chip" data-group="' + escapeHtml(group.key) + '" aria-pressed="'
          + state.groups.has(group.key) + '"><strong>' + escapeHtml(group.label) + '</strong><span>' + count + '</span></button>';
      }).join('');
    }

    function filteredView() {
      const query = state.search.trim().toLowerCase();
      const allRows = (tab.rows || []).filter(row => !row.section);
      let categoryColumns = tab.columns || [];
      const matchingColumns = query ? categoryColumns.filter(column => (column.label + ' ' + (column.case_id || '') + ' '
        + (column.desc || '')).toLowerCase().includes(query)) : [];
      const matchingRows = query ? allRows.filter(row => (row.model_label + ' ' + row.family + ' '
        + ((row.parser && row.parser.text) || '')).toLowerCase().includes(query)) : allRows;
      const rows = query && matchingColumns.length === 0 ? matchingRows : allRows;
      if (query && matchingColumns.length > 0) categoryColumns = matchingColumns;
      if (state.peerOnly) categoryColumns = categoryColumns.filter(column => allRows.some(row => peerHasData(tab, row, column)));
      const columns = categoryColumns.filter(column => state.groups.has(column.group_key));
      return { columns, categoryColumns, rows };
    }

    function cellCodes(row, column, visibleCandidates, reference) {
      const cell = row.cells && row.cells[column.sub];
      return visibleCandidates.map(candidate => ({
        candidate,
        status: statusFor(tab, row, column, candidate, reference),
        entry: cell && cell.cmp && cell.cmp[candidate.key],
      }));
    }

    function rowSummaries(row, columns, visibleCandidates, reference) {
      return visibleCandidates.map(candidate => {
        let pass = 0;
        let denominator = 0;
        for (const column of columns) {
          const status = statusFor(tab, row, column, candidate, reference);
          if (!['unsupported', 'missing'].includes(status)) denominator += 1;
          if (status === 'pass') pass += 1;
        }
        const percentage = denominator ? Math.round(pass / denominator * 100) : null;
        const key = engineKey(candidate);
        return '<div class="model-summary" style="--engine-color:' + (engineColors[key] || 'var(--accent)') + '"><b>'
          + escapeHtml((engineNames[key] || candidate.label).slice(0, 1)) + '</b><span class="model-summary-bar"><i style="width:'
          + (percentage == null ? 0 : percentage) + '%"></i></span><span>' + (percentage == null ? '—' : percentage + '%') + '</span></div>';
      });
    }

    function statusCell(row, column, visibleCandidates, reference, rowIndex, columnIndex, engineOnly) {
      const codes = cellCodes(row, column, visibleCandidates, reference);
      const signatures = codes.filter(code => code.entry && code.entry.na !== 1 && code.entry.err !== 1).map(code => code.entry.sig);
      const disagreement = new Set(signatures).size > 1;
      const problem = codes.some(code => ['diverge', 'leak', 'error', 'expected', 'missing', 'unsupported'].includes(code.status));
      const focused = state.focus === 'all' || (state.focus === 'disagreements' ? disagreement : problem);
      const segments = codes.map(code => '<i class="matrix-status status-' + code.status + '" title="'
        + escapeHtml((code.candidate.label || code.candidate.key) + ': ' + statusNames[code.status]) + '"></i>').join('');
      const label = tab.columns[columnIndex];
      return '<td class="matrix-cell' + (focused ? '' : ' focus-dim') + (engineOnly ? ' stacked-cell' : '') + '" tabindex="0" role="button"'
        + ' data-row="' + rowIndex + '" data-column="' + columnIndex + '" aria-label="' + escapeHtml((row.model_label || row.family) + ', ' + label.label + ': '
        + codes.map(code => (code.candidate.label || code.candidate.key) + ' ' + statusNames[code.status]).join('; ')) + '">'
        + '<span class="matrix-segments">' + segments + '</span></td>';
    }

    function renderMatrix(columns, rows) {
      const shown = candidates.filter(candidate => state.shown.has(candidate.key));
      const visibleCandidates = shown.length ? shown : candidates.filter(candidate => candidate.key === referenceKey(panel)).slice(0, 1);
      const reference = referenceKey(panel);
      const groupRuns = [];
      columns.forEach(column => {
        const last = groupRuns[groupRuns.length - 1];
        if (last && last.key === column.group_key) last.span += 1;
        else groupRuns.push({ key: column.group_key, span: 1 });
      });
      const groupHeader = '<tr class="matrix-group-row"><th class="matrix-corner">Model family</th>'
        + groupRuns.map(run => {
          const group = (tab.column_groups || []).find(item => item.key === run.key);
          return '<th colspan="' + run.span + '">' + escapeHtml(group ? group.label : run.key) + '</th>';
        }).join('') + '<th class="matrix-summary-head">Conformance</th></tr>';
      const caseHeader = '<tr class="matrix-case-row"><th class="matrix-corner">' + (state.layout === 'split'
        ? 'cell = ' + visibleCandidates.map(candidate => '<b style="color:' + (engineColors[engineKey(candidate)] || 'var(--accent)') + '">'
          + escapeHtml((engineNames[engineKey(candidate)] || candidate.label).slice(0, 1)) + '</b>').join(' · ') : 'Engine') + '</th>'
        + columns.map(column => '<th title="' + escapeHtml(column.desc || column.case_id || column.label) + '">' + escapeHtml(column.display_number || column.label) + '</th>').join('')
        + '<th class="matrix-summary-head"></th></tr>';
      let body = '';
      rows.forEach(row => {
        const rowIndex = (tab.rows || []).filter(item => !item.section).indexOf(row);
        const rowColumns = columns.map(column => ({ column, columnIndex: tab.columns.indexOf(column) }));
        const summaries = rowSummaries(row, columns, visibleCandidates, reference);
        if (state.layout === 'split') {
          body += '<tr class="matrix-model-row"><th scope="row" class="matrix-model"><strong>'
            + escapeHtml(row.model_label || row.family || '') + '</strong><small>' + escapeHtml(row.parser_label || row.family || '') + '</small></th>'
            + rowColumns.map(({ column, columnIndex }) => statusCell(row, column, visibleCandidates, reference, rowIndex, columnIndex, false)).join('')
            + '<td class="matrix-summary">' + summaries.join('') + '</td></tr>';
        } else {
          visibleCandidates.forEach((candidate, candidateIndex) => {
            body += '<tr class="matrix-model-row stacked-row"><th scope="row" class="matrix-model">'
              + (candidateIndex === 0 ? '<strong>' + escapeHtml(row.model_label || row.family || '') + '</strong>' : '')
              + '<small class="stacked-engine" style="--engine-color:' + (engineColors[engineKey(candidate)] || 'var(--accent)') + '">'
              + escapeHtml(engineNames[engineKey(candidate)] || candidate.label) + '</small></th>'
              + rowColumns.map(({ column, columnIndex }) => statusCell(row, column, [candidate], reference, rowIndex, columnIndex, true)).join('')
              + '<td class="matrix-summary">' + (summaries[candidateIndex] || '') + '</td></tr>';
          });
        }
      });
      matrix.innerHTML = '<thead>' + groupHeader + caseHeader + '</thead><tbody>' + body + '</tbody>';
      matrix.dataset.empty = String(columns.length === 0 || rows.length === 0);
    }

    function update() {
      state.shown = new Set(selectedKeys(panel));
      const { columns, categoryColumns, rows } = filteredView();
      renderCards(columns, rows);
      renderCategories(columns, categoryColumns);
      renderMatrix(columns, rows);
      ui.classList.toggle('layout-stacked', state.layout === 'stacked');
      control.querySelectorAll('[data-layout]').forEach(button => button.setAttribute('aria-pressed', String(button.dataset.layout === state.layout)));
      control.querySelectorAll('[data-focus]').forEach(button => button.setAttribute('aria-pressed', String(button.dataset.focus === state.focus)));
      const peer = control.querySelector('[data-peer-only]');
      if (peer) peer.checked = state.peerOnly;
      const search = control.querySelector('[data-case-search]');
      if (search && search.value !== state.search) search.value = state.search;
    }

    function changeCandidate(key, checked) {
      const ctl = panel.querySelector('.cmpctl');
      const input = ctl && Array.from(ctl.querySelectorAll('input.cmp-on')).find(item => item.value === key);
      if (!input || input.disabled) return;
      input.checked = checked;
      input.dispatchEvent(new Event('change', { bubbles: true }));
      update();
    }

    engines.addEventListener('change', event => {
      const select = event.target.closest('[data-candidate-select]');
      if (select) {
        const engine = select.dataset.candidateSelect;
        state.chosen.set(engine, select.value);
        const items = candidatesByEngine.get(engine) || [];
        items.forEach(candidate => { if (candidate.key !== select.value) changeCandidate(candidate.key, false); });
        changeCandidate(select.value, true);
        update();
        return;
      }
      const version = event.target.closest('[data-candidate-show]');
      if (version) { changeCandidate(version.dataset.candidateShow, version.checked); return; }
      const showEngine = event.target.closest('[data-engine-show]');
      if (showEngine) {
        const engine = showEngine.dataset.engineShow;
        const key = state.chosen.get(engine);
        if (key) changeCandidate(key, showEngine.checked);
        return;
      }
      const ref = event.target.closest('[data-engine-reference]');
      if (ref) {
        const input = Array.from(panel.querySelectorAll('.cmp-ref')).find(item => item.value === ref.dataset.engineReference);
        if (input) { input.checked = true; input.dispatchEvent(new Event('change', { bubbles: true })); }
        update();
      }
    });
    control.addEventListener('click', event => {
      const layoutButton = event.target.closest('[data-layout]');
      const focusButton = event.target.closest('[data-focus]');
      if (layoutButton) { state.layout = layoutButton.dataset.layout; update(); }
      if (focusButton) { state.focus = focusButton.dataset.focus; update(); }
    });
    control.addEventListener('change', event => {
      if (event.target.matches('[data-peer-only]')) { state.peerOnly = event.target.checked; update(); }
    });
    control.addEventListener('input', event => {
      if (event.target.matches('[data-case-search]')) { state.search = event.target.value; update(); }
    });
    categories.addEventListener('click', event => {
      const button = event.target.closest('[data-group]');
      if (!button) return;
      const key = button.dataset.group;
      if (state.groups.has(key)) state.groups.delete(key);
      else state.groups.add(key);
      update();
    });

    function closeDrawer() {
      drawer.classList.remove('open');
      drawer.setAttribute('aria-hidden', 'true');
      if (state.selectedCell) state.selectedCell.focus();
    }
    function openDrawer(cellNode) {
      const rowIndex = Number(cellNode.dataset.row);
      const columnIndex = Number(cellNode.dataset.column);
      const row = (tab.rows || []).filter(item => !item.section)[rowIndex];
      const column = (tab.columns || [])[columnIndex];
      const cell = row && column && row.cells && row.cells[column.sub];
      state.selectedCell = cellNode;
      drawer.querySelector('[data-drawer-id]').textContent = (cell && cell.case_id) || column.case_id || column.label;
      drawer.querySelector('[data-drawer-title]').textContent = column.desc || column.label;
      drawer.querySelector('[data-drawer-description]').textContent = [row.model_label, row.family].filter(Boolean).join(' · ');
      const content = drawer.querySelector('[data-drawer-content]');
      content.innerHTML = cell && cell.tooltip && window.__renderConformanceTooltip
        ? window.__renderConformanceTooltip(cell.tooltip)
        : '<p class="drawer-empty">' + escapeHtml(cell && cell.kind === 'missing' ? 'No fixture was captured for this case.' : 'No output details are available for this case.') + '</p>';
      const candidateKeys = new Set(selectedKeys(panel));
      content.querySelectorAll('.cand').forEach(section => {
        const key = Array.from(section.classList).find(name => name.startsWith('cand-'));
        section.classList.toggle('cand-on', key && candidateKeys.has(key.slice(5)));
        section.classList.toggle('cand-base', key && key.slice(5) === referenceKey(panel));
      });
      if (tab.id === 'tab-unified') {
        content.querySelectorAll('.cand-golden').forEach(section => section.classList.add('cand-on', 'cand-base'));
      }
      drawer.classList.add('open');
      drawer.setAttribute('aria-hidden', 'false');
      const closeButton = drawer.querySelector('.drawer-close');
      closeButton.focus();
    }
    matrix.addEventListener('click', event => {
      const cell = event.target.closest('.matrix-cell');
      if (cell) openDrawer(cell);
    });
    matrix.addEventListener('keydown', event => {
      const cell = event.target.closest('.matrix-cell');
      if (cell && (event.key === 'Enter' || event.key === ' ')) { event.preventDefault(); openDrawer(cell); }
    });
    drawer.addEventListener('click', event => { if (event.target.closest('[data-drawer-close]')) closeDrawer(); });
    drawer.addEventListener('keydown', event => {
      if (event.key !== 'Tab' || !drawer.classList.contains('open')) return;
      const focusable = Array.from(drawer.querySelectorAll('button:not([disabled]),a[href],input:not([disabled]),select:not([disabled]),[tabindex="0"]'));
      if (!focusable.length) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus(); }
      else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
    });
    document.addEventListener('keydown', event => { if (event.key === 'Escape' && drawer.classList.contains('open')) closeDrawer(); });
    document.addEventListener('click', event => {
      if (event.target.closest('[data-tab-target]')) {
        drawer.classList.remove('open');
        drawer.setAttribute('aria-hidden', 'true');
        state.selectedCell = null;
      }
    });
    panel.querySelector('.legacy-report-details').addEventListener('toggle', update);
    panel.addEventListener('change', event => { if (event.target.closest('.cmpctl')) update(); });
    function dismissVisibleTooltips() {
      document.querySelectorAll('.ttip.ttip-visible').forEach(tip => {
        const owner = tip._ttipOwner;
        if (owner && owner._ttipUnpin) owner._ttipUnpin();
        else tip.classList.remove('ttip-visible');
      });
    }
    const transposeObserver = new MutationObserver(() => {
      dismissVisibleTooltips();
      if (panel.classList.contains('active') && (legacyRequested
        || document.body.classList.contains('transpose-mode')
        || document.body.classList.contains('view-details'))) legacy.open = true;
      else legacy.open = false;
    });
    transposeObserver.observe(document.body, { attributes: true, attributeFilter: ['class'] });
    update();
  }

  function install() {
    makeHeader();
    document.querySelectorAll('.tab-panel').forEach(panel => {
      const tab = page.tabs.find(item => item.id === panel.id);
      if (tab) createUi(panel, tab);
    });
  }

  try { install(); }
  catch (error) { console.error('comparison_layout: failed to build comparison UI', error); }
})();
