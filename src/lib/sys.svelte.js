// Store số liệu hệ thống (contract v2): mẫu `metrics` mới nhất, thông tin tĩnh, lịch sử dạng cột.
//
// Hiệu năng:
// - `m` là $state.raw: mỗi tick thay một đối tượng, component chỉ đọc trường cần.
// - Lịch sử `h` là các mảng thường (không reactive) + bộ đếm `hv`; biểu đồ vẽ lại khi `hv` đổi.
//   Cắt bớt đầu mảng theo lô (≥ 60 điểm) để không splice mỗi tick.
// - Mọi chuỗi trong lịch sử có cùng độ dài với `h.t`; thiết bị/chuỗi chỉ có từ sự kiện sống
//   (từng lõi, engine GPU, VRAM) được đệm NaN cho phần lịch sử cũ (biểu đồ bỏ qua NaN).
import * as api from './api.js';
import { prefs } from './prefs.svelte.js';
import { toasts } from './toasts.svelte.js';
import { errText } from './format.js';
import { t } from './i18n.svelte.js';

const KEEP_MS = 60 * 60 * 1000;
export const INTERVALS = [500, 1000, 2000, 5000, 0];

function pad(n) {
  return new Array(n).fill(NaN);
}

class Sys {
  m = $state.raw(null); // mẫu metrics mới nhất
  info = $state.raw(null); // system_info()
  lastT = $state(0); // thời điểm (máy khách) nhận mẫu gần nhất
  now = $state(Date.now()); // đồng hồ 1 s cho "Cập nhật Ns trước"
  hv = $state(0); // phiên bản lịch sử

  /** Lịch sử: t, cpu, mem, cores[i], disks[id]{active,read,write}, nets[id]{rx,tx}, gpus[id]{util,eng{name},ded,shared} */
  h = { t: [], cpu: [], mem: [], cores: [], disks: {}, nets: {}, gpus: {} };
  #all = []; // mọi mảng chuỗi (để đệm/cắt đồng loạt)
  #started = false;
  #histLoaded = false;
  #pending = []; // mẫu sống nhận trước khi có metrics_history

