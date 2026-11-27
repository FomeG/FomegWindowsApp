// Backend giả lập cho trang Ổ cứng (du_*), chỉ dùng khi chạy trong trình duyệt.
// Cây được sinh dần theo hạt giống của đường dẫn; số liệu "lớn dần" theo thời gian để mô phỏng quét trực tiếp.
const GB = 1024 ** 3;
const MB = 1024 ** 2;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function hash(s) {
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) h = Math.imul(h ^ s.charCodeAt(i), 16777619);
  return h >>> 0;
}
function rng(seed) {
  let x = seed || 1;
  return () => ((x = Math.imul(x ^ (x >>> 15), 2246822519) ^ Math.imul(x ^ (x >>> 13), 3266489917)), ((x ^= x >>> 16) >>> 0) / 4294967296);
}

const DRIVES = [
  { root: 'C:\\', letter: 'C:', label: 'Local Disk', fs: 'NTFS', kind: 'fixed', total: 400 * GB, free: 2 * GB, ready: true },
  { root: 'D:\\', letter: 'D:', label: 'Games', fs: 'NTFS', kind: 'fixed', total: 500 * GB, free: 120 * GB, ready: true },
  { root: 'E:\\', letter: 'E:', label: 'Docs / Applications', fs: 'NTFS', kind: 'fixed', total: 300 * GB, free: 88 * GB, ready: true },
  { root: 'F:\\', letter: 'F:', label: 'Other', fs: 'exFAT', kind: 'fixed', total: 200 * GB, free: 150 * GB, ready: true },
  { root: 'Z:\\', letter: 'Z:', label: '', fs: '', kind: 'network', total: 0, free: 0, ready: false },
];
const C_TOP = [
  ['Users', 231], ['Windows', 64], ['Program Files', 38.5], ['Program Files (x86)', 17.9], ['ProgramData', 13.5], ['swsetup', 5.5],
  ['tmp', 0.8], ['Games', 0.6], ['temp-chromexz1', 0.5], ['SQL2022', 0.4], ['LDPlayer', 0.2], ['$Recycle.Bin', 0.01], ['Recovery', 0], ['Documents and Settings', 0, 'link'], ['System Volume Information', 0, 'denied'],
];
const NAMES = ['AppData', 'Local', 'Roaming', 'Microsoft', 'Google', 'Chrome', 'User Data', 'Default', 'Cache', 'Code', 'Temp', 'Packages', 'node_modules', 'cache', 'Python312', 'Lib', 'site-packages', 'assets', 'data', 'logs', 'bin', 'lib', 'share', 'resources', 'Downloads', 'Documents', 'Videos', 'Pictures', 'Steam', 'workDir', 'uv', 'archive-v0', '.cargo', 'registry', 'target', 'release', 'build', 'deps'];
const EXT = ['.dll', '.exe', '.json', '.log', '.bin', '.dat', '.png', '.mp4', '.zip', '.pak', '.db', '.js'];

const PROTECTED = /^[a-z]:\\?$|^[a-z]:\\(windows|program files( \(x86\))?|programdata|users)$|^[a-z]:\\windows\\system32/i;

