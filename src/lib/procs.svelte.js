// Store tiến trình (contract v2, sự kiện `procs`).
//
// Dữ liệu nằm trong Map thường pid -> đối tượng P (không reactive, sửa tại chỗ). Mỗi sự kiện chỉ làm
// O(số hàng) việc gán số; sau đó tăng `version` một lần. Trang Tiến trình dựng danh sách hiển thị
// trong một $derived phụ thuộc `version`, và chỉ các hàng đang thấy được tạo đối tượng mới/render lại.
import * as api from './api.js';
import { toasts } from './toasts.svelte.js';
import { errText } from './format.js';
import { t } from './i18n.svelte.js';

const DEV = import.meta.env.DEV;
// Lịch sử ngắn cho ngăn chi tiết: HN mẫu gần nhất (≥ 2 phút ở chu kỳ 0,5 s) cho MỌI tiến trình,
// dạng vòng đệm Float32Array cố định -> mỗi tick chỉ ghi số, không cấp phát (~0,9 MB cho 300 tiến trình).
export const HN = 240;

class Procs {
  map = new Map(); // pid -> P
  windows = new Map(); // pid -> tiêu đề cửa sổ (nguồn chuẩn cho "có cửa sổ")
  hasWindows = false; // đã nhận bản đồ windows ít nhất một lần
  t = 0; // thời điểm của sự kiện gần nhất (ms)
  version = $state(0);
  paused = $state(false);
  /** PID cần chọn sẵn khi mở trang Tiến trình (từ Cổng / ngăn chi tiết dịch vụ); trang đọc rồi xoá. */
  revealPid = 0;
  loaded = $state(false);
  totalDisk = 0; // tổng đĩa (B/s) của mọi tiến trình
  // icon: path -> dataURL | null
  icons = new Map();
  iconV = $state(0);
  #iconPending = new Set();
  #iconTimer = 0;

  // lịch sử: tick k (tăng dần), thời điểm từng tick, và pid -> { since, cpu, mem, disk }
  #k = -1;
  #ht = new Float64Array(HN);
  #ph = new Map();

  #unlisten = null;
  #watchers = 0;
  /** Thời gian xử lý sự kiện gần nhất (ms), chỉ để đo trong bản dev. */
  perf = DEV ? { last: 0, samples: [] } : null;

