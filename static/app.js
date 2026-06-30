(function () {
  'use strict';

  var traces = [];
  var sortCol = 'timestamp';
  var sortAsc = false;
  var filterText = '';
  var selectedTraceId = null;
  var appConfig = {};
  var viewMode = 'traces';
  var listLoadedAt = null;
  var comparing = false;
  var compareSelection = [];

  window.addEventListener('hashchange', route);
  document.addEventListener('DOMContentLoaded', init);

  async function init() {
    try {
      var resp = await fetch('/api/config');
      if (resp.ok) appConfig = await resp.json();
    } catch (e) { /* proceed without config */ }

    window.appConfig = appConfig;

    if (appConfig.focusTrace) {
      location.hash = '#/traces/' + encodeURIComponent(appConfig.focusTrace);
      return;
    }
    route();
  }

  function route() {
    var hash = location.hash || '#/';
    var match = hash.match(/^#\/traces\/(.+)$/);
    var compareMatch = hash.match(/^#\/compare\/([^/]+)\/(.+)$/);
    if (compareMatch) {
      comparing = false;
      compareSelection = [];
      showCompare(decodeURIComponent(compareMatch[1]), decodeURIComponent(compareMatch[2]));
    } else if (match) {
      selectedTraceId = null;
      showTrace(decodeURIComponent(match[1]));
    } else if (hash === '#/sessions') {
      showSessions();
    } else {
      showList();
    }
  }

  async function showList() {
    var app = document.getElementById('app');
    viewMode = 'traces';
    app.innerHTML = '';
    app.appendChild(buildSkeleton());

    try {
      var resp = await fetch('/api/traces');
      if (!resp.ok) {
        var body = await resp.json().catch(function () { return {}; });
        throw new Error(body.error || resp.statusText);
      }
      traces = await resp.json();
      listLoadedAt = Date.now();
      renderList();
    } catch (e) {
      app.innerHTML = '<div class="error-msg">failed to load traces: ' + esc(e.message) + '</div>';
    }
  }

  async function showSessions() {
    var app = document.getElementById('app');
    viewMode = 'sessions';
    app.innerHTML = '';
    app.appendChild(buildSkeleton());

    try {
      var resp = await fetch('/api/sessions');
      if (!resp.ok) throw new Error('failed to load sessions');
      var sessions = await resp.json();
      renderSessions(sessions);
    } catch (e) {
      app.innerHTML = '<div class="error-msg">' + esc(e.message) + '</div>';
    }
  }

  function renderSessions(sessions) {
    var app = document.getElementById('app');
    app.innerHTML = '';

    var bar = el('div', 'filter-bar');
    var viewToggle = buildViewToggle();
    bar.appendChild(viewToggle);
    var refreshBtn = el('button', 'btn refresh-btn', '↻ refresh');
    refreshBtn.addEventListener('click', function () { showSessions(); });
    bar.appendChild(refreshBtn);
    app.appendChild(bar);

    if (sessions.length === 0) {
      var empty = el('div', 'empty-state');
      empty.textContent = 'no sessions found - traces without a session_id are not grouped';
      app.appendChild(empty);
      return;
    }

    var list = el('div', 'session-list');
    sessions.forEach(function (sess) {
      var card = el('div', 'session-card');

      var header = el('div', 'session-header');
      var name = el('span', 'session-name', sess.sessionId);
      header.appendChild(name);

      var meta = el('span', 'session-meta');
      meta.textContent = sess.traceCount + ' trace' + (sess.traceCount !== 1 ? 's' : '') +
        (sess.totalCost > 0 ? ' · ' + fmtCost(sess.totalCost) : '');
      header.appendChild(meta);

      card.appendChild(header);

      var traceList = el('div', 'session-traces collapsed');
      sess.traces.forEach(function (traceId) {
        var row = el('div', 'session-trace-row');
        var link = el('a', 'session-trace-link', traceId.slice(0, 20));
        link.href = '#/traces/' + encodeURIComponent(traceId);
        row.appendChild(link);
        traceList.appendChild(row);
      });
      card.appendChild(traceList);

      header.addEventListener('click', function () {
        traceList.classList.toggle('collapsed');
        card.classList.toggle('expanded');
      });

      list.appendChild(card);
    });

    app.appendChild(list);
  }

  function buildSkeleton() {
    var skel = el('div', 'skeleton');
    for (var i = 0; i < 8; i++) {
      var row = el('div', 'skeleton-row');
      row.appendChild(el('div', 'skeleton-cell wide'));
      row.appendChild(el('div', 'skeleton-cell narrow'));
      row.appendChild(el('div', 'skeleton-cell medium'));
      row.appendChild(el('div', 'skeleton-cell narrow'));
      skel.appendChild(row);
    }
    return skel;
  }

  function buildViewToggle() {
    var toggle = el('div', 'view-toggle');
    var tracesBtn = el('button', 'view-btn' + (viewMode === 'traces' ? ' active' : ''), 'traces');
    tracesBtn.addEventListener('click', function () { location.hash = '#/'; });
    var sessionsBtn = el('button', 'view-btn' + (viewMode === 'sessions' ? ' active' : ''), 'sessions');
    sessionsBtn.addEventListener('click', function () { location.hash = '#/sessions'; });
    toggle.appendChild(tracesBtn);
    toggle.appendChild(sessionsBtn);
    return toggle;
  }

  function renderList() {
    var filtered = traces.filter(function (t) {
      if (!filterText) return true;
      var q = filterText.toLowerCase();
      return (t.name || '').toLowerCase().includes(q) ||
             t.id.toLowerCase().includes(q) ||
             (t.tags || []).some(function (tag) { return tag.toLowerCase().includes(q); }) ||
             (t.userId || '').toLowerCase().includes(q) ||
             (t.release || '').toLowerCase().includes(q);
    });

    var sorted = filtered.slice().sort(function (a, b) {
      var va = a[sortCol], vb = b[sortCol];
      if (va == null && vb == null) return 0;
      if (va == null) return 1;
      if (vb == null) return -1;
      if (typeof va === 'string') va = va.toLowerCase();
      if (typeof vb === 'string') vb = vb.toLowerCase();
      if (va < vb) return sortAsc ? -1 : 1;
      if (va > vb) return sortAsc ? 1 : -1;
      return 0;
    });

    var app = document.getElementById('app');
    app.innerHTML = '';

    var bar = el('div', 'filter-bar');
    var viewToggle = buildViewToggle();
    bar.appendChild(viewToggle);

    var input = el('input', 'filter-input');
    input.type = 'text';
    input.placeholder = 'filter traces…  (/ to focus)';
    input.value = filterText;
    input.addEventListener('input', function (e) {
      filterText = e.target.value;
      selectedTraceId = null;
      renderList();
    });
    bar.appendChild(input);

    var compareBtnLabel = comparing
      ? 'cancel compare (' + compareSelection.length + '/2)'
      : 'compare';
    var compareBtn = el('button', 'btn compare-btn' + (comparing ? ' compare-active' : ''), compareBtnLabel);
    compareBtn.title = 'Select two traces to compare side-by-side';
    compareBtn.addEventListener('click', function () {
      comparing = !comparing;
      compareSelection = [];
      renderList();
    });
    bar.appendChild(compareBtn);

    var refreshBtn = el('button', 'btn refresh-btn', '↻ refresh');
    refreshBtn.title = 'Fetch latest traces from Langfuse (R)';
    refreshBtn.addEventListener('click', doRefresh);
    bar.appendChild(refreshBtn);

    if (listLoadedAt) {
      var stale = el('span', 'staleness', 'loaded ' + fmtAgo(listLoadedAt));
      stale.title = new Date(listLoadedAt).toLocaleString();
      bar.appendChild(stale);
    }

    app.appendChild(bar);

    if (sorted.length === 0) {
      var empty = el('div', 'empty-state');
      if (filterText) {
        empty.textContent = 'no traces match "' + filterText + '"';
        var clearLink = el('span', 'empty-action', ' - clear filter');
        clearLink.addEventListener('click', function () {
          filterText = '';
          selectedTraceId = null;
          renderList();
        });
        empty.appendChild(clearLink);
      } else {
        empty.textContent = 'no traces yet';
        var refreshLink = el('span', 'empty-action', ' - press R to refresh');
        empty.appendChild(refreshLink);
      }
      app.appendChild(empty);
      return;
    }

    var table = el('table', 'trace-table');
    var thead = el('thead');
    var headRow = el('tr');

    var columns = [
      { key: 'name', label: 'Name' },
      { key: 'id', label: 'ID' },
      { key: 'timestamp', label: 'Time' },
      { key: 'latency', label: 'Duration' },
      { key: 'totalCost', label: 'Cost' },
      { key: 'tags', label: 'Tags' },
    ];

    columns.forEach(function (col) {
      var th = el('th');
      th.textContent = col.label;
      if (sortCol === col.key) {
        th.classList.add('sorted');
        var arrow = el('span', 'arrow');
        arrow.textContent = sortAsc ? '▲' : '▼';
        th.appendChild(arrow);
      }
      th.addEventListener('click', function () {
        if (sortCol === col.key) { sortAsc = !sortAsc; }
        else { sortCol = col.key; sortAsc = true; }
        renderList();
      });
      headRow.appendChild(th);
    });

    thead.appendChild(headRow);
    table.appendChild(thead);

    var tbody = el('tbody');
    sorted.forEach(function (t) {
      var row = el('tr');
      row.dataset.traceId = t.id;
      if (t.id === selectedTraceId) row.classList.add('selected');
      if (compareSelection.indexOf(t.id) !== -1) row.classList.add('compare-selected');

      row.addEventListener('click', function () {
        if (comparing && compareSelection.indexOf(t.id) === -1) {
          compareSelection.push(t.id);
          if (compareSelection.length === 2) {
            comparing = false;
            location.hash = '#/compare/' + encodeURIComponent(compareSelection[0]) + '/' + encodeURIComponent(compareSelection[1]);
            compareSelection = [];
          } else {
            renderList();
          }
          return;
        }
        if (!comparing) {
          location.hash = '#/traces/' + encodeURIComponent(t.id);
        }
      });

      row.appendChild(td(el('span', 'trace-name', t.name || '(unnamed)')));
      row.appendChild(td(el('span', 'trace-id', t.id.slice(0, 12))));
      row.appendChild(td(document.createTextNode(fmtTime(t.timestamp))));

      var dur = el('span', 'duration');
      dur.textContent = t.latency != null ? fmtDuration(t.latency * 1000) : '-';
      row.appendChild(td(dur));

      var cost = el('span', 'cost');
      cost.textContent = t.totalCost != null ? fmtCost(t.totalCost) : '-';
      row.appendChild(td(cost));

      var tagsCell = el('td');
      (t.tags || []).forEach(function (tag) {
        tagsCell.appendChild(el('span', 'tag', tag));
      });
      row.appendChild(tagsCell);

      tbody.appendChild(row);
    });

    table.appendChild(tbody);
    app.appendChild(table);
  }

  function findSelectedRow() {
    if (!selectedTraceId) return null;
    return document.querySelector('.trace-table tbody tr[data-trace-id="' + CSS.escape(selectedTraceId) + '"]');
  }

  function selectTraceRow(delta) {
    var rows = Array.from(document.querySelectorAll('.trace-table tbody tr'));
    if (rows.length === 0) return;

    var curIdx = rows.findIndex(function (r) { return r.dataset.traceId === selectedTraceId; });

    var prev = curIdx >= 0 ? rows[curIdx] : null;
    if (prev) prev.classList.remove('selected');

    if (curIdx === -1) {
      curIdx = delta > 0 ? 0 : rows.length - 1;
    } else {
      curIdx = Math.max(0, Math.min(rows.length - 1, curIdx + delta));
    }

    var cur = rows[curIdx];
    cur.classList.add('selected');
    cur.scrollIntoView({ block: 'nearest' });
    selectedTraceId = cur.dataset.traceId;
  }

  function enterSelectedTrace() {
    var row = findSelectedRow();
    if (row) row.click();
  }

  async function doRefresh() {
    var btn = document.querySelector('.refresh-btn');
    if (btn) {
      btn.disabled = true;
      btn.textContent = '↻ refreshing…';
    }

    try {
      var resp = await fetch('/api/refresh', { method: 'POST' });
      if (!resp.ok) {
        var body = await resp.json().catch(function () { return {}; });
        throw new Error(body.error || resp.statusText);
      }
      selectedTraceId = null;
      await showList();
    } catch (e) {
      var app = document.getElementById('app');
      app.innerHTML = '<div class="error-msg">refresh failed: ' + esc(e.message) + '</div>';
    } finally {
      if (btn) {
        btn.disabled = false;
        btn.textContent = '↻ refresh';
      }
    }
  }

  async function showTrace(traceId) {
    var app = document.getElementById('app');
    app.innerHTML = '<div class="loading">loading trace…</div>';

    try {
      var resp = await fetch('/api/traces/' + encodeURIComponent(traceId));
      if (!resp.ok) {
        var body = await resp.json().catch(function () { return {}; });
        throw new Error(body.error || resp.statusText);
      }
      var data = await resp.json();
      window.renderTraceDetail(app, data);
    } catch (e) {
      app.innerHTML = '<div class="error-msg">failed to load trace: ' + esc(e.message) + '</div>';
    }
  }

  async function showCompare(idA, idB) {
    var app = document.getElementById('app');
    app.innerHTML = '';

    var hdr = el('div', 'detail-header');
    var back = el('a', 'back-link', '← traces');
    back.href = '#/';
    hdr.appendChild(back);
    hdr.appendChild(el('h2', null, 'compare traces'));
    app.appendChild(hdr);

    var grid = el('div', 'compare-grid');

    var colA = el('div', 'compare-col');
    colA.innerHTML = '<div class="loading">loading…</div>';
    grid.appendChild(colA);

    var colB = el('div', 'compare-col');
    colB.innerHTML = '<div class="loading">loading…</div>';
    grid.appendChild(colB);

    app.appendChild(grid);

    var dataA, dataB;
    try {
      var [respA, respB] = await Promise.all([
        fetch('/api/traces/' + encodeURIComponent(idA)),
        fetch('/api/traces/' + encodeURIComponent(idB)),
      ]);
      if (!respA.ok || !respB.ok) throw new Error('failed to fetch traces');
      dataA = await respA.json();
      dataB = await respB.json();
    } catch (e) {
      app.innerHTML = '<div class="error-msg">' + esc(e.message) + '</div>';
      return;
    }

    colA.innerHTML = '';
    colB.innerHTML = '';

    renderCompareCol(colA, dataA);
    renderCompareCol(colB, dataB);

    var diffBar = buildStatsDiff(dataA.stats, dataB.stats);
    grid.insertBefore(diffBar, grid.firstChild);
  }

  function renderCompareCol(col, data) {
    col.appendChild(el('h3', 'compare-title', data.trace.name || data.trace.id));
    col.appendChild(renderMiniStats(data.stats));

    var treeContainer = el('div', 'tree-container');
    var totalMs = data.stats.totalDurationMs;
    var traceStart = findTreeStart(data.tree);
    data.tree.forEach(function (node) {
      treeContainer.appendChild(window.renderSingleNode(node, 0, totalMs, traceStart));
    });
    col.appendChild(treeContainer);
  }

  function renderMiniStats(stats) {
    var bar = el('div', 'stats-bar compact');
    miniStat(bar, 'Duration', stats.totalDurationMs != null ? fmtDuration(stats.totalDurationMs) : '-');
    miniStat(bar, 'Tokens', (stats.totalTokensIn + stats.totalTokensOut).toLocaleString());
    miniStat(bar, 'Cost', fmtCost(stats.totalCost));
    if (stats.errorCount > 0) miniStat(bar, 'Errors', stats.errorCount, 'stat-errors');
    return bar;
  }

  function miniStat(bar, label, value, cls) {
    var item = el('div', 'stat-item' + (cls ? ' ' + cls : ''));
    item.appendChild(el('span', 'stat-label', label));
    item.appendChild(el('span', 'stat-value', String(value)));
    bar.appendChild(item);
  }

  function buildStatsDiff(a, b) {
    var bar = el('div', 'compare-diff');
    diffItem(bar, 'Duration', a.totalDurationMs, b.totalDurationMs, fmtDuration);
    diffItem(bar, 'Tokens in', a.totalTokensIn, b.totalTokensIn);
    diffItem(bar, 'Tokens out', a.totalTokensOut, b.totalTokensOut);
    diffItem(bar, 'Cost', a.totalCost, b.totalCost, fmtCost);
    diffItem(bar, 'Errors', a.errorCount, b.errorCount);
    return bar;
  }

  function diffItem(bar, label, valA, valB, fmt) {
    var item = el('div', 'diff-item');
    item.appendChild(el('span', 'diff-label', label));
    var a = fmt ? fmt(valA || 0) : String(valA || 0);
    var b = fmt ? fmt(valB || 0) : String(valB || 0);
    var diff = (valB || 0) - (valA || 0);
    var diffStr = diff === 0 ? '=' : (diff > 0 ? '+' : '') + (fmt ? fmt(Math.abs(diff)) : String(diff));
    var diffCls = diff === 0 ? 'diff-neutral' : (diff > 0 ? 'diff-up' : 'diff-down');
    item.appendChild(el('span', 'diff-val', a + ' → ' + b));
    item.appendChild(el('span', 'diff-delta ' + diffCls, diffStr));
    bar.appendChild(item);
  }

  function findTreeStart(tree) {
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

  // --- help overlay ---

  function toggleHelp() {
    var existing = document.querySelector('.help-overlay');
    if (existing) { existing.remove(); return; }

    var overlay = el('div', 'help-overlay');
    overlay.addEventListener('click', function (e) {
      if (e.target === overlay) overlay.remove();
    });

    var panel = el('div', 'help-panel');
    panel.appendChild(el('h2', null, 'keyboard shortcuts'));

    var sections = [
      { title: 'trace list', keys: [
        ['j / k', 'move down / up'],
        ['Enter', 'open selected trace'],
        ['/', 'focus filter input'],
        ['R', 'refresh traces'],
      ]},
      { title: 'span tree', keys: [
        ['j / k', 'move down / up'],
        ['Enter', 'expand / collapse node'],
        ['o', 'toggle input/output panel'],
        ['Esc', 'back to trace list'],
      ]},
      { title: 'views', keys: [
        ['sessions', 'group traces by session_id'],
        ['compare', 'select 2 traces to diff side-by-side'],
        ['export JSON', 'download full trace data'],
      ]},
      { title: 'global', keys: [
        ['?', 'toggle this help'],
        ['/', 'focus filter input'],
        ['Esc', 'close overlay / go back'],
      ]},
    ];

    sections.forEach(function (s) {
      panel.appendChild(el('h3', 'help-section-title', s.title));
      var table = el('table', 'help-table');
      s.keys.forEach(function (pair) {
        var row = el('tr');
        var keyCell = el('td', 'help-key');
        keyCell.appendChild(el('kbd', 'kbd', pair[0]));
        row.appendChild(keyCell);
        row.appendChild(el('td', 'help-desc', pair[1]));
        table.appendChild(row);
      });
      panel.appendChild(table);
    });

    var closeBtn = el('button', 'btn help-close', 'close');
    closeBtn.addEventListener('click', function () { overlay.remove(); });
    panel.appendChild(closeBtn);

    overlay.appendChild(panel);
    document.body.appendChild(overlay);
  }

  // --- auto-refresh ---

  var autoRefreshTimer = null;

  function startAutoRefresh(intervalMs) {
    stopAutoRefresh();
    autoRefreshTimer = setInterval(function () {
      var onList = !location.hash || location.hash === '#/' || location.hash === '#';
      if (onList) doRefresh();
    }, intervalMs);
  }

  function stopAutoRefresh() {
    if (autoRefreshTimer) {
      clearInterval(autoRefreshTimer);
      autoRefreshTimer = null;
    }
  }

  window.startAutoRefresh = startAutoRefresh;
  window.stopAutoRefresh = stopAutoRefresh;

  // --- keyboard shortcuts ---

  document.addEventListener('keydown', function (e) {
    if (e.target.tagName === 'INPUT') {
      if (e.key === 'Escape') { e.target.blur(); e.preventDefault(); }
      return;
    }

    if (e.key === '?') {
      e.preventDefault();
      toggleHelp();
      return;
    }

    var helpOpen = document.querySelector('.help-overlay');
    if (helpOpen) {
      if (e.key === 'Escape') { helpOpen.remove(); e.preventDefault(); }
      return;
    }

    var onList = !location.hash || location.hash === '#/' || location.hash === '#';
    var onDetail = location.hash.startsWith('#/traces/');

    if (e.key === '/') {
      e.preventDefault();
      var input = document.querySelector('.filter-input');
      if (input) input.focus();
      return;
    }

    if (e.key === 'Escape' && onDetail) {
      location.hash = '#/';
      return;
    }

    if (onList) {
      if (e.key === 'j') { e.preventDefault(); selectTraceRow(1); }
      if (e.key === 'k') { e.preventDefault(); selectTraceRow(-1); }
      if (e.key === 'Enter' && selectedTraceId) { e.preventDefault(); enterSelectedTrace(); }
      if (e.key === 'r' || e.key === 'R') { e.preventDefault(); doRefresh(); }
    }

    if (onDetail && window.handleTreeKeydown) {
      window.handleTreeKeydown(e);
    }
  });

  // --- helpers ---

  function el(tag, cls, text) {
    var node = document.createElement(tag);
    if (cls) node.className = cls;
    if (text) node.textContent = text;
    return node;
  }

  function td(child) {
    var cell = document.createElement('td');
    cell.appendChild(child);
    return cell;
  }

  function esc(s) {
    var d = document.createElement('div');
    d.textContent = s;
    return d.innerHTML;
  }

  function fmtTime(iso) {
    if (!iso) return '-';
    var d = new Date(iso);
    return d.toLocaleString(undefined, {
      month: 'short', day: 'numeric',
      hour: '2-digit', minute: '2-digit', second: '2-digit',
    });
  }

  function fmtAgo(ts) {
    var s = Math.floor((Date.now() - ts) / 1000);
    if (s < 5) return 'just now';
    if (s < 60) return s + 's ago';
    if (s < 3600) return Math.floor(s / 60) + 'm ago';
    return Math.floor(s / 3600) + 'h ago';
  }

  window.fmtDuration = function (ms) {
    if (ms < 1) return '<1ms';
    if (ms < 1000) return Math.round(ms) + 'ms';
    if (ms < 10000) return (ms / 1000).toFixed(2) + 's';
    if (ms < 60000) return (ms / 1000).toFixed(1) + 's';
    return (ms / 60000).toFixed(1) + 'm';
  };

  window.fmtCost = function (cost) {
    if (cost == null) return '-';
    if (cost < 0.001) return '<$0.001';
    if (cost < 1) return '$' + cost.toFixed(4);
    return '$' + cost.toFixed(2);
  };

  window.el = el;
})();