export function createDuMock(emit) {
  let seq = 0;
  let cur = null; // { id, root, t0, dur, nodes: [], cancelled, doneAt }

  function mkNode(s, parent, name, size, files, opts = {}) {
    const r = rng(hash(name + size));
    const node = {
      id: s.nodes.length, parent, name, size, files: Math.max(0, Math.round(files)),
      alloc: Math.round(size * (1.01 + r() * 0.03)),
      mtime: Date.now() - Math.floor(r() * 400) * 86400000,
      start: r() * 0.7, // thời điểm (phần của tổng thời gian quét) bắt đầu "lộ" số liệu
      link: opts.link || false, denied: opts.denied || false,
      kids: null, removed: false,
    };
    s.nodes.push(node);
    return node;
  }
  function gen(s, n) {
    if (n.kids) return;
    n.kids = [];
    if (n.link || n.denied) { n.own_size = 0; n.own_files = 0; return; }
    const r = rng(hash(n.name + n.id));
    if (n.parent == null && /^c:\\$/i.test(s.root)) {
      for (const [name, gb, flag] of C_TOP) n.kids.push(mkNode(s, n.id, name, gb * GB, gb * 9000 + 3, { link: flag === 'link', denied: flag === 'denied' }).id);
      n.own_size = 14.2 * GB; n.own_files = 7;
    } else {
      const k = n.size < 20 * MB ? 0 : Math.floor(2 + r() * 9);
      n.own_size = n.size * (k ? 0.05 + r() * 0.15 : 1);
      n.own_files = Math.max(k ? 0 : 1, Math.round(n.files * (k ? 0.1 : 1)));
      let rest = n.size - n.own_size, restF = n.files - n.own_files;
      const ws = Array.from({ length: k }, () => r() ** 2.5);
      const sum = ws.reduce((a, b) => a + b, 0) || 1;
      const used = new Set();
      for (const w of ws) {
        let name = NAMES[Math.floor(r() * NAMES.length)];
        while (used.has(name)) name += '_';
        used.add(name);
        n.kids.push(mkNode(s, n.id, name, (rest * w) / sum, (restF * w) / sum).id);
      }
    }
    n.dirs = 0;
  }
  function frac(s) {
    if (s.cancelled != null) return s.cancelled;
    return Math.min(1, (Date.now() - s.t0) / s.dur);
  }
  function shown(s, n) {
    const f = frac(s);
    const g = Math.max(0, Math.min(1, (f - n.start * 0.6) / Math.max(0.05, 1 - n.start * 0.6)));
    return g;
  }
  function out(s, n) {
    gen(s, n);
    const g = shown(s, n);
    const kids = n.kids.filter((c) => !s.nodes[c].removed);
    const dirs = kids.length + kids.reduce((a, c) => a + Math.round((s.nodes[c].files || 0) / 40), 0);
    return {
      id: n.id, name: n.name, size: Math.round(n.size * g), alloc: Math.round(n.alloc * g), files: Math.round(n.files * g), dirs: Math.round(dirs * g),
      own_files: g > 0 ? n.own_files : 0, own_size: g > 0 ? Math.round(n.own_size) : 0, own_alloc: g > 0 ? Math.round(n.own_size * 1.02) : 0,
      mtime: n.mtime, done: g >= 1 || n.link || n.denied, ...(n.denied ? { denied: true } : {}), ...(n.link ? { link: true } : {}), kids: g > 0 ? kids.length : 0,
    };
  }
  function pathOf(s, n) {
    const parts = [];
    for (let x = n; x; x = x.parent == null ? null : s.nodes[x.parent]) parts.push(x.parent == null ? s.root : x.name);
    return parts.reverse().reduce((a, p) => (a ? (a.endsWith('\\') ? a + p : a + '\\' + p) : p), '');
  }
  function progress(s) {
    const f = frac(s);
    const root = s.nodes[0];
    const done = f >= 1 || s.cancelled != null;
    return { scan: s.id, root: s.root, files: Math.round(root.files * f), dirs: Math.round(root.files * f / 6), bytes: Math.round(root.size * f), errors: /^c:/i.test(s.root) ? 107 : 0, elapsed_ms: Math.round(Math.min(Date.now(), s.t0 + s.dur) - s.t0), done, cancelled: s.cancelled != null && s.cancelled < 1 };
  }

  /** Câu trả lời giả lập của trợ lý khi có kèm kết quả quét (dùng trong mock-v2 ai_send). */
  function aiReply(focus) {
    if (!cur) return null;
    const S = '\\';
    const user = ['C:', 'Users', 'ADMIN'].join(S);
    const base = focus || (/^c:/i.test(cur.root) ? [user, 'AppData', 'Local'].join(S) : cur.root.replace(/\\$/, ''));
    const sub = (...p) => [base, ...p].join(S);
    const lines = [
      `${sub('Temp')} | tệp tạm, Windows và ứng dụng tự tạo lại`,
      `${sub('uv', 'cache')} | cache của uv, tải lại khi cần`,
      `${sub('npm-cache')} | cache npm`,
      `${sub('Microsoft', 'Windows', 'INetCache')} | cache trình duyệt cũ`,
      `${sub('CrashDumps')} | bản ghi lỗi ứng dụng`,
    ];
    return [
      focus ? `### Thư mục \`${focus}\`` : `### Ổ ${cur.root} đang gần đầy`,
      '',
      `Phần lớn dung lượng nằm ở **${user}** (~231 GB). Các mục sau là **cache / tệp tạm**, xoá đi ứng dụng sẽ tự tạo lại:`,
      '',
      '```xoa',
      ...lines,
      '```',
      '',
      'Không nên xoá `Documents`, `Pictures` hay thư mục chương trình. Bấm nút ở trên để xem lại và tự chọn mục cần xoá.',
    ].join('\n');
  }

  /** Nút hoặc một tổ tiên của nó đã bị gỡ: con của thư mục đã xoá cũng không còn truy cập được. */
  const detached = (s, n) => {
    for (let x = n; x; x = x.parent == null ? null : s.nodes[x.parent]) if (x.removed) return true;
    return false;
  };
  /** Gỡ nút khỏi cây giả lập, trừ số liệu khỏi mọi thư mục cha (như Scan::forget). */
  function forgetNode(n) {
    if (!n || n.parent == null || n.removed) return;
    n.removed = true;
    for (let p = cur.nodes[n.parent]; p; p = p.parent == null ? null : cur.nodes[p.parent]) { p.size -= n.size; p.alloc -= n.alloc; p.files -= n.files; }
  }
  /** Giống du_delete thật: cây quét được cập nhật ngay ở backend (nút đã gỡ, hoặc tệp trừ đúng size / alloc) và ổ trống thêm khi xoá vĩnh viễn. */
  async function applyDelete(path, permanent) {
    const s = cur;
    let freed = 0;
    const norm = (x) => x.replace(/\\+$/, '').toLowerCase();
    const node = s?.nodes.find((x) => !detached(s, x) && norm(pathOf(s, x)) === norm(path));
    if (node) {
      freed = node.size;
      forgetNode(node);
    } else if (s) {
      const cut = path.lastIndexOf('\\');
      const dir = s.nodes.find((x) => !detached(s, x) && norm(pathOf(s, x)) === norm(path.slice(0, cut)));
      if (dir) {
        gen(s, dir);
        const f = (await mock.du_files({ path: pathOf(s, dir), limit: 99999 })).items.find((x) => x.name.toLowerCase() === path.slice(cut + 1).toLowerCase());
        if (f) {
          freed = f.size;
          (dir.gone ??= new Map()).set(f.name.toLowerCase(), f.size); // du_files không được liệt kê lại tệp này
          dir.own_files = Math.max(0, dir.own_files - 1);
          dir.own_size = Math.max(0, dir.own_size - f.size);
          for (let p = dir; p; p = p.parent == null ? null : s.nodes[p.parent]) { p.size -= f.size; p.alloc -= f.alloc; p.files -= 1; }
        }
      }
    }
    // Thùng rác nằm cùng ổ: chỉ xoá vĩnh viễn mới làm ổ trống thêm
    const d = DRIVES.find((x) => path.toUpperCase().startsWith(x.letter));
    if (d && permanent && freed) d.free = Math.min(d.total, d.free + freed);
  }

  const mock = {
    __aiReply: aiReply,
    /** Dọn dẹp giả lập giải phóng `bytes` trên ổ hệ thống (C:) -> số ổ trống của trang Ổ cứng tăng theo. */
    __freeBytes(bytes) {
      const d = DRIVES[0];
      if (d && bytes > 0) d.free = Math.min(d.total, d.free + bytes);
    },
    async du_drives() { await sleep(30); return structuredClone(DRIVES); },
    async du_start({ root }) {
      await sleep(40);
      root = String(root || '').trim();
      if (!/^[a-z]:/i.test(root) || /khong|missing/i.test(root)) throw 'Không tìm thấy đường dẫn.';
      if (/^[a-z]:$/i.test(root)) root += '\\';
      if (cur) cur.cancelled = cur.cancelled ?? frac(cur);
      const id = ++seq;
      const drive = DRIVES.find((d) => root.toUpperCase().startsWith(d.letter));
      const total = /^[a-z]:\\$/i.test(root) ? (drive ? drive.total - drive.free : 50 * GB) : 3 * GB + (hash(root) % 40) * GB;
      const s = { id, root, t0: Date.now(), dur: /^c:\\$/i.test(root) ? 9000 : 4000, nodes: [], cancelled: null };
      mkNode(s, null, root, total, total / 150000);
      s.nodes[0].start = 0;
      cur = s;
      const tick = () => {
        if (cur !== s) return;
        const p = progress(s);
        emit('du', p);
        if (!p.done) setTimeout(tick, 250);
      };
      setTimeout(tick, 250);
      return progress(s);
    },
    async du_progress() { return cur ? progress(cur) : null; },
    async du_children({ scan, ids, sort, desc, limit }) {
      await sleep(8);
      if (!cur || cur.id !== scan) throw 'Lần quét này đã kết thúc hoặc đã được thay bằng lần quét khác.';
      const s = cur;
      return ids.filter((id) => s.nodes[id] && !detached(s, s.nodes[id])).map((id) => {
        const n = s.nodes[id];
        const node = out(s, n);
        const kids = node.kids ? n.kids.filter((c) => !s.nodes[c].removed).map((c) => out(s, s.nodes[c])) : [];
        const v = (x) => (sort === 'name' ? x.name.toLowerCase() : x[sort] ?? x.size);
        kids.sort((a, b) => { const p = v(a), q = v(b); const o = p < q ? -1 : p > q ? 1 : 0; return desc ? -o : o; });
        return { node, path: pathOf(s, n), children: kids.slice(0, limit), more: Math.max(0, kids.length - limit) };
      });
    },
    async du_files({ path, limit }) {
      await sleep(60);
      const s = cur;
      const n = s?.nodes.find((x) => pathOf(s, x) === path && !detached(s, x));
      // tệp đã xoá vẫn nằm trong dãy sinh theo hạt giống: sinh đủ số cũ rồi lọc đúng những tệp đó ra
      const gone = n?.gone;
      const count = n ? n.own_files + (gone?.size || 0) : 5;
      const total = n ? n.own_size + [...(gone?.values() || [])].reduce((a, b) => a + b, 0) : 10 * MB;
      const r = rng(hash(path));
      const ws = Array.from({ length: count }, () => r() ** 3);
      const sum = ws.reduce((a, b) => a + b, 0) || 1;
      const items = ws.map((w, i) => ({ name: `${NAMES[i % NAMES.length].toLowerCase()}${i ? '-' + i : ''}${EXT[i % EXT.length]}`, size: Math.round((total * w) / sum), alloc: Math.ceil((total * w) / sum / 4096) * 4096, mtime: Date.now() - Math.floor(r() * 300) * 86400000 }));
      items.sort((a, b) => b.size - a.size);
      if (gone?.size) items.splice(0, items.length, ...items.filter((x) => !gone.has(x.name.toLowerCase())));
      return { total: items.length, items: items.slice(0, limit) };
    },
    async du_cancel() { if (cur && cur.cancelled == null) cur.cancelled = frac(cur); emit('du', progress(cur)); },
    async du_close() { cur = null; },
    /** Menu Windows không có trong trình duyệt; cây giả lập đã được cập nhật lúc xoá, nên chỉ trả kết quả rỗng. */
    async du_sync_paths() { await sleep(20); return { inexact: [] }; },
    async du_delete({ paths, permanent }) {
      await sleep(300);
      if (paths.some((p) => /^[a-z]:\\?$/i.test(p) || /\\windows$/i.test(p))) throw 'Không cho phép xoá thư mục hệ thống / gốc ổ.';
      console.info('[mock] du_delete', paths, permanent);
      for (const p of paths) await applyDelete(p, permanent);
      deleted.push(...paths);
      return { done: true, inexact: [] };
    },
    async du_lookup({ paths }) {
      await sleep(120);
      return paths.map((raw) => {
        const path = String(raw).trim().replace(/\//g, '\\').replace(/(.{3,}?)\\+$/, '$1');
        if (!/^[a-z]:\\|^\\\\/i.test(path)) return { path: raw, exists: false, is_dir: false, size: null, files: null, blocked: 'Không phải đường dẫn đầy đủ' };
        const gone = deleted.includes(path) || /crashdumps/i.test(path);
        const isDir = !/\.[a-z0-9]{2,4}$/i.test(path);
        const h = hash(path.toLowerCase());
        const size = isDir ? (0.3 + (h % 90) / 10) * GB : (h % 900) * MB;
        const blocked = gone ? 'Không còn tồn tại' : PROTECTED.test(path) ? 'Thư mục hệ thống / gốc ổ – không cho phép xoá' : undefined;
        return { path, exists: !gone, is_dir: isDir, size: gone ? null : Math.round(size), files: isDir && !gone ? 2000 + (h % 90000) : null, ...(blocked ? { blocked } : {}) };
      });
    },
    async du_delete_paths({ paths, permanent }) {
      await sleep(600);
      console.info('[mock] du_delete_paths', paths, permanent);
      const res = [];
      for (const path of paths) {
        if (PROTECTED.test(path)) { res.push({ path, ok: false, error: 'Thư mục hệ thống / gốc ổ – không cho phép xoá' }); continue; }
        // mô phỏng một mục đang bị khoá để xem cách hiện lỗi từng mục
        if (/\\inetcache$/i.test(path)) { res.push({ path, ok: false, error: 'Tệp đang được một chương trình khác sử dụng.' }); continue; }
        await applyDelete(path, permanent);
        deleted.push(path);
        res.push({ path, ok: true });
      }
      return res;
    },
    async du_exists({ path }) { return !deleted.includes(path); },
    async du_open({ path, is_dir }) { console.info('[mock] du_open', path, is_dir); return null; },
    async du_menu() { throw 'Không có menu Windows trong trình duyệt'; },
    async du_pick_folder() { return null; },
  };
  return mock;
}
const deleted = [];