  async watch() {
    if (++this.#watchers > 1) return;
    try {
      this.#unlisten = await api.listen('procs', (p) => this.onProcs(p));
      if (this.#watchers === 0) { this.#unlisten(); this.#unlisten = null; return; }
      await api.setWatch(true);
    } catch (e) {
      toasts.error(t('procs.toast.watch_fail'), [errText(e)]);
    }
  }
  unwatch() {
    if (--this.#watchers > 0) return;
    this.#watchers = 0;
    this.#unlisten?.();
    this.#unlisten = null;
    api.setWatch(false).catch(() => {});
    // giải phóng: lần theo dõi sau sẽ nhận reset đầy đủ
    this.map.clear();
    this.windows.clear();
    this.#ph.clear();
    this.#k = -1;
    this.hasWindows = false;
    this.loaded = false;
  }

  onProcs(p) {
    if (!p) return;
    const t0 = DEV ? performance.now() : 0;
    const map = this.map;
    const ph = this.#ph;
    if (p.reset) { map.clear(); ph.clear(); }
    // `removed` TRƯỚC `added`: khi PID bị tái sử dụng, cùng một pid có thể nằm ở cả hai danh sách.
    for (const pid of p.removed || []) { map.delete(pid); this.windows.delete(pid); ph.delete(pid); }
    for (const a of p.added || []) {
      const lname = (a.name || '').toLowerCase();
      map.set(a.pid, {
        ...a,
        exe: a.exe || '',
        cmd: a.cmd || '',
        user: a.user || '',
        lexe: (a.exe || '').toLowerCase(),
        hay: `${lname}\n${a.pid}\n${(a.desc || '').toLowerCase()}\n${(a.exe || '').toLowerCase()}\n${(a.cmd || '').toLowerCase()}`,
        cpu: 0, mem: 0, disk: 0, gpu: 0, thr: 0, hnd: 0,
      });
    }
    let disk = 0;
    const k = ++this.#k;
    const slot = k % HN;
    this.#ht[slot] = p.t || Date.now();
    for (const r of p.rows || []) {
      const o = map.get(r[0]);
      if (!o) continue;
      o.cpu = r[1]; o.mem = r[2]; o.disk = r[3]; o.gpu = r[4]; o.thr = r[5]; o.hnd = r[6];
      disk += r[3];
      let h = ph.get(r[0]);
      if (!h) ph.set(r[0], (h = { since: k, cpu: new Float32Array(HN), mem: new Float32Array(HN), disk: new Float32Array(HN) }));
      h.cpu[slot] = r[1]; h.mem[slot] = r[2]; h.disk[slot] = r[3];
    }
    this.totalDisk = disk;
    if (p.windows) {
      const w = this.windows;
      w.clear();
      for (const k in p.windows) w.set(Number(k), p.windows[k]);
      this.hasWindows = true;
    } else if (p.reset) {
      this.windows.clear();
    }
    this.t = p.t || Date.now();
    if (!this.loaded) this.loaded = true;
    if (!this.paused || p.reset) this.version++;
    if (DEV) {
      // handler = chỉ cập nhật Map; total = cả lần flush DOM của Svelte (microtask của Svelte đã
      // được xếp hàng khi `version++`, nên microtask này chạy sau nó).
      const handler = performance.now() - t0;
      queueMicrotask(() => {
        const total = performance.now() - t0;
        const s = this.perf.samples;
        s.push({ handler, total, n: map.size });
        if (s.length > 300) s.shift();
        this.perf.last = total;
      });
    }
  }

  /** Lịch sử gần đây (cộng dồn nếu nhiều pid) -> { t, cpu, mem, disk } mảng thường, dùng cho Chart. */
  history(pids) {
    const k = this.#k;
    const out = { t: [], cpu: [], mem: [], disk: [] };
    const hs = [];
    for (const pid of pids) { const h = this.#ph.get(pid); if (h) hs.push(h); }
    if (k < 0 || !hs.length) return out;
    let from = Math.max(0, k - HN + 1);
    from = Math.max(from, Math.min(...hs.map((h) => h.since)));
    for (let j = from; j <= k; j++) {
      const s = j % HN;
      let c = 0, m = 0, d = 0;
      for (const h of hs) if (h.since <= j) { c += h.cpu[s]; m += h.mem[s]; d += h.disk[s]; }
      out.t.push(this.#ht[s]); out.cpu.push(c); out.mem.push(m); out.disk.push(d);
    }
    return out;
  }

  snapshot(pids) {
    return new Map(Array.from(pids, (pid) => [pid, this.map.get(pid)]));
  }

  stillSame(snapshot) {
    const same = [], gone = [];
    for (const [pid, entry] of snapshot) {
      (entry && this.map.get(pid) === entry ? same : gone).push(pid);
    }
    return { same, gone };
  }

  /** Bỏ ngay các pid đã kết thúc (không chờ tick sau). */
  dropLocal(pids) {
    let n = 0;
    for (const pid of pids) if (this.map.delete(pid)) { this.windows.delete(pid); this.#ph.delete(pid); n++; }
    if (n) this.version++;
  }

  isApp(p) {
    return this.windows.has(p.pid) || p.kind === 'app';
  }

  // ---------- icon ----------
  /** Yêu cầu icon cho các đường dẫn chưa có (gộp + debounce). */
  wantIcons(paths) {
    let any = false;
    for (const path of paths) {
      if (!path || this.icons.has(path) || this.#iconPending.has(path)) continue;
      this.#iconPending.add(path);
      any = true;
    }
    if (!any || this.#iconTimer) return;
    this.#iconTimer = setTimeout(() => this.#fetchIcons(), 120);
  }
  async #fetchIcons() {
    this.#iconTimer = 0;
    const paths = [...this.#iconPending].slice(0, 64);
    if (!paths.length) return;
    try {
      const res = (await api.getIcons(paths)) || {};
      for (const p of paths) this.icons.set(p, res[p] ?? null);
    } catch {
      for (const p of paths) this.icons.set(p, null);
    } finally {
      for (const p of paths) this.#iconPending.delete(p);
    }
    this.iconV++;
    if (this.#iconPending.size) this.#iconTimer = setTimeout(() => this.#fetchIcons(), 50);
  }
}

export const procs = new Procs();
// Chỉ bản dev: cho phép đo từ DevTools (window.__fomegProcs.perf.samples).
if (DEV && typeof window !== 'undefined') window.__fomegProcs = procs;

// Store dùng chung (một bản duy nhất): sửa file khi dev -> tải lại cả trang, tránh hai bản cũ / mới cùng chạy sau HMR.
if (import.meta.hot) import.meta.hot.decline();
