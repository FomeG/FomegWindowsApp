// Dựng danh sách hàng hiển thị cho trang Tiến trình từ Map pid -> P (thuần JS, không reactive).
//
// Ba chế độ:
// - group: phần "Ứng dụng" / "Tiến trình nền" / "Tiến trình Windows", trong mỗi phần gộp theo exe
//   thành một hàng mở rộng được ("Google Chrome (15)") với số liệu cộng dồn.
// - list: phẳng.
// - tree: cây cha/con theo ppid.
// Mỗi lần gọi tạo đối tượng hàng mới (rẻ: ~300 đối tượng nhỏ) để bảng ảo hoá thấy giá trị mới.
import { collator } from './format.js';
import { t } from './i18n.svelte.js';

export const MIN5 = 5 * 60 * 1000;

// phần của chế độ Nhóm -> khoá nhãn (t() gọi khi dựng hàng, nên đổi ngôn ngữ là đổi ngay)
const SECTIONS = [['app', 'procs.section.app'], ['background', 'procs.section.background'], ['windows', 'procs.section.windows']];

const NUM = { cpu: 1, mem: 1, disk: 1, gpu: 1, thr: 1, hnd: 1, pid: 1, uptime: 1 };

function cmpFor(sort) {
  const { key, desc } = sort;
  const dir = desc ? -1 : 1;
  if (key === 'uptime') return (a, b) => ((b.start_ms || 0) - (a.start_ms || 0)) * dir || collator.compare(a.label, b.label);
  if (NUM[key]) return (a, b) => ((a[key] || 0) - (b[key] || 0)) * dir || collator.compare(a.label, b.label) || a.pid - b.pid;
  if (key === 'name') return (a, b) => collator.compare(a.label, b.label) * dir || (a.pid || 0) - (b.pid || 0);
  return (a, b) => collator.compare(a[key] || '', b[key] || '') * dir || collator.compare(a.label, b.label);
}

function procRow(p, depth, extra) {
  return {
    key: 'p' + p.pid,
    type: 'proc',
    depth,
    p,
    pid: p.pid,
    label: p.name,
    sub: '',
    exe: p.exe,
    cpu: p.cpu, mem: p.mem, disk: p.disk, gpu: p.gpu, thr: p.thr, hnd: p.hnd,
    start_ms: p.start_ms,
    user: p.user,
    publisher: p.publisher,
    cmd: p.cmd,
    kind: p.kind,
    pids: null,
    count: 1,
    expandable: false,
    expanded: false,
    ...extra,
  };
}

/**
 * @param {Map<number, any>} map
 * @param {{ mode: 'group'|'list'|'tree', q: string, chip: string, hideSys: boolean, sort: {key:string, desc:boolean},
 *           expanded: Set<string>, collapsed: Set<number>, now: number, windows: Map<number,string>, memTotal: number }} o
 */
