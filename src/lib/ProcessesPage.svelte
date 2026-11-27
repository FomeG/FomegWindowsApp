<script>
  // Trang Tiến trình kiểu Task Manager: nhóm / danh sách / cây, tô nhiệt ô số, menu chuột phải,
  // kết thúc / khởi động lại / độ ưu tiên, icon app, ngăn chi tiết.
  // Hiệu năng: Map pid -> P trong procs.svelte.js (không reactive) + `procs.version`. Danh sách hàng dựng
  // lại một lần mỗi tick (procview.js); bảng ảo hoá chỉ render các hàng đang thấy.
  import { onMount, tick, untrack } from 'svelte';
  import { SvelteSet } from 'svelte/reactivity';
  import { procs } from './procs.svelte.js';
  import { sys } from './sys.svelte.js';
  import { store } from './store.svelte.js';
  import { ai } from './ai.svelte.js';
  import * as api from './api.js';
  import { toasts, summarize } from './toasts.svelte.js';
  import { buildRows, heat } from './procview.js';
  import { loadPref, savePref } from './prefs.svelte.js';
  import { fmtPct, fmtBytes, fmtRate, fmtInt, fmtDuration, copyText, errText } from './format.js';
  import Glyph from './Glyph.svelte';
  import SearchBox from './SearchBox.svelte';
  import ContextMenu from './ContextMenu.svelte';
  import ProcInspector from './ProcInspector.svelte';
  import SplitView from './SplitView.svelte';
  import { InspState } from './inspector.svelte.js';
  import { t, labels, i18n } from './i18n.svelte.js';

  const ROW_H = 26;
  const HEAD_H = 46;
  const OVERSCAN = 8;

  // Cột cố định; nhãn lấy theo ngôn ngữ qua `COL_LABELS` (getter).
  const COLS = [
    { key: 'name', width: 280, min: 140 },
    { key: 'pid', width: 70, min: 50, align: 'r' },
    { key: 'status', width: 120, min: 70, def: false },
    { key: 'cpu', width: 74, min: 56, align: 'r', heat: true },
    { key: 'mem', width: 100, min: 70, align: 'r', heat: true },
    { key: 'disk', width: 92, min: 60, align: 'r', heat: true },
    { key: 'gpu', width: 70, min: 50, align: 'r', heat: true },
    { key: 'thr', width: 70, min: 50, align: 'r', def: false },
    { key: 'hnd', width: 78, min: 56, align: 'r', def: false },
    { key: 'uptime', width: 120, min: 80, def: false },
    { key: 'user', width: 120, min: 70, def: false },
    { key: 'publisher', width: 180, min: 80 },
    { key: 'cmd', width: 380, min: 120, def: false },
  ];
  const COL_LABELS = labels({
    name: 'procs.col.name',
    pid: 'procs.col.pid',
    status: 'procs.col.status',
    cpu: 'procs.col.cpu',
    mem: 'procs.col.mem',
    disk: 'procs.col.disk',
    gpu: 'procs.col.gpu',
    thr: 'procs.col.thr',
    hnd: 'procs.col.hnd',
    uptime: 'procs.col.uptime',
    user: 'procs.col.user',
    publisher: 'procs.col.publisher',
    cmd: 'procs.col.cmd',
  });
  // Ngăn chi tiết: trạng thái + bố cục dùng chung (inspector.svelte.js / SplitView.svelte).
  const insp = new InspState('proc');
  const NUMERIC = new Set(['pid', 'cpu', 'mem', 'disk', 'gpu', 'thr', 'hnd', 'uptime']);
  const CHIPS = $derived([
    ['all', t('procs.chip.all')],
    ['cpu', t('procs.chip.cpu')],
    ['mem', t('procs.chip.mem')],
    ['recent', t('procs.chip.recent')],
  ]);
  const MODES = $derived([
    ['group', t('procs.mode.group'), 'group'],
    ['list', t('procs.mode.list'), 'bullets'],
    ['tree', t('procs.mode.tree'), 'tree'],
  ]);
  const PRIO_IDS = ['idle', 'below', 'normal', 'above', 'high', 'realtime'];
  const PRIO_LABELS = labels({
    idle: 'procs.prio.idle',
    below: 'procs.prio.below',
    normal: 'procs.prio.normal',
    above: 'procs.prio.above',
    high: 'procs.prio.high',
    realtime: 'procs.prio.realtime',
  });

  onMount(() => {
    procs.watch();
    return () => procs.unwatch();
  });

  // ---------- tuỳ chọn được nhớ ----------
  const defCols = Object.fromEntries(COLS.map((c) => [c.key, c.def !== false]));
  let colOn = $state({ ...defCols, ...loadPref('proc:cols', {}) });
  let widths = $state(loadPref('proc:widths', {}));
  let mode = $state(loadPref('proc:mode', 'group'));
  let sort = $state(loadPref('proc:sort', { key: 'name', desc: false }));
  let hideSys = $state(loadPref('proc:hideSys', false));
  // Mở từ trang Cổng / ngăn chi tiết dịch vụ: lọc theo PID và chọn sẵn tiến trình đó.
  let revealPid = procs.revealPid;
  procs.revealPid = 0;
  if (revealPid) hideSys = false;
  let q = $state(revealPid ? String(revealPid) : '');
  let chip = $state('all');

  const expanded = new SvelteSet(); // key nhóm đang mở (chế độ Nhóm)
  const collapsed = new SvelteSet(); // pid đang thu gọn (chế độ Cây)
  const sel = new SvelteSet(); // key hàng được chọn
  let focusKey = $state(null);
  let anchorKey = null;
  let menu = $state(null); // { x, y, targets }
  let colMenu = $state(false);
  // hàng đang xem trong inspector: đúng 1 hàng được chọn (tạo lại mỗi tick để có số liệu mới)
  const inspTarget = $derived.by(() => {
    procs.version;
    rows;
    if ((!insp.shown && !insp.overlay) || sel.size !== 1) return null;
    return selectedRows()[0] ?? null;
  });

  const cols = $derived(COLS.filter((c) => c.key === 'name' || colOn[c.key]));
  const cw = (c) => Math.max(c.min, widths[c.key] ?? c.width);
  const template = $derived(cols.map((c, i) => (i === cols.length - 1 ? `minmax(${cw(c)}px, 1fr)` : `${cw(c)}px`)).join(' '));
  const minW = $derived(cols.reduce((a, c) => a + cw(c), 0));
  const memTotal = $derived(sys.info?.mem_total || untrack(() => sys.m?.mem.total) || 0);

  // ---------- danh sách hàng ----------
  const view = $derived.by(() => {
    procs.version;
    return buildRows(procs.map, { mode, q, chip, hideSys, sort, expanded, collapsed, now: procs.t || Date.now(), windows: procs.windows });
  });
  const rows = $derived(view.rows);

  let scroller = $state();
  let scrollTop = $state(0);
  let viewH = $state(600);
  const start = $derived(Math.max(0, Math.floor(scrollTop / ROW_H) - OVERSCAN));
  const end = $derived(Math.min(rows.length, Math.ceil((scrollTop + viewH) / ROW_H) + OVERSCAN));
  const vis = $derived(rows.slice(start, end));

  // icon cho các hàng đang thấy (gộp + debounce trong store)
  $effect(() => {
    const paths = [];
    for (const r of vis) if (r.exe && !procs.icons.has(r.exe)) paths.push(r.exe);
    if (paths.length) procs.wantIcons(paths);
  });

  $effect(() => {
    procs.version;
    if (revealPid && procs.map.has(revealPid)) {
      untrack(() => selectOnly('p' + revealPid));
      revealPid = 0;
    }
  });

  // bỏ chọn các tiến trình đã thoát
  $effect(() => {
    const rs = rows;
    untrack(() => {
      if (!sel.size && !focusKey) return;
      const keys = new Set();
      for (const r of rs) keys.add(r.key);
      const alive = (k) => (k[0] === 'p' ? procs.map.has(Number(k.slice(1))) : keys.has(k));
      for (const k of [...sel]) if (!alive(k)) sel.delete(k);
      if (focusKey && !alive(focusKey)) focusKey = null;
    });
  });

  function inspMore(e) {
    const b = e.currentTarget.getBoundingClientRect();
    menu = { x: Math.max(8, b.right - 230), y: b.bottom + 4, targets: selectedRows() };
  }

  // tổng ở đầu cột
  const totals = $derived.by(() => {
    const m = sys.m;
    procs.version;
    return {
      cpu: m ? fmtPct(m.cpu.total) : '',
      mem: m ? fmtPct((m.mem.used / m.mem.total) * 100) : '',
      disk: procs.loaded ? mbs(procs.totalDisk) : '',
      gpu: m?.gpus?.length ? fmtPct(Math.max(...m.gpus.map((g) => g.util))) : '',
    };
  });
  const totalTip = labels({ cpu: 'procs.total_tip.cpu', mem: 'procs.total_tip.mem', disk: 'procs.total_tip.disk', gpu: 'procs.total_tip.gpu' });

  // ---------- định dạng ô ----------
  const nf1 = $derived(new Intl.NumberFormat(i18n.intl, { maximumFractionDigits: 1, minimumFractionDigits: 1 }));
  function mbs(b) { return nf1.format((b || 0) / 1048576) + ' MB/s'; }
  function memMB(b) { return nf1.format((b || 0) / 1048576) + ' MB'; }
  // nhãn trạng thái: tính một lần mỗi lần đổi ngôn ngữ, không gọi t() cho từng hàng mỗi tick
  const ST = $derived({
    app: t('procs.status.app'),
    background: t('procs.status.background'),
    windows: t('procs.status.windows'),
    win: t('procs.status.has_window'),
    admin: t('procs.status.admin'),
  });
  function cell(r, k) {
    switch (k) {
      case 'pid': return r.type === 'group' ? '' : r.pid;
      case 'status': {
        const s = r.type === 'group' ? '' : (procs.windows.has(r.pid) ? ST.win : ST[r.kind] || '');
        return r.p?.elevated ? (s ? s + ' · ' + ST.admin : ST.admin) : s;
      }
      case 'cpu': return fmtPct(r.cpu, r.cpu > 0 && r.cpu < 10 ? 1 : 0);
      case 'mem': return memMB(r.mem);
      case 'disk': return mbs(r.disk);
      case 'gpu': return fmtPct(r.gpu, r.gpu > 0 && r.gpu < 10 ? 1 : 0);
      case 'thr': return fmtInt(r.thr);
      case 'hnd': return fmtInt(r.hnd);
      case 'uptime': return r.start_ms ? fmtDuration((procs.t - r.start_ms) / 1000) : '';
      case 'user': return r.user || '';
      case 'publisher': return r.publisher || '';
      case 'cmd': return r.cmd || '';
    }
    return '';
  }

  // ---------- sắp xếp / cột ----------
  function onsort(key) {
    sort = sort.key === key ? { key, desc: !sort.desc } : { key, desc: NUMERIC.has(key) && key !== 'pid' };
    savePref('proc:sort', sort);
  }
  function setMode(m) {
    mode = m;
    savePref('proc:mode', m);
    scrollTop = 0;
    if (scroller) scroller.scrollTop = 0;
  }
  function toggleCol(k) {
    colOn = { ...colOn, [k]: !colOn[k] };
    savePref('proc:cols', colOn);
  }
  function resetCols() {
    colOn = { ...defCols };
    widths = {};
    savePref('proc:cols', colOn);
    savePref('proc:widths', widths);
  }
  function startResize(e, c) {
    e.preventDefault();
    e.stopPropagation();
    const x0 = e.clientX;
    const w0 = cw(c);
    const tgt = e.currentTarget;
    tgt.setPointerCapture(e.pointerId);
    const move = (ev) => { widths = { ...widths, [c.key]: Math.max(c.min, Math.round(w0 + ev.clientX - x0)) }; };
    const up = () => {
      tgt.removeEventListener('pointermove', move);
      tgt.removeEventListener('pointerup', up);
      tgt.removeEventListener('pointercancel', up);
      savePref('proc:widths', widths);
    };
    tgt.addEventListener('pointermove', move);
    tgt.addEventListener('pointerup', up);
    tgt.addEventListener('pointercancel', up);
  }
  function resetWidth(c) {
    const { [c.key]: _, ...rest } = widths;
    widths = rest;
    savePref('proc:widths', widths);
  }

  // ---------- chọn / bàn phím ----------
  const selectable = (r) => r && r.type !== 'section';
  function indexOfKey(k) {
    for (let i = 0; i < rows.length; i++) if (rows[i].key === k) return i;
    return -1;
  }
  function selectOnly(k) {
    sel.clear();
    sel.add(k);
    focusKey = k;
    anchorKey = k;
  }
  function rowClick(e, r) {
    scroller?.focus({ preventScroll: true });
    if (e.ctrlKey || e.metaKey) {
      sel.has(r.key) ? sel.delete(r.key) : sel.add(r.key);
      focusKey = r.key;
      anchorKey = r.key;
    } else if (e.shiftKey && anchorKey) {
      const a = indexOfKey(anchorKey), b = indexOfKey(r.key);
      if (a < 0 || b < 0) return selectOnly(r.key);
      sel.clear();
      for (let i = Math.min(a, b); i <= Math.max(a, b); i++) if (selectable(rows[i])) sel.add(rows[i].key);
      focusKey = r.key;
    } else selectOnly(r.key);
  }
  function toggleExpand(r) {
    if (!r.expandable) return;
    if (r.type === 'group') expanded.has(r.key) ? expanded.delete(r.key) : expanded.add(r.key);
    else collapsed.has(r.pid) ? collapsed.delete(r.pid) : collapsed.add(r.pid);
  }
  function ensureVisible(i) {
    if (!scroller) return;
    const top = i * ROW_H;
    const avail = scroller.clientHeight - HEAD_H;
    if (top < scroller.scrollTop) scroller.scrollTop = top;
    else if (top + ROW_H > scroller.scrollTop + avail) scroller.scrollTop = top + ROW_H - avail;
  }
  function moveFocus(delta) {
    let i = focusKey ? indexOfKey(focusKey) : -1;
    const n = rows.length;
    if (!n) return;
    if (i < 0) i = delta > 0 ? -1 : n;
    let j = Math.max(0, Math.min(n - 1, i + delta));
    const step = delta > 0 ? 1 : -1;
    while (j >= 0 && j < n && !selectable(rows[j])) j += step;
    if (j < 0 || j >= n) return;
    selectOnly(rows[j].key);
    ensureVisible(j);
  }
  function focusedRow() {
    const i = focusKey ? indexOfKey(focusKey) : -1;
    return i >= 0 ? rows[i] : null;
  }
  function tableKey(e) {
    if (menu) return;
    const pageRows = Math.max(1, Math.floor((viewH - HEAD_H) / ROW_H) - 1);
    switch (e.key) {
      case 'ArrowDown': e.preventDefault(); moveFocus(1); break;
      case 'ArrowUp': e.preventDefault(); moveFocus(-1); break;
      case 'PageDown': e.preventDefault(); moveFocus(pageRows); break;
      case 'PageUp': e.preventDefault(); moveFocus(-pageRows); break;
      case 'Home': e.preventDefault(); focusKey = null; moveFocus(1); break;
      case 'End': e.preventDefault(); focusKey = null; moveFocus(-1); break;
      case 'ArrowRight': { const r = focusedRow(); if (r?.expandable && !r.expanded) { e.preventDefault(); toggleExpand(r); } break; }
      case 'ArrowLeft': {
        const r = focusedRow();
        if (!r) break;
        e.preventDefault();
        if (r.expandable && r.expanded) toggleExpand(r);
        else if (r.parentKey) { selectOnly(r.parentKey); ensureVisible(indexOfKey(r.parentKey)); }
        break;
      }
      case 'Enter': { const r = focusedRow(); if (r) { e.preventDefault(); openDetails(r); } break; }
      case 'Escape': if (insp.overlay) { e.preventDefault(); insp.closeOverlay(); } else if (sel.size) { e.preventDefault(); sel.clear(); focusKey = null; anchorKey = null; } break;
      case 'Delete': { const tg = selectedRows(); if (tg.length) { e.preventDefault(); killRows(tg, false); } break; }
      case 'ContextMenu': case 'F10':
        if (e.key === 'F10' && !e.shiftKey) break;
        e.preventDefault();
        openMenuAtFocus();
        break;
      case 'a':
        if (e.ctrlKey || e.metaKey) { e.preventDefault(); sel.clear(); for (const r of rows) if (selectable(r)) sel.add(r.key); }
        break;
    }
  }

  function selectedRows() {
    const out = [];
    const byKey = new Map(rows.map((r) => [r.key, r]));
    for (const k of sel) {
      const r = byKey.get(k);
      if (r) out.push(r);
      else if (k[0] === 'p') {
        const p = procs.map.get(Number(k.slice(1)));
        if (p) out.push({ key: k, type: 'proc', p, pid: p.pid, label: p.name, exe: p.exe, cmd: p.cmd, cpu: p.cpu, mem: p.mem, count: 1 });
      }
    }
    return out;
  }
  const pidsOf = (r) => (r.type === 'group' ? r.pids : [r.pid]);
  const nameOf = (r) => (r.type === 'group' ? r.p.name : r.p?.name || r.label);

  // ---------- menu chuột phải ----------
  function rowMenu(e, r) {
    e.preventDefault();
    if (!sel.has(r.key)) selectOnly(r.key);
    else focusKey = r.key;
    menu = { x: e.clientX, y: e.clientY, targets: selectedRows() };
  }
  function openMenuAtFocus() {
    const i = focusKey ? indexOfKey(focusKey) : -1;
    if (i < 0) return;
    ensureVisible(i);
    const rect = scroller.getBoundingClientRect();
    menu = { x: rect.left + 60, y: rect.top + HEAD_H + (i * ROW_H - scroller.scrollTop) + ROW_H, targets: selectedRows() };
  }
  function closeMenu() {
    menu = null;
    scroller?.focus({ preventScroll: true });
  }
  const menuItems = $derived.by(() => {
    if (!menu) return [];
    const tg = menu.targets;
    const one = tg.length === 1 ? tg[0] : null;
    const n = tg.reduce((a, r) => a + pidsOf(r).length, 0);
    return [
      { id: 'kill', label: n > 1 ? t('procs.menu.kill_n', { n }) : t('procs.menu.kill'), glyph: 'close', hint: 'Delete', danger: true },
      { id: 'killtree', label: t('procs.menu.killtree'), glyph: 'tree', danger: true },
      { id: 'restart', label: t('procs.menu.restart'), glyph: 'refresh', disabled: !one },
      { sep: true },
      { id: 'details', label: t('procs.menu.details'), glyph: 'info', hint: 'Enter', disabled: !one },
      { id: 'open', label: t('procs.menu.open'), glyph: 'folder', disabled: !one?.exe },
      { id: 'props', label: t('procs.menu.props'), glyph: 'document', disabled: !one?.exe },
      { id: 'prio', label: t('procs.menu.prio'), glyph: 'speed', sub: PRIO_IDS.map((id) => ({ id: 'prio:' + id, label: PRIO_LABELS[id], danger: id === 'realtime' })) },
      { sep: true },
      { id: 'copy', label: t('procs.menu.copy'), glyph: 'copy', sub: [
        { id: 'copy:name', label: t('procs.menu.copy_name') }, { id: 'copy:pid', label: 'PID' },
        { id: 'copy:path', label: t('procs.menu.copy_path'), disabled: !tg.some((r) => r.exe) }, { id: 'copy:cmd', label: t('procs.menu.copy_cmd'), disabled: !tg.some((r) => r.cmd) },
      ] },
      { sep: true },
      { id: 'ai', label: t('procs.menu.ai'), glyph: 'chat', disabled: !one },
    ];
  });
  function pickMenu(id) {
    run(id, menu?.targets || []);
  }

  // ---------- hành động ----------
  function run(id, tg) {
    if (!tg.length) return;
    const one = tg[0];
    if (id === 'kill' || id === 'killtree') return killRows(tg, id === 'killtree');
    if (id === 'restart') return restart(one);
    if (id === 'details') return openDetails(one);
    if (id === 'open') return api.procOpenLocation(one.exe).catch((e) => toasts.error(t('procs.toast.open_fail'), [errText(e)]));
    if (id === 'props') return api.procProperties(one.exe).catch((e) => toasts.error(t('procs.toast.props_fail'), [errText(e)]));
    if (id === 'ai') return askAi(one);
    if (id.startsWith('prio:')) return setPriority(tg, id.slice(5));
    if (id.startsWith('copy:')) return copy(tg, id.slice(5));
  }

  async function killRows(tg, tree) {
    const pids = [...new Set(tg.flatMap(pidsOf))];
    if (!pids.length) return;
    const snapshot = procs.snapshot(pids);
    const label = tg.length === 1 ? nameOf(tg[0]) : t('procs.kill.n_items', { n: tg.length });
    const sysProc = tg.some((r) => (r.p?.kind || r.kind) === 'windows' && !procs.windows.has(r.pid));
    if (!loadPref('proc:noAskKill', false)) {
      const key = pids.length > 1 ? (tree ? 'procs.kill.msg_many_tree' : 'procs.kill.msg_many') : (tree ? 'procs.kill.msg_tree' : 'procs.kill.msg');
      const msg = t(key, { label, n: pids.length }) + (sysProc ? ' ' + t('procs.kill.msg_sys') : '');
      const r = await store.confirm({ title: tree ? t('procs.menu.killtree') : t('procs.menu.kill'), message: msg, okLabel: t('procs.kill.ok'), danger: true, checkLabel: t('procs.kill.no_ask') });
      if (!r.ok) return;
      if (r.checked) savePref('proc:noAskKill', true);
    }
    try {
      const { same, gone } = procs.stillSame(snapshot);
      const res = same.length ? (await api.procKill(same, tree)) || [] : [];
      for (const pid of gone) res.push({ pid, ok: false, error: t('ai.review.kill.not_found') });
      const okPids = res.filter((x) => x.ok).map((x) => x.pid);
      procs.dropLocal(okPids.filter((pid) => procs.map.get(pid) === snapshot.get(pid)));
      const errors = res.filter((x) => !x.ok).map((x) => {
        const name = snapshot.get(x.pid)?.name;
        return `PID ${x.pid}${name ? ' (' + name + ')' : ''}: ${x.error || t('procs.err.generic')}`;
      });
      if (res.length === 1 && !errors.length) toasts.success(t('procs.toast.killed', { name: label }));
      else if (res.length === 1) toasts.error(t('procs.toast.kill_fail', { name: label }), errors);
      else summarize(t('procs.kill.verb'), t('procs.unit'), okPids.length, errors);
    } catch (e) {
      toasts.error(t('procs.toast.kill_fail', { name: label }), [errText(e)]);
    }
  }

  async function restart(r) {
    const name = nameOf(r);
    try {
      const res = await api.procRestart(r.pid);
      if (res?.ok) {
        toasts.success(t('procs.toast.restarted', { name }), res.pid ? [t('procs.toast.new_pid', { pid: res.pid })] : []);
        if (res.pid && r.type === 'proc' && sel.has(r.key)) selectOnly('p' + res.pid);
      } else toasts.error(t('procs.toast.restart_fail', { name }), [res?.error || t('procs.err.no_reply')]);
    } catch (e) {
      toasts.error(t('procs.toast.restart_fail', { name }), [errText(e)]);
    }
  }

  async function setPriority(tg, level) {
    const label = PRIO_LABELS[level] || level;
    if (level === 'realtime') {
      const yes = await store.confirm({
        title: t('procs.prio.rt_title'),
        message: t('procs.prio.rt_msg'),
        okLabel: t('procs.prio.rt_ok'),
        danger: true,
      });
      if (!yes) return;
    }
    const pids = [...new Set(tg.flatMap(pidsOf))];
    const res = await Promise.all(pids.map((pid) => api.procSetPriority(pid, level).then((r) => ({ pid, ...(r || { ok: false }) })).catch((e) => ({ pid, ok: false, error: errText(e) }))));
    const errors = res.filter((r) => !r.ok).map((r) => `PID ${r.pid}: ${r.error || t('procs.err.generic')}`);
    const ok = res.length - errors.length;
    if (res.length === 1) {
      if (ok) toasts.success(t('procs.toast.prio_set', { label }), [nameOf(tg[0])]);
      else toasts.error(t('procs.toast.prio_fail', { label }), errors);
    } else summarize(t('procs.prio.verb', { label }), t('procs.unit'), ok, errors);
  }

  async function copy(tg, what) {
    let text = '';
    if (what === 'name') text = [...new Set(tg.map(nameOf))].join('\n');
    else if (what === 'pid') text = tg.flatMap(pidsOf).join(', ');
    else if (what === 'path') text = [...new Set(tg.map((r) => r.exe).filter(Boolean))].join('\n');
    else if (what === 'cmd') text = tg.flatMap((r) => (r.type === 'group' ? r.members.map((p) => p.cmd) : [r.cmd])).filter(Boolean).join('\n');
    const keys = { name: 'procs.copied.name', pid: 'procs.copied.pid', path: 'procs.copied.path', cmd: 'procs.copied.cmd' };
    if (text && (await copyText(text))) toasts.success(t(keys[what]));
    else toasts.error(t('procs.toast.copy_fail'));
  }

  function askAi(r) {
    const p = r.p || procs.map.get(r.pid);
    if (!p) return;
    const unk = t('procs.ai.unknown');
    const lines = [
      r.type === 'group' ? t('procs.ai.q_group', { name: p.name, n: r.count }) : t('procs.ai.q', { name: p.name }),
      t('procs.ai.desc', { v: p.desc || unk }),
      t('procs.ai.path', { v: p.exe || unk }),
      t('procs.ai.publisher', { v: p.publisher || unk }),
      t('procs.ai.cmd', { v: p.cmd || unk }),
      t('procs.ai.usage', { cpu: fmtPct(r.cpu, 1), ram: fmtBytes(r.mem) }),
    ];
    ai.ask(lines.join('\n'));
  }

  function openDetails(r) {
    if (!r || r.type === 'section') return;
    selectOnly(r.key);
    insp.open();
  }
  function closeInsp(overlay) {
    if (overlay) {
      insp.closeOverlay();
      scroller?.focus({ preventScroll: true });
    } else insp.toggle();
  }

  function togglePause() {
    procs.paused = !procs.paused;
    if (!procs.paused) procs.version++;
  }
  function setChip(c) {
    chip = c;
  }
  function toggleHideSys() {
    hideSys = !hideSys;
    savePref('proc:hideSys', hideSys);
  }
  function onScroll() {
    scrollTop = scroller.scrollTop;
  }
