(function () {
  'use strict';

  var selectedRow = null;

  window.renderTraceDetail = function (container, data) {
    container.innerHTML = '';
    selectedRow = null;

    var hdr = el('div', 'detail-header');
    var back = el('a', 'back-link', '← traces');
    back.href = '#/';
    hdr.appendChild(back);
    hdr.appendChild(el('h2', null, data.trace.name || data.trace.id));

    var headerActions = el('div', 'detail-header-actions');
    var copyId = el('button', 'btn btn-sm copy-id-btn', 'copy ID');
    copyId.title = data.trace.id;
    copyId.addEventListener('click', function () {
      navigator.clipboard.writeText(data.trace.id).then(function () {
        copyId.textContent = 'copied';
        setTimeout(function () { copyId.textContent = 'copy ID'; }, 1500);
      }).catch(function () {});
    });
    headerActions.appendChild(copyId);

    var config = window.appConfig || {};
    if (config.langfuseHost) {
      var lfLink = el('a', 'btn btn-sm langfuse-link', 'open in Langfuse ↗');
      lfLink.href = config.langfuseHost.replace(/\/$/, '') + '/trace/' + encodeURIComponent(data.trace.id);
      lfLink.target = '_blank';
      lfLink.rel = 'noopener';
      headerActions.appendChild(lfLink);
    }

    var exportBtn = el('a', 'btn btn-sm', 'export JSON');
    exportBtn.href = '/api/traces/' + encodeURIComponent(data.trace.id) + '/export';
    exportBtn.download = 'pistui-' + data.trace.id + '.json';
    headerActions.appendChild(exportBtn);

    hdr.appendChild(headerActions);
    container.appendChild(hdr);

    var traceIo = buildTraceIoPanel(data.trace);
    if (traceIo) container.appendChild(traceIo);

    var meta = renderMetadata(data.trace);
    if (meta) container.appendChild(meta);

    container.appendChild(renderStats(data.stats));

    var notices = renderTraceNotices(data.stats);
    if (notices) container.appendChild(notices);

    if (data.stats.modelBreakdown && data.stats.modelBreakdown.length > 0) {
      container.appendChild(renderModelBreakdown(data.stats.modelBreakdown));
    }

    var renderedIds = {};
    (function collectIds(nodes) {
      (nodes || []).forEach(function (n) {
        renderedIds[n.id] = true;
        collectIds(n.children);
      });
    })(data.tree);
    var traceScores = (data.scores || []).filter(function (s) {
      return s.observationId == null || !renderedIds[s.observationId];
    });
    if (traceScores.length > 0) {
      container.appendChild(renderScores(traceScores));
    }

    if (window.renderExplainButton) {
      window.renderExplainButton(container, data.trace.id);
    }

    var treeFilter = el('div', 'filter-bar');
    var input = el('input', 'filter-input');
    input.type = 'text';
    input.placeholder = 'filter by name, model, status, kind…  (/ to focus)';
    input.addEventListener('input', function (e) {
      filterTree(treeContainer, e.target.value.toLowerCase());
      selectedRow = null;
      clearSelection();
    });
    treeFilter.appendChild(input);
    container.appendChild(treeFilter);

    var totalMs = data.stats.totalDurationMs;
    var traceStart = findTraceStart(data.tree);

    var treeContainer = el('div', 'tree-container');
    data.tree.forEach(function (node) {
      treeContainer.appendChild(renderNode(node, 0, totalMs, traceStart));
    });
    container.appendChild(treeContainer);
  };

  window.handleTreeKeydown = function (e) {
    if (e.key === 'j') { e.preventDefault(); selectSpanRow(1); }
    if (e.key === 'k') { e.preventDefault(); selectSpanRow(-1); }
    if (e.key === 'Enter' && selectedRow) {
      e.preventDefault();
      toggleExpandSelected();
    }
    if (e.key === 'o' && selectedRow) {
      e.preventDefault();
      toggleDetailSelected();
    }
  };

  function getVisibleRows() {
    var all = document.querySelectorAll('.node-row');
    return Array.from(all).filter(function (r) {
      return r.offsetParent !== null;
    });
  }

  function clearSelection() {
    if (selectedRow) selectedRow.classList.remove('selected');
    selectedRow = null;
  }

  function selectSpanRow(delta) {
    var rows = getVisibleRows();
    if (rows.length === 0) return;

    var curIdx = -1;
    if (selectedRow) {
      curIdx = rows.indexOf(selectedRow);
      selectedRow.classList.remove('selected');
    }

    if (curIdx === -1) {
      curIdx = delta > 0 ? 0 : rows.length - 1;
    } else {
      curIdx += delta;
    }

    if (curIdx < 0) curIdx = 0;
    if (curIdx >= rows.length) curIdx = rows.length - 1;

    selectedRow = rows[curIdx];
    selectedRow.classList.add('selected');
    selectedRow.scrollIntoView({ block: 'nearest' });
  }

  function toggleExpandSelected() {
    if (!selectedRow) return;
    var toggle = selectedRow.querySelector('.toggle');
    var wrapper = selectedRow.closest('.tree-node');
    if (!wrapper) return;
    var childrenEl = wrapper.querySelector(':scope > .node-children');
    if (toggle && !toggle.classList.contains('leaf') && childrenEl) {
      toggle.classList.toggle('expanded');
      childrenEl.classList.toggle('collapsed');
    }
  }

  function toggleDetailSelected() {
    if (!selectedRow) return;
    var wrapper = selectedRow.closest('.tree-node');
    if (!wrapper) return;
    var detail = wrapper.querySelector(':scope > .node-detail');
    if (detail) detail.classList.toggle('open');
  }

  function findTraceStart(tree) {
    var min = Infinity;
    function walk(nodes) {
      for (var i = 0; i < nodes.length; i++) {
        var t = new Date(nodes[i].startTime).getTime();
        if (t < min) min = t;
        walk(nodes[i].children);
      }
    }
    walk(tree);
    return min;
  }

  function buildTraceIoPanel(trace) {
    var hasInput = trace.input != null;
    var hasOutput = trace.output != null;
    if (!hasInput && !hasOutput) return null;

    var panel = el('div', 'trace-io-panel');
    var tabs = el('div', 'detail-tabs');
    var sections = [];

    if (hasInput) addTab(tabs, sections, panel, 'Input', trace.input);
    if (hasOutput) addTab(tabs, sections, panel, 'Output', trace.output);

    panel.appendChild(tabs);
    sections.forEach(function (s) { panel.appendChild(s); });

    if (tabs.children.length > 0) tabs.children[0].click();

    return panel;
  }

  function renderStats(stats) {
    var bar = el('div', 'stats-bar');
    stat(bar, 'Duration', stats.totalDurationMs != null ? fmtDuration(stats.totalDurationMs) : '-', 'stat-duration');
    stat(bar, 'Spans', stats.spanCount);
    stat(bar, 'Generations', stats.generationCount);
    stat(bar, 'Events', stats.eventCount);
    if (stats.errorCount > 0) stat(bar, 'Errors', stats.errorCount, 'stat-errors');
    stat(bar, 'Tokens in', stats.totalTokensIn.toLocaleString(), 'stat-tokens-in');
    stat(bar, 'Tokens out', stats.totalTokensOut.toLocaleString(), 'stat-tokens-out');
    stat(bar, 'Cost', fmtCost(stats.totalCost), 'stat-cost');
    return bar;
  }

  function stat(bar, label, value, cls) {
    var item = el('div', 'stat-item' + (cls ? ' ' + cls : ''));
    item.appendChild(el('span', 'stat-label', label));
    item.appendChild(el('span', 'stat-value', String(value)));
    bar.appendChild(item);
  }

  function renderTraceNotices(stats) {
    var box = el('div', 'trace-notices');
    if (stats.truncated) {
      box.appendChild(el('div', 'trace-notice warn',
        'This trace was large and has been trimmed — some observations are not shown.'));
    }
    if (stats.orphanCount > 0) {
      var n = stats.orphanCount;
      box.appendChild(el('div', 'trace-notice info',
        n + ' observation' + (n === 1 ? '' : 's') + ' had a missing parent and ' +
        (n === 1 ? 'was' : 'were') + ' re-rooted to the top.'));
    }
    return box.children.length > 0 ? box : null;
  }

  function renderNode(node, depth, totalMs, traceStart) {
    var wrapper = el('div', 'tree-node depth-' + depth);
    wrapper.dataset.name = (node.name || '').toLowerCase();
    wrapper.dataset.id = node.id;
    wrapper.dataset.status = node.status || '';
    wrapper.dataset.model = (node.model || '').toLowerCase();
    wrapper.dataset.kind = (node.kind || '').toLowerCase();

    var row = el('div', 'node-row');

    var toggle = el('span', 'toggle');
    toggle.textContent = '▶';
    if (node.children.length === 0) {
      toggle.classList.add('leaf');
    } else {
      toggle.classList.add('expanded');
    }
    row.appendChild(toggle);

    var kindClass = node.kind.toLowerCase();
    row.appendChild(el('span', 'kind-badge ' + kindClass, node.kind));

    var nameEl = el('span', 'node-name', node.name);
    if (node.status === 'error') nameEl.classList.add('node-status-error');
    row.appendChild(nameEl);

    var copySpan = el('span', 'copy-span-id', '⎘');
    copySpan.title = 'Copy span ID: ' + node.id;
    copySpan.tabIndex = 0;
    copySpan.addEventListener('click', function (e) {
      e.stopPropagation();
      navigator.clipboard.writeText(node.id).then(function () {
        copySpan.textContent = '✓';
        setTimeout(function () { copySpan.textContent = '⎘'; }, 1200);
      }).catch(function () {});
    });
    copySpan.addEventListener('keydown', function (e) {
      if (e.key === 'Enter' || e.key === ' ') {
        e.preventDefault();
        copySpan.click();
      }
    });
    row.appendChild(copySpan);

    if (node.model) {
      row.appendChild(el('span', 'node-model', node.model));
    }

    if (node.durationMs != null) {
      row.appendChild(el('span', 'node-duration', fmtDuration(node.durationMs)));
    }

    if (node.ttftMs != null && node.kind === 'GENERATION') {
      var ttft = el('span', 'node-ttft', 'ttft ' + fmtDuration(node.ttftMs));
      row.appendChild(ttft);
    }

    if (node.scores && node.scores.length > 0) {
      node.scores.forEach(function (s) {
        var badge = el('span', 'score-badge', fmtScoreBadge(s));
        badge.title = s.name + ': ' + fmtScoreValue(s);
        row.appendChild(badge);
      });
    }

    if (totalMs && totalMs > 0 && traceStart !== Infinity) {
      row.appendChild(buildWaterfall(node, totalMs, traceStart));
    }

    wrapper.appendChild(row);

    var detail = buildDetailPanel(node);
    wrapper.appendChild(detail);

    var childrenEl = el('div', 'node-children');
    node.children.forEach(function (child) {
      childrenEl.appendChild(renderNode(child, depth + 1, totalMs, traceStart));
    });
    wrapper.appendChild(childrenEl);

    row.addEventListener('click', function (e) {
      e.stopPropagation();
      if (selectedRow) selectedRow.classList.remove('selected');
      selectedRow = row;
      row.classList.add('selected');

      if (e.detail === 2) {
        detail.classList.toggle('open');
      } else if (node.children.length > 0) {
        toggle.classList.toggle('expanded');
        childrenEl.classList.toggle('collapsed');
      }
    });

    return wrapper;
  }

  function fmtScoreBadge(s) {
    if (s.dataType === 'BOOLEAN') return s.value === 1 ? '✓' : '✗';
    if (s.dataType === 'CATEGORICAL') return s.stringValue || s.name.slice(0, 4);
    if (s.value != null) return String(Math.round(s.value * 100) / 100);
    return s.stringValue || '?';
  }

  function fmtScoreValue(s) {
    if (s.dataType === 'BOOLEAN') return s.value === 1 ? 'true' : 'false';
    if (s.value != null) return String(s.value);
    return s.stringValue || '-';
  }

  function buildWaterfall(node, totalMs, traceStart) {
    var container = el('div', 'waterfall');

    var nodeStart = new Date(node.startTime).getTime();
    var offsetMs = nodeStart - traceStart;
    var offsetPct = Math.max(0, Math.min(100, (offsetMs / totalMs) * 100));

    var isMarker = node.kind === 'EVENT' || node.durationMs == null;
    var widthPct = 0;
    if (!isMarker && node.durationMs != null) {
      widthPct = Math.max(0.5, Math.min(100 - offsetPct, (node.durationMs / totalMs) * 100));
    }

    var bar = el('div', 'waterfall-bar ' + node.kind.toLowerCase());
    bar.style.left = offsetPct + '%';

    if (isMarker) {
      bar.classList.add('marker');
    } else {
      bar.style.width = widthPct + '%';
    }

    if (node.status === 'error') {
      bar.classList.add('error');
    }

    var tip = node.name;
    if (node.durationMs != null) tip += ' - ' + fmtDuration(node.durationMs);
    bar.title = tip;

    container.appendChild(bar);
    return container;
  }

  function buildDetailPanel(node) {
    var panel = el('div', 'node-detail');

    var tabs = el('div', 'detail-tabs');
    var sections = [];

    if (node.input != null) {
      addTab(tabs, sections, panel, 'Input', node.input);
    }
    if (node.output != null) {
      addTab(tabs, sections, panel, 'Output', node.output);
    }
    if (node.statusMessage) {
      addTab(tabs, sections, panel, 'Error', node.statusMessage);
    }
    if (node.metadata != null) {
      addTab(tabs, sections, panel, 'Metadata', node.metadata);
    }

    if (tabs.children.length === 0) {
      panel.appendChild(el('div', 'empty-state', 'no data'));
      return panel;
    }

    panel.appendChild(tabs);
    sections.forEach(function (s) { panel.appendChild(s); });

    if (tabs.children.length > 0) {
      tabs.children[0].click();
    }

    var actions = el('div', 'detail-actions');
    var copyBtn = el('button', 'btn', 'copy');
    copyBtn.addEventListener('click', function () {
      var active = panel.querySelector('.detail-content:not([style*="none"])');
      if (active) {
        navigator.clipboard.writeText(active.textContent).then(function () {
          copyBtn.textContent = 'copied';
          setTimeout(function () { copyBtn.textContent = 'copy'; }, 1500);
        }).catch(function () {});
      }
    });
    actions.appendChild(copyBtn);
    panel.appendChild(actions);

    return panel;
  }

  function addTab(tabs, sections, panel, label, data) {
    var content = el('div', 'detail-content');
    var pre = el('pre');
    pre.textContent = typeof data === 'string' ? data : JSON.stringify(data, null, 2);
    content.appendChild(pre);
    content.style.display = 'none';
    sections.push(content);

    var tab = el('div', 'detail-tab', label);
    tab.addEventListener('click', function () {
      Array.from(tabs.children).forEach(function (t) { t.classList.remove('active'); });
      sections.forEach(function (s) { s.style.display = 'none'; });
      tab.classList.add('active');
      content.style.display = 'block';
    });
    tabs.appendChild(tab);
  }

  function renderMetadata(trace) {
    var items = [];
    if (trace.userId) items.push(['User', trace.userId]);
    if (trace.sessionId) items.push(['Session', trace.sessionId]);
    if (trace.release) items.push(['Release', trace.release]);
    if (trace.timestamp) items.push(['Timestamp', new Date(trace.timestamp).toLocaleString()]);
    var hasTags = Array.isArray(trace.tags) && trace.tags.length > 0;
    if (items.length === 0 && !hasTags) return null;

    var bar = el('div', 'metadata-bar');
    items.forEach(function (pair) {
      var item = el('div', 'meta-item');
      item.appendChild(el('span', 'meta-label', pair[0]));
      item.appendChild(el('span', 'meta-value', pair[1]));
      bar.appendChild(item);
    });

    if (hasTags) {
      var tagsItem = el('div', 'meta-item');
      tagsItem.appendChild(el('span', 'meta-label', 'Tags'));
      var tagsWrap = el('span', 'meta-tags');
      trace.tags.forEach(function (tag) {
        tagsWrap.appendChild(el('span', 'tag', tag));
      });
      tagsItem.appendChild(tagsWrap);
      bar.appendChild(tagsItem);
    }

    return bar;
  }

  function renderModelBreakdown(breakdown) {
    var section = el('div', 'model-breakdown');
    var heading = el('div', 'breakdown-heading', 'Models');
    section.appendChild(heading);

    var table = el('table', 'breakdown-table');
    var thead = el('thead');
    var headRow = el('tr');
    ['Model', 'Calls', 'Tokens in', 'Tokens out', 'Cost'].forEach(function (h) {
      headRow.appendChild(el('th', null, h));
    });
    thead.appendChild(headRow);
    table.appendChild(thead);

    var tbody = el('tbody');
    breakdown.forEach(function (m) {
      var row = el('tr');
      row.appendChild(el('td', 'model-name', m.model));
      row.appendChild(el('td', null, String(m.count)));
      row.appendChild(el('td', null, m.tokensIn.toLocaleString()));
      row.appendChild(el('td', null, m.tokensOut.toLocaleString()));
      row.appendChild(el('td', null, fmtCost(m.cost)));
      tbody.appendChild(row);
    });
    table.appendChild(tbody);
    section.appendChild(table);
    return section;
  }

  function renderScores(scores) {
    var section = el('div', 'scores-section');
    var heading = el('div', 'scores-heading', 'Scores');
    section.appendChild(heading);

    var list = el('div', 'scores-list');
    scores.forEach(function (s) {
      var item = el('div', 'score-item');
      item.appendChild(el('span', 'score-name', s.name));

      var val;
      if (s.dataType === 'BOOLEAN') {
        val = s.value === 1 ? 'true' : 'false';
      } else if (s.dataType === 'CATEGORICAL') {
        val = s.stringValue || '-';
      } else {
        val = s.value != null ? String(s.value) : (s.stringValue || '-');
      }
      item.appendChild(el('span', 'score-value', val));

      if (s.comment) {
        item.appendChild(el('span', 'score-comment', s.comment));
      }
      if (s.source) {
        item.appendChild(el('span', 'score-source', s.source));
      }
      list.appendChild(item);
    });
    section.appendChild(list);
    return section;
  }

  window.renderSingleNode = renderNode;

  function filterTree(container, query) {
    var nodes = container.querySelectorAll('.tree-node');
    if (!query) {
      nodes.forEach(function (n) { n.style.display = ''; });
      return;
    }
    nodes.forEach(function (n) { n.style.display = 'none'; });
    nodes.forEach(function (n) {
      var matches = (n.dataset.name && n.dataset.name.includes(query)) ||
        (n.dataset.status && n.dataset.status.includes(query)) ||
        (n.dataset.model && n.dataset.model.includes(query)) ||
        (n.dataset.kind && n.dataset.kind.includes(query));
      if (matches) showWithAncestors(n);
    });
  }

  function showWithAncestors(node) {
    node.style.display = '';
    var children = node.querySelector('.node-children');
    if (children) children.classList.remove('collapsed');
    var toggle = node.querySelector('.toggle');
    if (toggle && !toggle.classList.contains('leaf')) toggle.classList.add('expanded');
    var parent = node.parentElement;
    while (parent) {
      if (parent.classList && parent.classList.contains('tree-node')) {
        parent.style.display = '';
        var pc = parent.querySelector('.node-children');
        if (pc) pc.classList.remove('collapsed');
        var pt = parent.querySelector('.toggle');
        if (pt && !pt.classList.contains('leaf')) pt.classList.add('expanded');
      }
      parent = parent.parentElement;
    }
  }

  function el(tag, cls, text) {
    return window.el(tag, cls, text);
  }
})();
