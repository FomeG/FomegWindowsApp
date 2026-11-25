// Store trung tâm: danh sách mục, dịch vụ, RAM, lịch sử, cài đặt, trạng thái quét.
//
// Nguyên tắc hiệu năng:
// - `startup` / `services` là $state.raw: mỗi thay đổi thay bằng mảng mới, mục không đổi giữ nguyên
//   tham chiếu nên các hàng không bị render lại.
// - RAM sống nằm trong SvelteMap `ram` (id -> MB), theo dõi theo từng key: mỗi tick chỉ ô RAM
//   của id thay đổi được cập nhật; danh sách lọc/sắp xếp không phụ thuộc RAM sống.
// - Sắp xếp/lọc theo RAM dùng `ramSnap` (Map thường) + bộ đếm `snapVersion`.
// - Lịch sử RAM là mảng thường + bộ đếm `histVersion` (không tạo lại mảng mỗi tick).
import { SvelteMap, SvelteSet } from 'svelte/reactivity';
import * as api from './api.js';
import { loadPref } from './prefs.svelte.js';
import { toasts, summarize } from './toasts.svelte.js';
import { START_LABELS, errText, CRITICAL } from './format.js';
import { t } from './i18n.svelte.js';

const HIST_MS = 60 * 60 * 1000;
/** Nguồn hiện mặc định ở trang Khởi động. */
export const DEFAULT_KINDS = ['Run', 'Folder'];
const isAutoMode = (m) => m === 'auto' || m === 'delayed';

class Store {
  // dữ liệu
  startup = $state.raw([]);
  services = $state.raw([]);
  startupById = $derived(new Map(this.startup.map((i) => [i.id, i])));
  servicesById = $derived(new Map(this.services.map((i) => [i.id, i])));

  // quét
  scanning = $state(false);
  loaded = $state(false);
  tookMs = $state(0);
  timings = $state.raw([]);
  warnings = $state.raw([]);

  // cài đặt
  settings = $state({ restart_apps: false, is_admin: true, log_path: '' });
  settingsLoaded = $state(false);
  restartPending = $state(false);

  // RAM
  ram = new SvelteMap();
  ramSnap = new Map();
  snapVersion = $state(0);
  ramUsed = $state(0);
  ramTotal = $state(0);
  top = $state.raw([]);
  history = [];
  histVersion = $state(0);

  // trạng thái thao tác
  pending = new SvelteSet(); // id đang xử lý
  selStartup = new SvelteSet();
  selServices = new SvelteSet();

  // giao diện
  page = $state('dashboard');
  confirmState = $state(null);
  /** Yêu cầu mở trang tới một mục: { list: 'startup' | 'services', id } – trang đọc rồi xoá. */
  focus = $state(null);
  // Bộ lọc trang Khởi động: tab bật/tắt + các nguồn đang chọn (mặc định chỉ Registry Run + Thư mục Startup,
  // giống Task Manager; mỗi lần mở app đều về mặc định). kinds rỗng = mọi nguồn.
  startupUi = $state({ q: '', tab: 'all', kinds: loadPref('ui:startupAll', false) ? [] : [...DEFAULT_KINDS], showMs: false, onlyRunning: false, sort: { key: 'name', desc: false } });
  // Bộ lọc trang Dịch vụ: tab trạng thái chạy + kiểu khởi động + chỉ bên thứ ba.
  servicesUi = $state({ q: '', tab: 'all', start: '', third: false, sort: { key: 'name', desc: false } });

  #started = false;

