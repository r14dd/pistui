(function () {
  'use strict';

  var traces = [];
  var sortCol = 'timestamp';
  var sortAsc = false;
  var filterText = '';
  var selectedIdx = -1;

  window.addEventListener('hashchange', route);
  document.addEventListener('DOMContentLoaded', route);

  function route() {
    var hash = location.hash || '#/';
    var match = hash.match(/^#\/traces\/(.+)$/);
    if (match) {
      selectedIdx = -1;
      showTrace(decodeURIComponent(match[1]));
    } else {
      showList();
    }
  }

  async function showList() {
    var app = document.getElementById('app');

    try {
      var resp = await fetch('/api/traces');
      if (!resp.ok) {
        var body = await resp.json().catch(function () { return {}; });
        throw new Error(body.error || resp.statusText);
      }
      traces = await resp.json();
      renderList();
    } catch (e) {
      app.innerHTML = '<div class="error-msg">failed to load traces: ' + esc(e.message) + '</div>';
    }
  }

  function renderList() {
    var filtered = traces.filter(function (t) {
      if (!filterText) return true;
      var q = filterText.toLowerCase();
      return (t.name || '').toLowerCase().includes(q) ||
             t.id.toLowerCase().includes(q) ||
             (t.tags || []).some(function (tag) { return tag.toLowerCase().includes(q); });
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
    var input = el('input', 'filter-input');
    input.type = 'text';
    input.placeholder = 'filter traces…  (/ to focus)';
    input.value = filterText;
    input.addEventListener('input', function (e) {
      filterText = e.target.value;
      selectedIdx = -1;
      renderList();
    });
    bar.appendChild(input);

    var refreshBtn = el('button', 'btn refresh-btn', '↻ refresh');
    refreshBtn.title = 'Fetch latest traces from Langfuse (R)';
    refreshBtn.addEventListener('click', doRefresh);
    bar.appendChild(refreshBtn);

    app.appendChild(bar);

    if (sorted.length === 0) {
      app.appendChild(el('div', 'empty-state', filterText ? 'no matching traces' : 'no traces found'));
      return;
    }

    if (selectedIdx >= sorted.length) selectedIdx = sorted.length - 1;

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
    sorted.forEach(function (t, idx) {
      var row = el('tr');
      row.dataset.idx = idx;
      if (idx === selectedIdx) row.classList.add('selected');

      row.addEventListener('click', function () {
        location.hash = '#/traces/' + encodeURIComponent(t.id);
      });

      row.appendChild(td(el('span', 'trace-name', t.name || '(unnamed)')));
      row.appendChild(td(el('span', 'trace-id', t.id.slice(0, 12))));
      row.appendChild(td(document.createTextNode(fmtTime(t.timestamp))));

      var dur = el('span', 'duration');
      dur.textContent = t.latency != null ? fmtDuration(t.latency * 1000) : '—';
      row.appendChild(td(dur));

      var cost = el('span', 'cost');
      cost.textContent = t.totalCost != null ? fmtCost(t.totalCost) : '—';
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

  function selectTraceRow(delta) {
    var rows = document.querySelectorAll('.trace-table tbody tr');
    if (rows.length === 0) return;

    var prev = rows[selectedIdx];
    if (prev) prev.classList.remove('selected');

    if (selectedIdx === -1) {
      selectedIdx = delta > 0 ? 0 : rows.length - 1;
    } else {
      selectedIdx += delta;
    }

    if (selectedIdx < 0) selectedIdx = 0;
    if (selectedIdx >= rows.length) selectedIdx = rows.length - 1;

    var cur = rows[selectedIdx];
    cur.classList.add('selected');
    cur.scrollIntoView({ block: 'nearest' });
  }

  function enterSelectedTrace() {
    var rows = document.querySelectorAll('.trace-table tbody tr');
    if (selectedIdx >= 0 && selectedIdx < rows.length) {
      rows[selectedIdx].click();
    }
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
      selectedIdx = -1;
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

  // --- keyboard shortcuts ---

  document.addEventListener('keydown', function (e) {
    if (e.target.tagName === 'INPUT') {
      if (e.key === 'Escape') { e.target.blur(); e.preventDefault(); }
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
      if (e.key === 'Enter' && selectedIdx >= 0) { e.preventDefault(); enterSelectedTrace(); }
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
    if (!iso) return '—';
    var d = new Date(iso);
    return d.toLocaleString(undefined, {
      month: 'short', day: 'numeric',
      hour: '2-digit', minute: '2-digit', second: '2-digit',
    });
  }

  window.fmtDuration = function (ms) {
    if (ms < 1) return '<1ms';
    if (ms < 1000) return Math.round(ms) + 'ms';
    if (ms < 10000) return (ms / 1000).toFixed(2) + 's';
    if (ms < 60000) return (ms / 1000).toFixed(1) + 's';
    return (ms / 60000).toFixed(1) + 'm';
  };

  window.fmtCost = function (cost) {
    if (cost == null) return '—';
    if (cost < 0.001) return '<$0.001';
    if (cost < 1) return '$' + cost.toFixed(4);
    return '$' + cost.toFixed(2);
  };

  window.el = el;
})();
