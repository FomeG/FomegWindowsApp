// Trạng thái trang Dọn dẹp (giữ khi chuyển trang: không quét lại mỗi lần mở).
//
// - scan(): backend trả kết quả sau ~2 s; mục còn đang đo (`pending`) đến sau qua sự kiện 'cleanup_item'.
// - chosen: mã các mục sẽ dọn. Mục chưa được người dùng đụng tới theo `default_on` của backend; mục đã bấm thì giữ
//   lựa chọn qua các lần quét lại.
// - clean(ids): dọn; backend đo lại CHỈ các mục vừa dọn và trả về (không quét lại cả máy). Dùng chung cho nút "Dọn" của trang và popup đề xuất của trợ lý (reviewKinds.js).
import { SvelteSet } from 'svelte/reactivity';
import * as api from './api.js';
import { toasts } from './toasts.svelte.js';
import { loadPref, savePref } from './prefs.svelte.js';
import { errText, fmtBytes, fmtInt } from './format.js';
import { t, labels } from './i18n.svelte.js';

/** Nhóm cache nhỏ hơn mức này không hiện (cùng mức với CACHE_MIN của backend). */
export const CACHE_MIN = 5 * 1048576;
export const GROUP_LABELS = labels({ temp: 'cleanup.group.temp', system: 'cleanup.group.system', cache: 'cleanup.group.cache' });
/** SAFETY[x][0] = nhãn (theo ngôn ngữ, getter), SAFETY[x][1] = lớp CSS */
const safety = (key, cls) => ({ get 0() { return t(key); }, 1: cls });
export const SAFETY = { safe: safety('cleanup.safety.safe', 'ok'), caution: safety('cleanup.safety.caution', 'warn') };

class Cleanup {
  scanId = $state(0);
  items = $state.raw([]);
  admin = $state(true);
  tookMs = $state(0);
  /** '' | 'scan' | 'clean' */
  phase = $state('');
  error = $state('');
  chosen = new SvelteSet();
  keepRecent = $state(loadPref('cleanup:keepRecent', true));
  /** kết quả lần dọn gần nhất theo mã mục */
  results = $state.raw(new Map());
  /** Tiến độ dọn cho thanh dưới trang (CleanupProgress): sự kiện `cleanup_progress` của backend (~4 lần/giây, chỉ trong
   *  lúc dọn), rồi { stage: 'done', … } vài giây. null = ẩn. Một object thay mới mỗi lần, không có mảng lớn. */
  progress = $state.raw(null);
  #hideTimer = 0;
  #touched = new Set();
  #unlisten = null;

  /** Mục hiện trên trang (bỏ nhóm cache quá nhỏ). */
  get visible() {
    return this.items.filter((it) => it.group !== 'cache' || it.pending || it.bytes >= CACHE_MIN);
  }
  get pendingCount() {
    return this.items.reduce((n, it) => n + (it.pending ? 1 : 0), 0);
  }
  get chosenItems() {
    return this.visible.filter((it) => this.chosen.has(it.id));
  }
  /** Dung lượng sẽ giải phóng ước tính (trừ phần mới nếu đang giữ tệp mới). */
  freeable(it) {
    return Math.max(0, it.bytes - (this.keepRecent ? it.recent_bytes : 0));
  }

  setKeepRecent(v) {
    this.keepRecent = v;
    savePref('cleanup:keepRecent', v);
  }
  toggle(id, on) {
    this.#touched.add(id);
    if (on) this.chosen.add(id);
    else this.chosen.delete(id);
  }
  setMany(ids, on) {
    for (const id of ids) this.toggle(id, on);
  }

  /** Lần đầu mở trang: quét. */
  ensure() {
    if (!this.scanId && !this.phase && !this.error) this.scan();
  }