export function buildRows(map, o) {
  const { q, chip, hideSys, now, windows } = o;
  const isApp = (p) => windows.has(p.pid) || p.kind === 'app';
  const pass = (p) => {
    if (hideSys && p.kind === 'windows' && !isApp(p)) return false;
    if (chip === 'cpu' && !(p.cpu >= 1)) return false;
    if (chip === 'mem' && !(p.mem >= 200 * 1048576)) return false;
    if (chip === 'recent' && !(now - (p.start_ms || 0) < MIN5)) return false;
    if (q && !p.hay.includes(q)) return false;
    return true;
  };
  const cmp = cmpFor(o.sort);
  const filtering = !!(q || chip !== 'all' || hideSys);

  if (o.mode === 'list') {
    const out = [];
    for (const p of map.values()) if (pass(p)) out.push(procRow(p, 0));
    out.sort(cmp);
    return { rows: out, shown: out.length };
  }

  if (o.mode === 'tree') {
    const kids = new Map();
    const roots = [];
    for (const p of map.values()) {
      const par = p.ppid !== p.pid && map.has(p.ppid) ? p.ppid : null;
      if (par == null) roots.push(p);
      else {
        let a = kids.get(par);
        if (!a) kids.set(par, (a = []));
        a.push(p);
      }
    }
    // có khớp bộ lọc trong cây con? (chỉ tính khi đang lọc)
    const keep = new Map();
    const seen = new Set();
    const mark = (p) => {
      if (keep.has(p.pid)) return keep.get(p.pid);
      if (seen.has(p.pid)) return false; // chống vòng lặp ppid
      seen.add(p.pid);
      let k = pass(p);
      for (const c of kids.get(p.pid) || []) if (mark(c)) k = true;
      keep.set(p.pid, k);
      return k;
    };
    if (filtering) for (const r of roots) mark(r);
    const out = [];
    let shown = 0;
    const visited = new Set();
    const walk = (list, depth) => {
      const rows = [];
      for (const p of list) {
        if (visited.has(p.pid)) continue;
        if (filtering && !keep.get(p.pid)) continue;
        const ch = kids.get(p.pid);
        const has = !!(ch && ch.length);
        rows.push(procRow(p, depth, { expandable: has, expanded: has && (filtering || !o.collapsed.has(p.pid)), dim: filtering && !pass(p) }));
      }
      rows.sort(cmp);
      for (const r of rows) {
        visited.add(r.pid);
        out.push(r);
        if (!r.dim) shown++;
        if (r.expanded) walk(kids.get(r.pid), depth + 1);
      }
    };
    walk(roots, 0);
    return { rows: out, shown };
  }

  // ---- group ----
  const groups = new Map(); // key -> { section, members[] }
  for (const p of map.values()) {
    if (!pass(p)) continue;
    const gk = p.lexe || 'n:' + p.name.toLowerCase();
    let g = groups.get(gk);
    if (!g) groups.set(gk, (g = { key: gk, members: [], app: false, win: true }));
    g.members.push(p);
    if (isApp(p)) g.app = true;
    if (p.kind !== 'windows') g.win = false;
  }
  const sections = { app: [], background: [], windows: [] };
  let shown = 0;
  for (const g of groups.values()) {
    const sec = g.app ? 'app' : g.win ? 'windows' : 'background';
    const m = g.members;
    shown += m.length;
    if (m.length === 1) {
      sections[sec].push(procRow(m[0], 1, { key: 'p' + m[0].pid, label: m[0].desc || m[0].name, sub: m[0].name, title: windows.get(m[0].pid) }));
      continue;
    }
    let cpu = 0, mem = 0, disk = 0, gpu = 0, thr = 0, hnd = 0, start = Infinity;
    // gốc của nhóm: thành viên có cha không thuộc nhóm (hoặc pid nhỏ nhất)
    const inGroup = new Set(m.map((p) => p.pid));
    let root = null;
    for (const p of m) {
      cpu += p.cpu; mem += p.mem; disk += p.disk; gpu += p.gpu; thr += p.thr; hnd += p.hnd;
      if (p.start_ms < start) start = p.start_ms;
      if (!inGroup.has(p.ppid) && (!root || p.pid < root.pid)) root = p;
    }
    root = root || m[0];
    const label = root.desc || root.name;
    const key = 'g' + sec + ':' + g.key;
    const expanded = o.expanded.has(key);
    let title = '';
    for (const p of m) { const w = windows.get(p.pid); if (w) { title = w; break; } }
    sections[sec].push({
      key, type: 'group', depth: 1, p: root, pid: root.pid, label, sub: root.name, title,
      exe: root.exe, cpu, mem, disk, gpu: Math.min(100, gpu), thr, hnd, start_ms: start,
      user: root.user, publisher: root.publisher, cmd: root.cmd, kind: root.kind,
      pids: m.map((p) => p.pid), members: m, count: m.length, expandable: true, expanded,
    });
  }
  const out = [];
  for (const [sec, lk] of SECTIONS) {
    const list = sections[sec];
    if (!list.length) continue;
    list.sort(cmp);
    out.push({ key: 's' + sec, type: 'section', label: t(lk, { n: list.length }), depth: 0 });
    for (const r of list) {
      out.push(r);
      if (r.type === 'group' && r.expanded) {
        const ch = r.members.map((p) => procRow(p, 2, { label: p.name, sub: '', title: windows.get(p.pid), parentKey: r.key }));
        ch.sort(cmp);
        for (const c of ch) out.push(c);
      }
    }
  }
  return { rows: out, shown };
}

/** Mức "nhiệt" 0..9 (−1 = không tô) cho ô số, giống Task Manager. */
export function heat(kind, v, memTotal) {
  let f;
  switch (kind) {
    case 'cpu': f = v / 40; break;
    case 'gpu': f = v / 40; break;
    case 'mem': f = memTotal ? Math.sqrt(v / (memTotal * 0.15)) : v / 2e9; break;
    case 'disk': f = Math.sqrt(v / (40 * 1048576)); break;
    default: return -1;
  }
  if (!(v > 0)) return 0;
  return Math.min(9, Math.max(0, Math.floor(f * 9.99)));
}
