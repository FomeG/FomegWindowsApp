// Trạng thái trang Ứng dụng (giữ khi chuyển trang: không đọc lại mỗi lần mở).
//
// - load(): danh sách app thường + app Store (~0,1–0,3 s), rồi đo dung lượng thật các thư mục cài đặt ở nền
//   (sự kiện 'app_size', lớn trước, ghi nhớ 10 phút ở backend). Rời trang = dừng đo.
// - sizes: Map đường dẫn viết thường -> { bytes, files, done } + sizeV (đếm phiên bản, tránh tạo Map mới mỗi sự kiện).
// - uninstall(app): xác nhận kiểu Windows, chạy trình gỡ của chính app; backend báo 'app_uninstall' khi trình gỡ
//   (và mọi tiến trình con) đã thoát, kèm app còn trong danh sách hay không.
import * as api from './api.js';
import { store } from './store.svelte.js';
import { toasts } from './toasts.svelte.js';
import { procs } from './procs.svelte.js';
import { ai } from './ai.svelte.js';
import { prefs } from './prefs.svelte.js';
import { SvelteSet } from 'svelte/reactivity';
import { errText, fmtBytes, fmtInt } from './format.js';
import { t, labels } from './i18n.svelte.js';

export const KIND_LABELS = labels({ win32: 'apps.kind.win32', store: 'apps.kind.store' });
export const SCOPE_LABELS = labels({ machine: 'common.scope.machine', user: 'apps.scope.user' });

// lỗi "không còn tồn tại" của backend (vi hoặc en) = thư mục đã tự mất, coi như dọn xong
// (khớp lỏng bằng "." thay cho chữ có dấu: đây là chuỗi so với backend, không phải chữ hiển thị)
const GONE = /kh.ng c.n t.n t.i|no longer exists/i;
/** Dữ liệu app Store / WindowsApps: Windows tự quản, không "bỏ qua" theo tên app (không so tên nơi đã dịch). */
export const isStoreArea = (f) => f.area === 'WindowsApps' || /\\AppData\\Local\\Packages\\/i.test(f.path || '');

class Apps {
  list = $state.raw([]);
  loaded = $state(false);
  loading = $state(false);
  error = $state('');
  storeError = $state('');
  /** đọc được lịch sử chạy đầy đủ (BAM / Prefetch, cần Admin) */
  usageFull = $state(false);
  tookMs = $state(0);
  sizes = new Map();
  sizeV = $state(0);
  /** đang đo (số thư mục còn lại) */
  measuring = $state(0);
  /** id app đang gỡ */
  removing = $state('');
  /** thư mục liên quan theo id app: null = đang tìm; relV tăng khi có kết quả */
  related = new Map();
  relV = $state(0);
  /** thư mục sẽ dọn khi app gỡ xong: id -> [{ path, bytes }] */
  #pending = new Map();
  // phần còn sót (tab "Còn sót lại")
  left = $state.raw(null);
  leftLoading = $state(false);
  leftError = $state('');
  leftInfo = $state.raw({ took_ms: 0, removed_known: 0, windowsapps_error: null });
  leftChosen = new SvelteSet();
  leftBusy = $state(false);
  #gen = 0;
  #unlisten = [];

  /** Dung lượng hiển thị: đo được > khai báo. { bytes, measured, partial } | null */
  size(a) {
    this.sizeV;
    const m = a.location ? this.sizes.get(a.location.toLowerCase()) : null;
    if (m) return { bytes: m.bytes, measured: true, partial: !m.done, files: m.files };
    return a.est_bytes ? { bytes: a.est_bytes, measured: false } : null;
  }

  /** Mở trang: lần đầu đọc đầy đủ; các lần sau làm mới NGẦM (backend trả bản nhớ trong ~1 ms nếu chưa cài / gỡ gì,
   *  có thay đổi thì đọc lại) -> vừa hiện ngay vừa không bao giờ cũ. */
  ensure() {
    if (!this.loaded) {
      if (!this.loading) this.load();
    } else this.load({ quiet: true });
  }