  async scan() {
    if (this.phase) return;
    this.phase = 'scan';
    this.error = '';
    try {
      if (!this.#unlisten) this.#unlisten = await api.listen('cleanup_item', (e) => this.#update(e));
      const r = await api.cleanupScan();
      this.scanId = r.id;
      this.admin = r.admin;
      this.tookMs = r.took_ms;
      this.items = r.items;
      this.#sync();
    } catch (e) {
      this.error = errText(e);
    } finally {
      this.phase = '';
    }
  }

  #update({ scan, item }) {
    if (scan !== this.scanId) return;
    const i = this.items.findIndex((x) => x.id === item.id);
    if (i < 0) return;
    const a = this.items.slice();
    a[i] = item;
    this.items = a;
    this.#sync();
  }

  // chọn sẵn theo backend cho mục chưa đụng tới; bỏ chọn mục lỗi / đã biến mất / quá nhỏ để hiện
  #sync() {
    const vis = new Set(this.visible.map((it) => it.id));
    for (const it of this.items) {
      if (it.error && !it.paths.length && it.id !== 'recycle') this.chosen.delete(it.id);
      else if (!this.#touched.has(it.id)) it.default_on ? this.chosen.add(it.id) : this.chosen.delete(it.id);
    }
    for (const id of [...this.chosen]) if (!vis.has(id)) this.chosen.delete(id);
  }

  /** Dọn các mục `ids` của lần quét hiện tại. Trả Map id -> kết quả. */
  async clean(ids, keepRecent = this.keepRecent) {
    if (this.phase || !ids.length) return new Map();
    this.phase = 'clean';
    let out = new Map();
    clearTimeout(this.#hideTimer);
    const want = new Set(ids);
    const picked = this.items.filter((x) => want.has(x.id) && x.id !== 'recycle');
    // hiện thanh ngay (backend gửi bước đầu sau vài ms)
    this.progress = { stage: 'delete', done_files: 0, total_files: picked.reduce((n, x) => n + x.files, 0), freed_bytes: 0, total_bytes: picked.reduce((n, x) => n + x.bytes, 0), item: picked.length === 1 ? picked[0].name : '', recycle: want.has('recycle'), elapsed_ms: 0 };
    let unlisten = null;
    try {
      unlisten = await api.listen('cleanup_progress', (p) => { if (this.phase === 'clean') this.progress = p; });
    } catch {}
    try {
      const r = await api.cleanupApply(this.scanId, ids, keepRecent);
      out = new Map(r.results.map((x) => [x.id, x]));
      this.results = out;
      const fresh = new Map((r.items || []).map((x) => [x.id, x]));
      if (fresh.size) this.items = this.items.map((x) => fresh.get(x.id) || x);
      const removed = r.results.reduce((n, x) => n + x.removed, 0);
      const bytes = r.results.reduce((n, x) => n + x.bytes, 0);
      const skipped = r.results.reduce((n, x) => n + x.skipped + x.kept_recent, 0);
      const fails = r.results.filter((x) => x.error);
      this.progress = { stage: 'done', done_files: removed + skipped, removed, skipped, freed_bytes: bytes, elapsed_ms: r.took_ms };
      toasts.success(t('cleanup.toast.cleaned', { bytes: fmtBytes(bytes), count: removed, n: fmtInt(removed) }), skipped ? [t('cleanup.toast.skipped', { count: skipped, n: fmtInt(skipped) })] : []);
      if (fails.length) toasts.error(t('cleanup.toast.partial', { count: fails.length, n: fmtInt(fails.length) }), fails.slice(0, 5).map((x) => `${this.items.find((i) => i.id === x.id)?.name || x.id}: ${x.error}`));
    } catch (e) {
      this.progress = null;
      toasts.error(t('cleanup.toast.failed'), [errText(e)]);
    } finally {
      unlisten?.();
      this.phase = '';
      // kết quả ở lại vài giây rồi thanh tự ẩn
      if (this.progress) this.#hideTimer = setTimeout(() => (this.progress = null), 6000);
    }
    return out;
  }
}

export const cleanup = new Cleanup();

// Store dùng chung (một bản duy nhất): sửa file khi dev -> tải lại cả trang, tránh hai bản cũ / mới cùng chạy sau HMR.
if (import.meta.hot) import.meta.hot.decline();