</script>

<SplitView {insp} hasTarget={sel.size === 1} cls="procs-page">
 {#snippet main()}
  <header class="page-head">
    <div>
      <h1>{t('procs.title')}</h1>
      <p class="muted">{#if procs.loaded}{t('procs.summary', { n: fmtInt((procs.version, procs.map.size)), shown: fmtInt(view.shown) })}{:else}{t('procs.loading')}{/if}</p>
    </div>
    <div class="ph-actions">
      {#if insp.fits}
        <button type="button" class="btn icon-only" class:active={insp.on} aria-pressed={insp.on} onclick={() => insp.toggle()} title={insp.on ? t('procs.pane.hide') : t('procs.pane.show')} aria-label={t('procs.pane.label')}>
          <Glyph name={insp.on ? 'paneClose' : 'pane'} size={13} />
        </button>
      {/if}
      <button type="button" class="btn" class:primary={procs.paused} aria-pressed={procs.paused} onclick={togglePause} title={t('procs.pause.tip')}>
        <Glyph name={procs.paused ? 'play' : 'pause'} size={12} /> {procs.paused ? t('procs.pause.resume') : t('procs.pause.pause')}
      </button>
      <button type="button" class="btn danger" disabled={!sel.size} onclick={() => killRows(selectedRows(), false)} title={t('procs.kill.tip')}>
        <Glyph name="close" size={11} /> {t('procs.menu.kill')}
      </button>
    </div>
  </header>

  <div class="toolbar">
    <SearchBox value={q} placeholder={t('procs.search')} onsearch={(v) => (q = v)} />
    <div class="seg" role="radiogroup" aria-label={t('procs.mode.label')}>
      {#each MODES as [id, label, g] (id)}
        <button type="button" role="radio" aria-checked={mode === id} class:active={mode === id} onclick={() => setMode(id)}><Glyph name={g} size={12} /> {label}</button>
      {/each}
    </div>
    <div class="col-menu-wrap">
      <button type="button" class="btn sm" aria-haspopup="true" aria-expanded={colMenu} onclick={() => (colMenu = !colMenu)}><Glyph name="list" size={12} /> {t('procs.cols.btn')}</button>
      {#if colMenu}
        <div class="cm-backdrop" role="presentation" onpointerdown={() => (colMenu = false)}></div>
        <div class="col-pop" role="dialog" aria-label={t('procs.cols.pick')} tabindex="-1" onkeydown={(e) => e.key === 'Escape' && (colMenu = false)}>
          {#each COLS as c (c.key)}
            <label class="check"><input type="checkbox" checked={c.key === 'name' || colOn[c.key]} disabled={c.key === 'name'} onchange={() => toggleCol(c.key)} /> {COL_LABELS[c.key]}</label>
          {/each}
          <button type="button" class="btn sm ghost" onclick={resetCols}>{t('procs.cols.reset')}</button>
        </div>
      {/if}
    </div>
  </div>
  <div class="chips">
    {#each CHIPS as [id, label] (id)}
      <button type="button" class="chip" class:active={chip === id} aria-pressed={chip === id} onclick={() => setChip(id)}>{label}</button>
    {/each}
    <button type="button" class="chip" class:active={hideSys} aria-pressed={hideSys} onclick={toggleHideSys}><Glyph name="shield" size={11} /> {t('procs.chip.hide_sys')}</button>
  </div>

  <div class="pt" style="--cols:{template}; --row-h:{ROW_H}px; --head-h:{HEAD_H}px">
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div class="pt-scroll" bind:this={scroller} bind:clientHeight={viewH} onscroll={onScroll} onkeydown={tableKey} tabindex="0" role="treegrid" aria-label={t('procs.table.label')} aria-rowcount={rows.length + 1} aria-multiselectable="true">
      <div class="pt-head" role="row" style="min-width:{minW}px">
        {#each cols as c (c.key)}
          <div class="pth" class:r={c.align === 'r'} class:sorted={sort.key === c.key} role="columnheader" aria-sort={sort.key === c.key ? (sort.desc ? 'descending' : 'ascending') : 'none'}>
            <button type="button" class="pth-btn" onclick={() => onsort(c.key)} title={t('procs.col.sort_by', { col: COL_LABELS[c.key] })}>
              <span class="pth-total" title={totalTip[c.key]}>{totals[c.key] ?? ''}</span>
              <span class="pth-label"><span class="ellipsis">{COL_LABELS[c.key]}</span><span class="arrow">{sort.key === c.key ? (sort.desc ? '↓' : '↑') : ''}</span></span>
            </button>
            <button type="button" class="resizer" tabindex="-1" aria-label={t('procs.col.resize_label', { col: COL_LABELS[c.key] })} title={t('procs.col.resize_tip')} onpointerdown={(e) => startResize(e, c)} ondblclick={() => resetWidth(c)}></button>
          </div>
        {/each}
      </div>

      {#if !procs.loaded}
        <div class="pt-body">
          {#each Array(16) as _, i (i)}
            <div class="ptr skeleton-row" style="opacity:{1 - i * 0.05}">{#each cols as c (c.key)}<div class="pc"><span class="sk" style="width:{40 + ((i * 31 + c.width) % 50)}%"></span></div>{/each}</div>
          {/each}
        </div>
      {:else if rows.length === 0}
        <div class="dt-empty">{t('procs.empty')}</div>
      {:else}
        <div class="pt-body" style="height:{rows.length * ROW_H}px; min-width:{minW}px">
          <div class="dt-window" style="transform:translateY({start * ROW_H}px)">
            {#each vis as r (r.key)}
              {#if r.type === 'section'}
                <div class="ptr sec" role="row"><div class="sec-label" role="gridcell">{r.label}</div></div>
              {:else}
                {@const icon = (procs.iconV, r.exe ? procs.icons.get(r.exe) : null)}
                <!-- svelte-ignore a11y_click_events_have_key_events -->
                <div
                  class="ptr"
                  class:sel={sel.has(r.key)}
                  class:focus={focusKey === r.key}
                  class:dim={r.dim}
                  class:grp={r.type === 'group'}
                  role="row"
                  tabindex="-1"
                  aria-selected={sel.has(r.key)}
                  aria-expanded={r.expandable ? r.expanded : undefined}
                  aria-level={r.depth + 1}
                  onclick={(e) => rowClick(e, r)}
                  ondblclick={() => openDetails(r)}
                  oncontextmenu={(e) => rowMenu(e, r)}
                >
                  {#each cols as c (c.key)}
                    {#if c.key === 'name'}
                      <div class="pc name" role="gridcell" style="padding-left:{6 + (mode === 'tree' ? r.depth * 16 : Math.max(0, r.depth - 1) * 22)}px">
                        {#if r.expandable}
                          <button type="button" class="exp" tabindex="-1" aria-label={r.expanded ? t('procs.collapse') : t('procs.expand')} onclick={(e) => { e.stopPropagation(); toggleExpand(r); }}>
                            <Glyph name={r.expanded ? 'chevronDown' : 'chevronRight'} size={9} />
                          </button>
                        {:else}
                          <span class="exp"></span>
                        {/if}
                        <span class="picon">{#if icon}<img src={icon} width="16" height="16" alt="" />{:else}<Glyph name={r.kind === 'windows' ? 'settings' : 'app'} size={13} />{/if}</span>
                        <span class="ellipsis" title={r.title ? `${r.label} – ${r.title}` : r.p?.exe || r.label}>{r.label}{#if r.type === 'group'}&nbsp;<span class="cnt">({r.count})</span>{/if}</span>
                      </div>
                    {:else if c.heat}
                      {@const lv = heat(c.key, r[c.key], memTotal)}
                      <div class="pc r hm h{lv}" role="gridcell">{cell(r, c.key)}</div>
                    {:else}
                      <div class="pc" class:r={c.align === 'r'} class:mono={c.key === 'cmd'} role="gridcell">{cell(r, c.key)}</div>
                    {/if}
                  {/each}
                </div>
              {/if}
            {/each}
          </div>
        </div>
      {/if}
    </div>
  </div>

 {/snippet}
 {#snippet inspector(overlay)}
  <ProcInspector target={inspTarget} selCount={sel.size} {overlay} onaction={(id) => run(id, selectedRows())} onmore={inspMore} onclose={() => closeInsp(overlay)} />
 {/snippet}
</SplitView>

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} items={menuItems} label={t('procs.menu.label')} onpick={pickMenu} onclose={closeMenu} />
{/if}
