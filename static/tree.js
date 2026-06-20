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
    container.appendChild(hdr);

    container.appendChild(renderStats(data.stats));

    if (window.renderExplainButton) {
      window.renderExplainButton(container, data.trace.id);
    }

    var treeFilter = el('div', 'filter-bar');
    var input = el('input', 'filter-input');
    input.type = 'text';
    input.placeholder = 'filter spans…  (/ to focus)';
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

  function renderStats(stats) {
    var bar = el('div', 'stats-bar');
    stat(bar, 'Duration', stats.totalDurationMs != null ? fmtDuration(stats.totalDurationMs) : '—', 'stat-duration');
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

  function renderNode(node, depth, totalMs, traceStart) {
    var wrapper = el('div', 'tree-node depth-' + depth);
    wrapper.dataset.name = (node.name || '').toLowerCase();
    wrapper.dataset.id = node.id;

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

    if (node.model) {
      row.appendChild(el('span', 'node-model', node.model));
    }

    if (node.durationMs != null) {
      row.appendChild(el('span', 'node-duration', fmtDuration(node.durationMs)));
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
    if (node.durationMs != null) tip += ' — ' + fmtDuration(node.durationMs);
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
        });
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

  function filterTree(container, query) {
    var nodes = container.querySelectorAll('.tree-node');
    if (!query) {
      nodes.forEach(function (n) { n.style.display = ''; });
      return;
    }
    nodes.forEach(function (n) { n.style.display = 'none'; });
    nodes.forEach(function (n) {
      if (n.dataset.name && n.dataset.name.includes(query)) {
        showWithAncestors(n);
      }
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