  init() {
    if (this.#started) return;
    this.#started = true;
    this.#all = [this.h.cpu, this.h.mem];
    api.systemInfo().then((i) => (this.info = i)).catch((e) => toasts.error(t('perf.toast.info_fail'), [errText(e)]));
    api.listen('metrics', (m) => this.onMetrics(m)).catch((e) => toasts.error(t('perf.toast.metrics_fail'), [errText(e)]));
    api.metricsHistory()
      .then((hist) => this.#loadHistory(hist))
      .catch(() => this.#loadHistory(null));
    if (prefs.interval !== 1000) this.setInterval(prefs.interval, true);
    if (prefs.idleMs !== 5000) this.setIdle(prefs.idleMs, true);

    const vis = () => {
      const v = !document.hidden;
      api.setVisible(v).catch(() => {});
      if (v) this.now = Date.now();
    };
    document.addEventListener('visibilitychange', vis);
    if (document.hidden) vis();
    // Đồng hồ cho thanh trạng thái; không làm gì khi cửa sổ ẩn.
    setInterval(() => {
      if (!document.hidden) this.now = Date.now();
    }, 1000);
  }

  async setInterval(ms, silent = false) {
    const prev = prefs.interval;
    prefs.set('interval', ms);
    try {
      await api.setTickInterval(ms);
    } catch (e) {
      prefs.set('interval', prev);
      if (!silent) toasts.error(t('perf.toast.interval_fail'), [errText(e)]);
    }
  }

  async setIdle(ms, silent = false) {
    const prev = prefs.idleMs;
    prefs.set('idleMs', ms);
    try {
      await api.setIdleInterval(ms);
    } catch (e) {
      prefs.set('idleMs', prev);
      if (!silent) toasts.error(t('perf.toast.idle_fail'), [errText(e)]);
    }
  }

  // ---------- lịch sử ----------
  #series(arr) {
    this.#all.push(arr);
    return arr;
  }

  #loadHistory(hist) {
    const h = this.h;
    if (hist && Array.isArray(hist.t) && hist.t.length) {
      const n = hist.t.length;
      const take = (a) => (Array.isArray(a) && a.length === n ? a.slice() : pad(n));
      h.t.push(...hist.t);
      h.cpu.push(...take(hist.cpu));
      h.mem.push(...take(hist.mem));
      for (const [id, d] of Object.entries(hist.disks || {})) {
        h.disks[id] = { active: this.#series(take(d.active)), read: this.#series(take(d.read)), write: this.#series(take(d.write)) };
      }
      for (const [id, d] of Object.entries(hist.nets || {})) {
        h.nets[id] = { rx: this.#series(take(d.rx)), tx: this.#series(take(d.tx)) };
      }
      for (const [id, u] of Object.entries(hist.gpus || {})) {
        h.gpus[id] = { util: this.#series(take(u)), eng: {}, ded: this.#series(pad(n)), shared: this.#series(pad(n)) };
      }
    }
    this.#histLoaded = true;
    const last = h.t.length ? h.t[h.t.length - 1] : 0;
    for (const m of this.#pending) if (m.t > last) this.#push(m);
    this.#pending = [];
    this.hv++;
  }

  onMetrics(m) {
    if (!m) return;
    this.m = m;
    this.lastT = Date.now();
    this.now = this.lastT;
    if (!this.#histLoaded) {
      this.#pending.push(m);
      return;
    }
    this.#push(m);
    this.hv++;
  }

  #push(m) {
    const h = this.h;
    const n = h.t.length;
    if (n && m.t <= h.t[n - 1]) return;
    h.t.push(m.t);
    h.cpu.push(m.cpu?.total ?? NaN);
    h.mem.push(m.mem?.used ?? NaN);
    const cores = m.cpu?.cores || [];
    for (let i = 0; i < cores.length; i++) {
      if (!h.cores[i]) h.cores[i] = this.#series(pad(n));
      h.cores[i].push(cores[i]);
    }
    for (const d of m.disks || []) {
      let s = h.disks[d.id];
      if (!s) s = h.disks[d.id] = { active: this.#series(pad(n)), read: this.#series(pad(n)), write: this.#series(pad(n)) };
      s.active.push(d.active); s.read.push(d.read); s.write.push(d.write);
    }
    for (const d of m.nets || []) {
      let s = h.nets[d.id];
      if (!s) s = h.nets[d.id] = { rx: this.#series(pad(n)), tx: this.#series(pad(n)) };
      s.rx.push(d.rx); s.tx.push(d.tx);
    }
    for (const g of m.gpus || []) {
      let s = h.gpus[g.id];
      if (!s) s = h.gpus[g.id] = { util: this.#series(pad(n)), eng: {}, ded: this.#series(pad(n)), shared: this.#series(pad(n)) };
      s.util.push(g.util);
      s.ded.push(g.ded_used);
      s.shared.push(g.shared_used);
      for (const e of g.engines || []) {
        let a = s.eng[e.name];
        if (!a) a = s.eng[e.name] = this.#series(pad(n));
        a.push(e.util);
      }
    }
    // chuỗi không có trong mẫu này (thiết bị biến mất) -> đệm để giữ thẳng hàng
    const len = n + 1;
    for (const a of this.#all) if (a.length < len) a.push(NaN);
    // cắt theo lô
    const cut = m.t - KEEP_MS;
    if (h.t[0] < cut && h.t.length > 120) {
      let k = 0;
      while (k < h.t.length && h.t[k] < cut) k++;
      if (k >= 60) {
        h.t.splice(0, k);
        for (const a of this.#all) a.splice(0, k);
      }
    }
  }
}

export const sys = new Sys();
// Chỉ bản dev: cho phép đo từ DevTools.
if (import.meta.env.DEV && typeof window !== 'undefined') window.__fomegSys = sys;

// Store dùng chung (một bản duy nhất): sửa file khi dev -> tải lại cả trang, tránh hai bản cũ / mới cùng chạy sau HMR.
if (import.meta.hot) import.meta.hot.decline();
