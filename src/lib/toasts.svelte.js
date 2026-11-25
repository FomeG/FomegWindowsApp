// Thông báo dạng toast (góc dưới phải). Lỗi hiển thị lâu hơn và có thể đóng.
import { t } from './i18n.svelte.js';

let seq = 0;

class Toasts {
  list = $state([]);

  push({ kind = 'info', title, details = [], timeout }) {
    const id = ++seq;
    const ms = timeout ?? (kind === 'error' ? 12000 : kind === 'warn' ? 8000 : 3500);
    this.list.push({ id, kind, title, details });
    if (this.list.length > 6) this.list.splice(0, this.list.length - 6);
    if (ms > 0) setTimeout(() => this.dismiss(id), ms);
    return id;
  }
  dismiss(id) {
    const i = this.list.findIndex((t) => t.id === id);
    if (i >= 0) this.list.splice(i, 1);
  }
  success(title, details) { return this.push({ kind: 'success', title, details }); }
  error(title, details) { return this.push({ kind: 'error', title, details }); }
  warn(title, details) { return this.push({ kind: 'warn', title, details }); }
  info(title, details) { return this.push({ kind: 'info', title, details }); }
}

export const toasts = new Toasts();

/** Tóm tắt kết quả hàng loạt: "Tắt: 5 mục thành công, 1 lỗi" */
export function summarize(verb, unit, ok, errors) {
  const parts = [];
  if (ok) parts.push(t('shell.bulk.ok', { n: ok, unit }));
  if (errors.length) parts.push(t('shell.bulk.errors', { count: errors.length }));
  if (!parts.length) parts.push(t('shell.bulk.none'));
  const title = t('shell.bulk.title', { verb, parts: parts.join(', ') });
  if (errors.length) toasts.push({ kind: ok ? 'warn' : 'error', title, details: errors, timeout: 15000 });
  else toasts.success(title);
}

// Store dùng chung (một bản duy nhất): sửa file khi dev -> tải lại cả trang, tránh hai bản cũ / mới cùng chạy sau HMR.
if (import.meta.hot) import.meta.hot.decline();