  init() {
    if (this.#started) return;
    this.#started = true;
    api.getSettings()
      .then((s) => { this.settings = { ...this.settings, ...s }; this.settingsLoaded = true; })
      .catch((e) => toasts.error(t('startup.err.settings'), [errText(e)]));
    api.ramHistory()
      .then((h) => {
        if (!h) return;
        if (h.total && !this.ramTotal) this.ramTotal = h.total;
        const pts = Array.isArray(h.points) ? h.points : [];
        // ghép với các điểm đã nhận qua sự kiện (nếu có)
        const lastT = pts.length ? pts[pts.length - 1][0] : 0;
        const extra = this.history.filter((p) => p[0] > lastT);
        this.history = pts.concat(extra);
        if (!this.ramUsed && this.history.length) this.ramUsed = this.history[this.history.length - 1][1];
        this.histVersion++;
      })
      .catch(() => {});
    this.scan();
    api.listen('ram', (p) => this.onRam(p)).catch((e) => toasts.error(t('startup.err.ram'), [errText(e)]));
  }

  // ---------- quét ----------
  /** Props cho RefreshButton của các trang dùng danh sách khởi động / dịch vụ (F5 cũng gọi scan()). */
  get refreshProps() {
    const tip = this.timings.length ? t('startup.refresh_tip') + '\n' + this.timings.map(([l, ms]) => `${l}: ${ms} ms`).join('\n') : t('startup.refresh_tip');
    return {
      onclick: () => this.scan(),
      busy: this.scanning,
      busyLabel: t('startup.scanning'),
      info: this.loaded && !this.scanning ? `${this.tookMs} ms` : '',
      title: tip,
    };
  }

  async scan() {
    if (this.scanning) return;
    this.scanning = true;
    try {
      const r = await api.scan();
      this.startup = r.startup || [];
      this.services = r.services || [];
      this.tookMs = r.took_ms || 0;
      this.timings = r.timings || [];
      this.warnings = r.warnings || [];
      this.loaded = true;
      this.#pruneSelection();
      this.snapshotRam();
      if (this.warnings.length) toasts.warn(t('startup.scan_warn', { count: this.warnings.length }), this.warnings);
    } catch (e) {
      toasts.error(t('startup.err.scan'), [errText(e)]);
    } finally {
      this.scanning = false;
    }
  }

  #pruneSelection() {
    for (const id of this.selStartup) if (!this.startupById.has(id)) this.selStartup.delete(id);
    for (const id of this.selServices) if (!this.servicesById.has(id)) this.selServices.delete(id);
  }

  // ---------- RAM ----------
  onRam(p) {
    if (!p) return;
    if (p.total) this.ramTotal = p.total;
    if (p.used != null) this.ramUsed = p.used;
    if (p.top) this.top = p.top;
    const items = p.items || [];
    if (p.reset) {
      const seen = new Set();
      for (const [id, mb] of items) {
        seen.add(id);
        if (this.ram.get(id) !== mb) this.ram.set(id, mb);
      }
      for (const id of [...this.ram.keys()]) if (!seen.has(id)) this.ram.delete(id);
      // lần đầu có dữ liệu RAM: chụp lại để sắp xếp theo RAM có ý nghĩa
      if (this.ramSnap.size === 0) this.snapshotRam();
    } else {
      for (const [id, mb] of items) this.ram.set(id, mb);
    }
    if (p.t && p.used != null) {
      const h = this.history;
      if (!h.length || p.t > h[h.length - 1][0]) h.push([p.t, p.used]);
      const cutoff = p.t - HIST_MS;
      let k = 0;
      while (k < h.length && h[k][0] < cutoff) k++;
      if (k) h.splice(0, k);
      this.histVersion++;
    }
  }

  /** Chụp RAM hiện tại để sắp xếp ổn định (hàng không nhảy khi RAM cập nhật). */
  snapshotRam() {
    this.ramSnap = new Map(this.ram);
    this.snapVersion++;
  }

