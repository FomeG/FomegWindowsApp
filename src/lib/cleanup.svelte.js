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
import { procs } from './procs.svelte.js';

/** Nhóm cache nhỏ hơn mức này không hiện (cùng mức với CACHE_MIN của backend). */
export const CACHE_MIN = 5 * 1048576;
export const GROUP_LABELS = labels({ temp: 'cleanup.group.temp', system: 'cleanup.group.system', cache: 'cleanup.group.cache', dev: 'cleanup.group.dev', other: 'cleanup.group.other' });
/** Nhóm tìm được theo tên thư mục (ứng dụng / công cụ lập trình / thư mục khác): ẩn khi quá nhỏ, dọn từng phần được. */
export const FOUND = new Set(['cache', 'dev', 'other']);
/** Nhóm cache của ứng dụng gồm những gì (backend `kinds`). */
export const PART_LABELS = labels({ cache: 'cleanup.part.cache', log: 'cleanup.part.log', crash: 'cleanup.part.crash' });
export const partsText = (it) => (it.kinds || []).map((k) => PART_LABELS[k] || k).join(' · ');
/** SAFETY[x][0] = nhãn (theo ngôn ngữ, getter), SAFETY[x][1] = lớp CSS */
const safety = (key, cls) => ({ get 0() { return t(key); }, 1: cls });
export const SAFETY = { safe: safety('cleanup.safety.safe', 'ok'), caution: safety('cleanup.safety.caution', 'warn') };

class Cleanup {
  scanId = $state(0);
  items = $state.raw([]);
  admin = $state(true);
  systemDrive = $state(null);
  driveRevision = $state(0);
  tookMs = $state(0);
  /** '' | 'scan' | 'clean' */
  phase = $state('');
  error = $state('');
  chosen = new SvelteSet();
  /** Mục chỉ dọn MỘT PHẦN: id -> Set chỉ số thư mục gốc (trong `paths`). Mục có ở đây cũng có trong `chosen`. */
  parts = $state.raw(new Map());
  keepRecent = $state(loadPref('cleanup:keepRecent', true));
  /** Kết quả lần dọn gần nhất của từng mục trong lần quét hiện tại. */
  results = $state.raw(new Map());
  /** Tiến độ dọn cho thanh dưới trang (CleanupProgress): sự kiện `cleanup_progress` của backend (~4 lần/giây, chỉ trong
   *  lúc dọn), rồi { stage: 'done', … } vài giây. null = ẩn. Một object thay mới mỗi lần, không có mảng lớn. */
  progress = $state.raw(null);
  #hideTimer = 0;
  #touched = new Set();
  #unlisten = null;