  // nhiều sự kiện đo đến dồn dập (lúc đầu ~300): gom lại, bảng sắp xếp / vẽ lại tối đa ~8 lần mỗi giây
  #bump = 0;
  #sizesChanged() {
    if (!this.#bump) this.#bump = setTimeout(() => { this.#bump = 0; this.sizeV++; }, 120);
  }

  async #listen() {
    if (this.#unlisten.length) return;
    this.#unlisten = await Promise.all([
      api.listen('app_size', (e) => {
        if (e.gen !== this.#gen) return;
        this.sizes.set(e.path.toLowerCase(), { bytes: e.bytes, files: e.files, done: e.done });
        this.measuring = Math.max(0, this.measuring - 1);
        this.#sizesChanged();
      }),
      api.listen('app_uninstall', (r) => this.#uninstalled(r)),
    ]);
  }

  /** force = bấm Làm mới (đọc lại + đo lại hết); quiet = làm mới ngầm khi mở lại trang (không hiện "đang đọc"). */
  async load({ force = false, quiet = false } = {}) {
    if (this.loading) return;
    if (!quiet) this.loading = true;
    this.error = '';
    if (force) {
      this.sizes.clear();
      this.related.clear();
      this.#relAt.clear();
      this.sizeV++;
    }
    try {
      await this.#listen();
      const r = await api.appsList(force);
      this.list = r.apps;
      this.storeError = r.store_error || '';
      this.usageFull = !!r.usage_full;
      this.tookMs = r.took_ms;
      this.loaded = true;
      procs.wantIcons(r.apps.map((a) => a.icon).filter(Boolean));
      // tắt "tự đo" trong Cài đặt: chỉ đo khi bấm Làm mới (force)
      if (!this.measuring && (prefs.appsMeasure || force)) this.#measure(!quiet);
    } catch (e) {
      this.error = errText(e);
    } finally {
      this.loading = false;
    }
  }

  /** Đo các thư mục chưa có kết quả (all = đo lại hết). Lớn (theo khai báo) trước. */
  #measure(all = false) {
    const seen = new Set();
    const paths = this.list
      .filter((a) => a.location && (all || !this.sizes.get(a.location.toLowerCase())?.done))
      .sort((a, b) => b.est_bytes - a.est_bytes)
      .map((a) => a.location)
      .filter((p) => !seen.has(p.toLowerCase()) && seen.add(p.toLowerCase()));
    if (!paths.length) return;
    const gen = ++this.#gen;
    this.measuring = paths.length;
    api.appsMeasure(gen, paths).catch(() => {}).finally(() => { if (gen === this.#gen) this.measuring = 0; });
  }

  /** Rời trang: dừng đo (kết quả đã có vẫn giữ). */
  pause() {
    if (!this.measuring) return;
    this.#gen++;
    this.measuring = 0;
    api.appsMeasureStop().catch(() => {});
  }

  /** Popup gỡ cài đặt (ActionReview, loại go_cai): liệt kê thư mục liên quan để chọn dọn sau khi gỡ. */
  uninstall(a) {
    if (!a) return;
    if (!a.can_uninstall) {
      toasts.error(t('apps.toast.cant_uninstall_here', { name: a.name }), [a.system ? t('apps.toast.system_app') : t('apps.toast.no_remove')]);
      return;
    }
    if (this.removing) {
      const cur = this.list.find((x) => x.id === this.removing)?.name || t('apps.another_app');
      toasts.error(t('apps.toast.waiting_uninstaller', { name: cur }), [t('apps.toast.waiting_uninstaller_hint')]);
      return;
    }
    ai.review = { kind: 'go_cai', app: a, items: [{ key: a.id, label: t('apps.related.searching_label'), reason: '', app: a }] };
  }

  /** Chạy trình gỡ; `folders` vào Thùng rác khi gỡ xong thật. Lỗi -> ném ra (popup báo). */
  async start(a, folders = []) {
    if (this.removing) throw t('apps.err.busy');
    this.removing = a.id;
    try {
      await api.appsUninstall(a.id);
      this.#pending.set(a.id, folders);
    } catch (e) {
      this.removing = '';
      throw e;
    }
  }

  #uninstalled(r) {
    if (this.removing === r.id) this.removing = '';
    const folders = this.#pending.get(r.id) || [];
    this.#pending.delete(r.id);
    if (r.removed) {
      const a = this.list.find((x) => x.id === r.id);
      this.list = this.list.filter((x) => x.id !== r.id);
      if (a?.location) this.sizes.delete(a.location.toLowerCase());
      this.related.delete(r.id);
      this.sizeV++;
      if (folders.length) this.#cleanAfter(r.name, folders);
      else toasts.success(t('apps.toast.uninstalled', { name: r.name }), [t('apps.toast.see_leftovers')]);
      this.left = null; // lần mở tab sau quét lại
    } else if (r.error) {
      toasts.error(t('apps.toast.uninstall_failed', { name: r.name }), [r.error]);
    } else {
      toasts.error(t('apps.toast.still_installed', { name: r.name }), [t('apps.toast.still_installed_hint')]);
    }
  }

  /** Gỡ xong: chuyển các thư mục đã chọn vào Thùng rác (thư mục trình gỡ đã tự xoá thì coi như xong). */
  async #cleanAfter(name, folders) {
    try {
      const res = await api.duDeletePaths(folders.map((f) => f.path), false);
      const gone = (x) => x.ok || GONE.test(x.error || '');
      const fail = res.filter((x) => !gone(x));
      const okPaths = new Set(res.filter(gone).map((x) => x.path.toLowerCase()));
      const bytes = folders.filter((f) => okPaths.has(f.path.toLowerCase())).reduce((n, f) => n + f.bytes, 0);
      toasts.success(t('apps.toast.uninstalled', { name }), [t('apps.toast.cleaned_related', { n: fmtInt(folders.length - fail.length), count: folders.length - fail.length, size: fmtBytes(bytes) })]);
      if (fail.length) toasts.error(t('apps.toast.folders_failed', { n: fmtInt(fail.length), count: fail.length }), fail.slice(0, 4).map((x) => `${x.path}: ${x.error}`));
    } catch (e) {
      toasts.error(t('apps.toast.related_failed', { name }), [errText(e)]);
    }
  }