  // ---------- thay thế phần tử ----------
  #replace(listName, updates) {
    if (!updates.size) return;
    this[listName] = this[listName].map((it) => updates.get(it.id) ?? it);
  }

  /** Cập nhật bản Svc và bản "Service" trong danh sách khởi động cho cùng tên dịch vụ. */
  #applyServices(svcByName) {
    if (!svcByName.size) return;
    const su = new Map();
    for (const s of this.services) {
      const n = svcByName.get(s.key1);
      if (n) su.set(s.id, { ...s, ...n, id: s.id, kind: 'Svc' });
    }
    this.#replace('services', su);
    const st = new Map();
    for (const it of this.startup) {
      if (it.kind !== 'Service') continue;
      const n = svcByName.get(it.key1);
      if (n) {
        st.set(it.id, {
          ...it,
          start_type: n.start_type ?? it.start_type,
          state: n.state ?? it.state,
          pid: n.pid ?? it.pid,
          shared: n.shared ?? it.shared,
          on: isAutoMode(n.start_type ?? it.start_type),
        });
      }
    }
    this.#replace('startup', st);
  }

  // ---------- bật/tắt mục khởi động ----------
  /** quiet: không báo toast. Trả Map id -> lỗi | '' (popup đề xuất của trợ lý tự báo). */
  async toggle(items, on, { quiet = false } = {}) {
    const todo = items.filter((it) => !this.pending.has(it.id) && it.on !== on);
    const results = new Map(items.filter((it) => it.on === on).map((it) => [it.id, '']));
    if (!todo.length) return results;
    const originals = new Map(todo.map((it) => [it.id, it]));
    // optimistic
    this.#replace('startup', new Map(todo.map((it) => [it.id, { ...it, on }])));
    for (const it of todo) this.pending.add(it.id);
    let ok = 0;
    const errors = [];
    try {
      const res = await api.toggleItems(todo, on);
      const upd = new Map();
      const svcSync = new Map();
      const byId = new Map((res || []).map((r) => [r.id, r]));
      for (const it of todo) {
        const r = byId.get(it.id);
        results.set(it.id, r && r.ok ? '' : r?.error || t('startup.err.no_response'));
        if (r && r.ok) {
          ok++;
          const ni = r.item ? { ...r.item } : { ...it, on };
          upd.set(it.id, ni);
          if (ni.kind === 'Service' && ni.key1) {
            svcSync.set(ni.key1, { start_type: ni.start_type, state: ni.state, pid: ni.pid });
          }
        } else {
          upd.set(it.id, originals.get(it.id));
          errors.push(`${it.name}: ${r?.error || t('startup.err.no_response')}`);
        }
      }
      this.#replace('startup', upd);
      this.#applyServices(svcSync);
    } catch (e) {
      this.#replace('startup', originals);
      errors.length = 0;
      errors.push(errText(e));
      ok = 0;
      for (const it of todo) results.set(it.id, errText(e));
    } finally {
      for (const it of todo) this.pending.delete(it.id);
    }
    if (quiet) return results;
    if (todo.length === 1) {
      const name = todo[0].name;
      if (errors.length) toasts.error(t(on ? 'startup.toast.enable_failed' : 'startup.toast.disable_failed', { name }), errors);
      else toasts.success(t(on ? 'startup.toast.enabled' : 'startup.toast.disabled', { name }));
    } else summarize(t(on ? 'startup.enable' : 'startup.disable'), t('startup.unit.items'), ok, errors);
    return results;
  }

  // ---------- dịch vụ ----------
  #svcPendingIds(names) {
    const ids = [];
    for (const s of this.services) if (names.includes(s.key1)) ids.push(s.id);
    for (const s of this.startup) if (s.kind === 'Service' && names.includes(s.key1)) ids.push(s.id);
    return ids;
  }

  /** svcs: mảng Svc item. mode: auto | delayed | demand | disabled */
  /** quiet: không báo toast. Trả Map tên dịch vụ -> lỗi | ''. */
  async setMode(svcs, mode, { confirmed = false, quiet = false } = {}) {
    const results = new Map(svcs.filter((s) => s.start_type === mode).map((s) => [s.key1, '']));
    let list = svcs.filter((s) => !this.pending.has(s.id) && s.start_type !== mode);
    const errors = [];
    if (mode === 'demand' || mode === 'disabled') {
      for (const s of list) if (s.critical) { errors.push(`${s.name}: ${CRITICAL.tip}`); results.set(s.key1, CRITICAL.tip); }
      list = list.filter((s) => !s.critical);
    }
    list = list.filter((s) => s.start_type !== 'boot' && s.start_type !== 'system');
    if (!list.length) {
      if (errors.length && !quiet) summarize(t('startup.svc.toast.change_to', { mode: START_LABELS[mode] }), t('startup.unit.services'), 0, errors);
      return results;
    }
    if (mode === 'disabled' && !confirmed) {
      const yes = await this.confirm({
        title: t('startup.svc.confirm.title'),
        message: t('startup.svc.confirm.disable', { count: list.length }),
        okLabel: t('startup.svc.disable_hard'),
        danger: true,
      });
      if (!yes) return results;
    }
    const names = list.map((s) => s.key1);
    const origByName = new Map(list.map((s) => [s.key1, { start_type: s.start_type, state: s.state, pid: s.pid }]));
    this.#applyServices(new Map(names.map((n) => [n, { start_type: mode }])));
    const ids = this.#svcPendingIds(names);
    for (const id of ids) this.pending.add(id);
    let ok = 0;
    try {
      const res = await api.serviceSetMode(names, mode);
      const byName = new Map((res || []).map((r) => [r.name, r]));
      const apply = new Map();
      for (const n of names) {
        const r = byName.get(n);
        results.set(n, r && r.ok ? '' : r?.error || t('startup.err.no_response'));
        if (r && r.ok) {
          ok++;
          apply.set(n, r.service ? { ...r.service } : { start_type: mode });
        } else {
          apply.set(n, origByName.get(n));
          const s = list.find((x) => x.key1 === n);
          errors.push(`${s?.name || n}: ${r?.error || t('startup.err.no_response')}`);
        }
      }
      this.#applyServices(apply);
    } catch (e) {
      this.#applyServices(origByName);
      errors.push(errText(e));
      for (const n of names) results.set(n, errText(e));
    } finally {
      for (const id of ids) this.pending.delete(id);
    }
    if (quiet) return results;
    const verb = t('startup.svc.toast.change_to', { mode: START_LABELS[mode] });
    if (list.length === 1 && errors.length <= 1) {
      if (errors.length) toasts.error(t('startup.svc.toast.mode_failed', { name: list[0].name, mode: START_LABELS[mode] }), errors);
      else toasts.success(`${list[0].name}: ${START_LABELS[mode]}`);
    } else summarize(verb, t('startup.unit.services'), ok, errors);
    return results;
  }

  /** action: start | stop. quiet: không báo toast. Trả Map tên dịch vụ -> lỗi | ''. */
  async control(svcs, action, { quiet = false } = {}) {
    const done = (s) => (action === 'start' ? s.state === 'running' : s.state === 'stopped');
    const results = new Map(svcs.filter(done).map((s) => [s.key1, '']));
    let list = svcs.filter((s) => !this.pending.has(s.id) && !done(s));
    const errors = [];
    if (action === 'stop') {
      for (const s of list) if (s.critical) { errors.push(`${s.name}: ${CRITICAL.tip}`); results.set(s.key1, CRITICAL.tip); }
      list = list.filter((s) => !s.critical);
    }
    const verb = t(action === 'start' ? 'startup.svc.start' : 'startup.svc.stop');
    if (!list.length) {
      if (errors.length && !quiet) summarize(verb, t('startup.unit.services'), 0, errors);
      return results;
    }
    const names = list.map((s) => s.key1);
    const ids = this.#svcPendingIds(names);
    for (const id of ids) this.pending.add(id);
    let ok = 0;
    try {
      const res = await api.serviceControl(names, action);
      const byName = new Map((res || []).map((r) => [r.name, r]));
      const apply = new Map();
      for (const n of names) {
        const r = byName.get(n);
        if (r?.service) apply.set(n, { ...r.service });
        results.set(n, r && r.ok ? '' : r?.error || t('startup.err.no_response'));
        if (r && r.ok) ok++;
        else {
          const s = list.find((x) => x.key1 === n);
          errors.push(`${s?.name || n}: ${r?.error || t('startup.err.no_response')}`);
        }
      }
      this.#applyServices(apply);
    } catch (e) {
      errors.push(errText(e));
      for (const n of names) results.set(n, errText(e));
    } finally {
      for (const id of ids) this.pending.delete(id);
    }
    if (quiet) return results;
    if (list.length === 1 && errors.length <= 1) {
      const name = list[0].name;
      const start = action === 'start';
      if (errors.length) toasts.error(t(start ? 'startup.svc.toast.start_failed' : 'startup.svc.toast.stop_failed', { name }), errors);
      else toasts.success(t(start ? 'startup.svc.toast.started' : 'startup.svc.toast.stopped', { name }));
    } else summarize(verb, t('startup.unit.services'), ok, errors);
    return results;
  }

  // ---------- khác ----------
  async setRestartApps(on) {
    const prev = this.settings.restart_apps;
    this.settings.restart_apps = on;
    this.restartPending = true;
    try {
      await api.setRestartApps(on);
      toasts.success(t(on ? 'startup.toast.restart_apps_on' : 'startup.toast.restart_apps_off'));
    } catch (e) {
      this.settings.restart_apps = prev;
      toasts.error(t('startup.err.setting'), [errText(e)]);
    } finally {
      this.restartPending = false;
    }
  }

  async openLocation(item) {
    try { await api.openLocation(item); } catch (e) { toasts.error(t('startup.err.open_location'), [errText(e)]); }
  }

  async openLog() {
    try { await api.openLog(); } catch (e) { toasts.error(t('startup.err.open_log'), [errText(e)]); }
  }

  /** opts: { title, message, okLabel, danger, checkLabel? }. Có checkLabel thì trả về { ok, checked }. */
  confirm(opts) {
    return new Promise((resolve) => {
      this.confirmState = {
        ...opts,
        checked: false,
        resolve: (v) => {
          const checked = !!this.confirmState?.checked;
          this.confirmState = null;
          resolve(opts.checkLabel ? { ok: v, checked } : v);
        },
      };
    });
  }

  /** Mở trang Khởi động / Dịch vụ và chọn sẵn một mục (từ Tổng quan, ngăn chi tiết…). */
  reveal(list, id) {
    this.focus = { list, id };
    this.page = list;
  }
}

export const store = new Store();

// Store dùng chung (một bản duy nhất): sửa file khi dev -> tải lại cả trang, tránh hai bản cũ / mới cùng chạy sau HMR.
if (import.meta.hot) import.meta.hot.decline();
