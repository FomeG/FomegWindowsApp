// Tự cập nhật (backend: src-tauri/src/update.rs, kế hoạch: docs/auto-update-plan.md).
// Kiểm tra 10 s sau khi mở app, rồi mỗi 6 giờ – chỉ khi cửa sổ đang hiện và người dùng bật "Tự kiểm tra".
// Bản tự build (không có repo / khoá do CI nhúng) -> info.enabled = false, không kiểm tra gì.
import * as api from './api.js';
import { prefs, loadPref, savePref } from './prefs.svelte.js';
import { toasts } from './toasts.svelte.js';
import { errText } from './format.js';
import { t } from './i18n.svelte.js';

const FIRST_MS = 10_000;
const EVERY_MS = 6 * 3600_000;

class Updater {
  /** { enabled, version, repo } */
  info = $state(null);
  /** bản mới đang báo: { version, current, notes, date_ms } | null */
  avail = $state(null);
  checking = $state(false);
  error = $state('');
  lastCheck = $state(0);
  /** idle | download | install */
  phase = $state('idle');
  got = $state(0);
  total = $state(0);
  /** "Để sau": ẩn dải thông báo tới lần mở app sau */
  hidden = $state(false);
  #timer = 0;
  #started = false;

  /** dải thông báo có hiện không */
  get show() {
    return !!this.avail && (this.phase !== 'idle' || (!this.hidden && this.avail.version !== loadPref('update:skip', '')));
  }

  async init() {
    if (this.#started) return;
    this.#started = true;
    try {
      this.info = await api.updateInfo();
    } catch {
      this.info = { enabled: false, version: '', repo: '' };
    }
    if (!this.info.enabled) return;
    api.listen('update_progress', (p) => {
      this.phase = p.stage;
      if (p.stage === 'download') {
        this.got = p.got;
        this.total = p.total;
      }
    }).catch(() => {});
    this.#schedule(FIRST_MS);
  }

  #schedule(ms) {
    clearTimeout(this.#timer);
    this.#timer = setTimeout(() => this.#tick(), ms);
  }

  #tick() {
    // cửa sổ ẩn / đã tắt tự kiểm tra: thử lại ở chu kỳ sau, không gọi mạng
    if (!document.hidden && prefs.updateAuto) this.check().catch(() => {});
    this.#schedule(EVERY_MS);
  }

  /** manual = bấm "Kiểm tra ngay": báo cả bản đã "Bỏ qua", báo lỗi / "đã là bản mới nhất" bằng toast. */
  async check({ manual = false } = {}) {
    if (!this.info?.enabled || this.checking || this.phase !== 'idle') return;
    this.checking = true;
    this.error = '';
    try {
      const a = await api.updateCheck();
      this.avail = a;
      this.lastCheck = Date.now();
      if (manual) {
        if (a) {
          this.hidden = false;
          if (a.version === loadPref('update:skip', '')) savePref('update:skip', '');
        } else {
          toasts.success(t('update.toast.latest', { v: this.info.version }));
        }
      }
    } catch (e) {
      this.error = errText(e);
      if (manual) toasts.error(t('update.toast.check_failed'), [this.error]);
    } finally {
      this.checking = false;
    }
  }

  later() {
    this.hidden = true;
  }

  skip() {
    if (this.avail) savePref('update:skip', this.avail.version);
    this.hidden = true;
  }

  async install() {
    if (!this.avail || this.phase !== 'idle') return;
    this.phase = 'download';
    this.got = 0;
    this.total = 0;
    try {
      // thành công: app tự đóng, bộ cài chạy rồi mở lại app -> không bao giờ quay lại đây
      await api.updateInstall();
    } catch (e) {
      this.phase = 'idle';
      toasts.error(t('update.toast.install_failed'), [errText(e)]);
    }
  }
}

export const updater = new Updater();

// Store dùng chung (một bản duy nhất): sửa file khi dev -> tải lại cả trang.
if (import.meta.hot) import.meta.hot.decline();