  /** Thư mục liên quan cho ngăn chi tiết. Nhớ 2 phút mỗi app (backend còn nhớ số đo 10 phút, lần sau ~1 ms);
   *  quá hạn thì giữ kết quả cũ trên màn hình trong lúc lấy bản mới. */
  #relAt = new Map();
  async loadRelated(a, force = false) {
    if (!a) return;
    const at = this.#relAt.get(a.id);
    if (!force && (this.related.has(a.id) && (at === undefined || Date.now() - at < 120_000))) return;
    this.#relAt.set(a.id, Date.now());
    if (!this.related.get(a.id)) {
      this.related.set(a.id, null);
      this.relV++;
    }
    let items = [];
    try { items = await api.appsRelated(a.id); } catch {}
    this.related.set(a.id, items);
    this.relV++;
  }

  // ---------- phần còn sót ----------
  async scanLeft() {
    if (this.leftLoading) return;
    this.leftLoading = true;
    this.leftError = '';
    try {
      const r = await api.leftoversScan();
      this.left = r.items;
      this.leftInfo = r;
      this.leftChosen.clear();
      for (const f of r.items) if (f.sure && f.kind !== 'info') this.leftChosen.add(leftId(f));
    } catch (e) {
      this.leftError = errText(e);
    } finally {
      this.leftLoading = false;
    }
  }