  /** Mục hiện trên trang (bỏ nhóm cache quá nhỏ). */
  get visible() {
    return this.items.filter((it) => !FOUND.has(it.group) || it.pending || it.bytes >= CACHE_MIN);
  }
  get pendingCount() {
    return this.items.reduce((n, it) => n + (it.pending ? 1 : 0), 0);
  }
  get chosenItems() {
    return this.visible.filter((it) => this.chosen.has(it.id));
  }
  get chosenSize() {
    const items = this.chosenItems;
    const bytes = items.reduce((n, it) => n + this.chosenFreeable(it), 0);
    if (!items.some((it) => it.pending)) return fmtBytes(bytes);
    return bytes ? `≥${fmtBytes(bytes)}` : '–';
  }
  #retained(it, keep = this.keepRecent) {
    const last = this.results.get(it.id);
    const skipped = last?.skipped_bytes ?? 0;
    // Tệp vừa mới vừa bị khoá chỉ tính vào phần bỏ qua ở lần dọn trước.
    const overlap = Math.min(it.recent_bytes, skipped, last?.skipped_recent_bytes ?? 0);
    const recent = keep ? it.recent_bytes - overlap : 0;
    return { recent, blocked: Math.min(Math.max(0, it.bytes - recent), skipped) };
  }
  /** Ước tính từ lần đo gần nhất và các tệp không xoá được ở lần dọn trước.
   *  `keep` mặc định theo cài đặt chung; popup AI truyền giá trị ô "giữ tệp mới" của nó. */
  freeable(it, keep = this.keepRecent) {
    if (it.pending) return 0;
    const { recent, blocked } = this.#retained(it, keep);
    return Math.max(0, it.bytes - recent - blocked);
  }
  /** Phần dọn được của các thư mục đang chọn (cả mục: như `freeable`). */
  chosenFreeable(it, keep = this.keepRecent) {
    const sel = this.parts.get(it.id);
    if (!sel) return this.chosen.has(it.id) ? this.freeable(it, keep) : 0;
    return [...sel].reduce((n, k) => n + this.partFreeable(it, k, keep), 0);
  }
  /** Phần dọn được của một thư mục gốc (ước tính: bỏ tệp mới khi "giữ tệp mới"). */
  partFreeable(it, k, keep = this.keepRecent) {
    const p = it.parts?.[k];
    if (!p || it.pending) return 0;
    return Math.max(0, p.bytes - (keep ? p.recent_bytes : 0));
  }
  /** Chỉ số các thư mục gốc đang chọn của mục. */
  selected(it) {
    const sel = this.parts.get(it.id);
    if (sel) return sel;
    return this.chosen.has(it.id) ? new Set(it.paths.map((_, k) => k)) : new Set();
  }
  /** Chọn đúng các thư mục `ks` của mục (rỗng = bỏ chọn, đủ = cả mục). */
  setParts(it, ks) {
    this.#touched.add(it.id);
    const next = new Map(this.parts);
    next.delete(it.id);
    if (!ks.size) this.chosen.delete(it.id);
    else {
      this.chosen.add(it.id);
      if (ks.size < it.paths.length) next.set(it.id, new Set(ks));
    }
    this.parts = next;
  }
  /** `{ id: [chỉ số] }` cho các mục chọn một phần trong `ids` (gửi kèm cleanup_apply). */
  partsFor(ids) {
    const out = {};
    for (const id of ids) {
      const sel = this.parts.get(id);
      if (sel) out[id] = [...sel];
    }
    return out;
  }
  retainedDetails(it) {
    if (it.pending) return [];
    const { recent, blocked } = this.#retained(it);
    return [
      recent || blocked ? t('cleanup.size.total', { bytes: fmtBytes(it.bytes) }) : '',
      recent ? t('cleanup.size.recent', { bytes: fmtBytes(recent) }) : '',
      blocked ? t('cleanup.size.blocked', { bytes: fmtBytes(blocked) }) : '',
    ].filter(Boolean);
  }

  setKeepRecent(v) {
    this.keepRecent = v;
    savePref('cleanup:keepRecent', v);
  }
  toggle(id, on) {
    this.#touched.add(id);
    if (on) this.chosen.add(id);
    else this.chosen.delete(id);
    if (this.parts.has(id)) {
      const next = new Map(this.parts);
      next.delete(id);
      this.parts = next;
    }
  }
  /** Bỏ các lựa chọn một phần (quét lại / vừa dọn: chỉ số thư mục có thể đã đổi). */
  #dropParts(ids = [...this.parts.keys()]) {
    if (!ids.some((id) => this.parts.has(id))) return;
    const next = new Map(this.parts);
    for (const id of ids) if (next.delete(id)) this.chosen.delete(id);
    this.parts = next;
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
      this.systemDrive = r.system_drive;
      this.tookMs = r.took_ms;
      this.#dropParts();
      this.items = r.items;
      // icon thật của ứng dụng (exe / logo Store), lấy một lần, backend giữ bản nhớ
      procs.wantIcons(r.items.map((it) => it.icon).filter(Boolean));
      this.results = new Map();
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
    if (i < 0 || !this.items[i].pending) return;
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

  /** Dọn các mục `ids` của lần quét hiện tại (mục chọn một phần: chỉ phần đã chọn, xem `parts`). Trả Map id -> kết quả. */
  async clean(ids, keepRecent = this.keepRecent, parts = this.partsFor(ids)) {
    if (this.phase || !ids.length) return new Map();
    this.phase = 'clean';
    let out = new Map();
    clearTimeout(this.#hideTimer);
    const want = new Set(ids);
    const picked = this.items.filter((x) => want.has(x.id) && x.id !== 'recycle');
    // hiện thanh ngay (backend gửi bước đầu sau vài ms)
    const pf = (x, f) => (parts[x.id] ? parts[x.id].reduce((n, k) => n + (x.parts?.[k]?.[f] || 0), 0) : x[f]);
    this.progress = { stage: 'delete', done_files: 0, total_files: picked.reduce((n, x) => n + pf(x, 'files'), 0), freed_bytes: 0, total_bytes: picked.reduce((n, x) => n + pf(x, 'bytes'), 0), item: picked.length === 1 ? picked[0].name : '', recycle: want.has('recycle'), elapsed_ms: 0 };
    let unlisten = null;
    try {
      unlisten = await api.listen('cleanup_progress', (p) => { if (this.phase === 'clean') this.progress = p; });
    } catch {}
    try {
      const r = await api.cleanupApply(this.scanId, ids, keepRecent, parts);
      this.#dropParts(Object.keys(parts));
      this.driveRevision += 1;
      out = new Map(r.results.map((x) => [x.id, x]));
      const fresh = new Map((r.items || []).map((x) => [x.id, x]));
      if (fresh.size) {
        this.items = this.items.map((x) => fresh.get(x.id) || x);
        const results = new Map(this.results);
        for (const id of fresh.keys()) {
          if (out.has(id)) results.set(id, out.get(id));
          else results.delete(id);
        }
        this.results = results;
      }
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
