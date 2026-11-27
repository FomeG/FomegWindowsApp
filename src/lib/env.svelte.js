// Dữ liệu trang Biến môi trường (Hệ thống → Biến môi trường), giữ khi chuyển trang.
// Backend đọc registry ~1–5 ms nên mỗi lần mở trang / sau mỗi lần ghi đều đọc lại (luôn mới, không cần cache).
import * as api from './api.js';
import { toasts } from './toasts.svelte.js';
import { errText } from './format.js';
import { t, labels } from './i18n.svelte.js';

export const SCOPE_LABELS = labels({ user: 'env.scope.user', system: 'env.scope.system' });
export const varId = (scope, name) => `${scope}:${name.toLowerCase()}`;
/** Khoá so sánh mục danh sách (giống backend): bản mở rộng, không phân biệt hoa thường, bỏ "\" cuối. */
export const entryKey = (s) => (s || '').trim().replace(/[\\/]+$/, '').toLowerCase();
/** Mục có vấn đề: thư mục không còn, trùng mục trước, hay đã có trong biến hệ thống. */
export const badEntry = (e) => e.exists === false || e.dup || e.other;

class EnvStore {
  vars = $state.raw([]);
  history = $state.raw([]);
  canSystem = $state(true);
  loaded = $state(false);
  loading = $state(false);
  error = $state('');
  tookMs = $state(0);
  /** đang ghi (khoá các nút Lưu / Xoá) */
  saving = $state(false);
  /** mở ngăn chi tiết ở chế độ thêm biến mới */
  adding = $state(false);
  /** id biến cần chọn sau khi tải lại (vừa thêm / mở từ nơi khác) */
  reveal = $state(null);

  async load() {
    this.loading = true;
    try {
      const r = await api.envList();
      this.vars = (r?.vars || []).map((v) => ({ ...v, id: varId(v.scope, v.name) }));
      this.history = (r?.history || []).slice().reverse();
      this.canSystem = !!r?.can_system;
      this.tookMs = r?.took_ms || 0;
      this.error = '';
      this.loaded = true;
    } catch (e) {
      this.error = errText(e);
    } finally {
      this.loading = false;
    }
  }

  ensure() {
    return this.loaded || this.loading ? Promise.resolve() : this.load();
  }

  find(scope, name) {
    const id = varId(scope, name);
    return this.vars.find((v) => v.id === id) || null;
  }

  get issueCount() {
    return this.vars.reduce((n, v) => n + v.issues, 0);
  }

  /**
   * Ghi (value = chuỗi) hoặc xoá (value = null) một biến rồi tải lại.
   * quiet: không báo toast (popup đề xuất AI tự báo một lần). Trả Change | null; lỗi -> ném lại.
   */
  async set(scope, name, value, { quiet = false } = {}) {
    this.saving = true;
    try {
      const c = await api.envSet(scope, name, value);
      if (!quiet) {
        if (!c) toasts.success(t('env.toast.same'), []);
        else toasts.success(value == null ? t('env.toast.deleted', { name }) : t('env.toast.saved', { name }), [t('env.toast.restart_hint')]);
      }
      return c;
    } catch (e) {
      if (!quiet) toasts.error(t('env.toast.failed', { name }), [errText(e)]);
      throw e;
    } finally {
      this.saving = false;
      await this.load();
    }
  }

  /** Hoàn tác một thay đổi của lịch sử. */
  async restore(c) {
    this.saving = true;
    try {
      await api.envRestore(c.id);
      toasts.success(t('env.toast.restored', { name: c.name }), [t('env.toast.restart_hint')]);
    } catch (e) {
      toasts.error(t('env.toast.restore_failed', { name: c.name }), [errText(e)]);
    } finally {
      this.saving = false;
      await this.load();
    }
  }
}

export const env = new EnvStore();

if (import.meta.hot) import.meta.hot.decline();