  /** Dọn các mục đã chọn: thư mục -> Thùng rác, gói WindowsApps của tài khoản khác -> gỡ qua Windows. */
  async cleanLeft() {
    const list = (this.left || []).filter((f) => this.leftChosen.has(leftId(f)) && f.kind !== 'info');
    if (!list.length || this.leftBusy) return;
    const folders = list.filter((f) => f.kind === 'folder');
    const pkgs = list.filter((f) => f.kind === 'package');
    const bytes = list.reduce((n, f) => n + f.bytes, 0);
    const ok = await store.confirm({
      title: t('apps.left.confirm.title', { n: fmtInt(list.length), count: list.length }),
      message: [
        folders.length ? t('apps.left.confirm.folders', { n: fmtInt(folders.length), count: folders.length }) : '',
        pkgs.length ? t('apps.left.confirm.packages', { n: fmtInt(pkgs.length), count: pkgs.length }) : '',
        t('apps.left.confirm.total', { size: fmtBytes(bytes) }),
      ].filter(Boolean).join(' '),
      okLabel: t('apps.left.clean'),
      danger: true,
    });
    if (!ok) return;
    this.leftBusy = true;
    const done = new Set();
    const errs = [];
    try {
      if (folders.length) {
        for (const x of await api.duDeletePaths(folders.map((f) => f.path), false)) {
          if (x.ok || GONE.test(x.error || '')) done.add(x.path.toLowerCase());
          else errs.push(`${x.path}: ${x.error}`);
        }
      }
      if (pkgs.length) {
        for (const x of await api.leftoversRemovePackages(pkgs.map((f) => f.package))) {
          if (x.ok) done.add(x.name.toLowerCase());
          else errs.push(`${x.name}: ${x.error}`);
        }
      }
    } catch (e) {
      errs.push(errText(e));
    } finally {
      this.leftBusy = false;
    }
    const freed = list.filter((f) => done.has(leftId(f).toLowerCase())).reduce((n, f) => n + f.bytes, 0);
    this.left = this.left.filter((f) => !done.has(leftId(f).toLowerCase()));
    for (const id of [...this.leftChosen]) if (done.has(id.toLowerCase())) this.leftChosen.delete(id);
    if (done.size) toasts.success(t('apps.left.toast.cleaned', { n: fmtInt(done.size), count: done.size, size: fmtBytes(freed) }), folders.length ? [t('apps.left.toast.in_recycle')] : []);
    if (errs.length) toasts.error(t('apps.left.toast.failed', { n: fmtInt(errs.length), count: errs.length }), errs.slice(0, 5));
  }

  /** Không nhắc phần còn sót của một app đã gỡ nữa. */
  async forget(owner) {
    try {
      await api.leftoversForget(owner);
      this.left = (this.left || []).filter((f) => f.owner !== owner || isStoreArea(f));
    } catch (e) {
      toasts.error(t('apps.left.toast.forget_failed'), [errText(e)]);
    }
  }

  async modify(a) {
    try { await api.appsModify(a.id); } catch (e) { toasts.error(t('apps.toast.modify_failed', { name: a.name }), [errText(e)]); }
  }
  async open(a, what) {
    try { await api.appsOpen(a.id, what); } catch (e) { toasts.error(what === 'registry' ? t('apps.toast.open_registry_failed') : t('apps.toast.open_folder_failed'), [errText(e)]); }
  }
}

const DAY = 864e5;
/** Lâu không dùng: lần dùng cuối > N ngày (Cài đặt, mặc định 90), hoặc (có dữ liệu đầy đủ) chưa thấy dùng và đã cài > N ngày. */
export const isStale = (a, full, now = Date.now(), days = prefs.staleDays) =>
  a.last_used > 0 ? now - a.last_used > days * DAY : full && !a.launches && a.installed > 0 && now - a.installed > days * DAY;
/** "hôm nay", "3 ngày trước", "5 tháng trước"… */
export function ago(ms, now = Date.now()) {
  if (!ms) return '';
  const d = Math.max(0, Math.floor((now - ms) / DAY));
  if (d === 0) return t('apps.ago.today');
  if (d === 1) return t('apps.ago.yesterday');
  if (d < 60) return t('apps.ago.days', { count: d });
  if (d < 730) return t('apps.ago.months', { count: Math.floor(d / 30) });
  return t('apps.ago.years', { count: Math.floor(d / 365) });
}

/** Khoá một mục còn sót: tên gói (gói WindowsApps) hoặc đường dẫn. */
export const leftId = (f) => f.package || f.path;

export const apps = new Apps();

// Store dùng chung (một bản duy nhất): sửa file khi dev -> tải lại cả trang, tránh hai bản cũ / mới cùng chạy sau HMR.
if (import.meta.hot) import.meta.hot.decline();
