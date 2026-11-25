import { mount } from 'svelte';
import './app.css';
import './v3.css';
import App from './App.svelte';
import { i18n } from './lib/i18n.svelte.js';

// Áp dụng theme trước khi mount để tránh nháy màu.
try {
  const t = localStorage.getItem('theme');
  if (t === 'light' || t === 'dark') document.documentElement.dataset.theme = t;
} catch {}

// Font icon của Windows (Segoe Fluent Icons / MDL2). Không có (trình duyệt ngoài Windows) -> ẩn glyph.
try {
  const g = document.createElement('canvas').getContext('2d');
  const w = (f) => { g.font = '32px ' + f; return g.measureText('').width; };
  if (w('"Segoe Fluent Icons", "Segoe MDL2 Assets", monospace') === w('monospace')) document.documentElement.classList.add('no-icon-font');
} catch {}

// Chặn menu chuột phải mặc định của WebView2 (Tải lại, In, Kiểm tra phần tử…) và phím mở công cụ nhà phát triển.
// Menu chuột phải của app (ContextMenu) vẫn chạy vì handler của từng thành phần vẫn nhận sự kiện.
// Bản release: ô nhập vẫn có menu Cắt / Sao chép / Dán (không có mục Kiểm tra vì bản release không kèm DevTools).
// Bản dev: giữ Alt khi chuột phải / bấm F12 để vẫn gỡ lỗi được.
const DEV = import.meta.env.DEV;
const editable = (el) => !!el?.closest?.('input, textarea, [contenteditable="true"]');
window.addEventListener(
  'contextmenu',
  (e) => {
    if (DEV && e.altKey) return;
    if (!DEV && editable(e.target)) return;
    e.preventDefault();
  },
  true
);
window.addEventListener(
  'keydown',
  (e) => {
    if (DEV && e.altKey) return;
    const k = e.key.toLowerCase();
    const ctrl = e.ctrlKey || e.metaKey;
    const devtools = e.key === 'F12' || (ctrl && e.shiftKey && ['i', 'j', 'c', 'k'].includes(k)) || (ctrl && (k === 'u' || k === 'p' || k === 's'));
    if (devtools) {
      e.preventDefault();
      e.stopPropagation();
    }
  },
  true
);

// Lỗi JS không ai bắt -> báo bằng thông báo (không bao giờ "bấm mà không thấy gì"); tối đa một lần mỗi 3 s.
let lastErr = 0;
async function reportError(err) {
  const msg = String(err?.message || err?.reason?.message || err?.reason || err || '').trim();
  if (!msg || /ResizeObserver loop/i.test(msg) || Date.now() - lastErr < 3000) return;
  lastErr = Date.now();
  console.error(err);
  try {
    const { toasts } = await import('./lib/toasts.svelte.js');
    const { t } = await import('./lib/i18n.svelte.js');
    toasts.error(t('shell.ui_error'), [msg.slice(0, 300)]);
  } catch {}
}
window.addEventListener('error', (e) => reportError(e.error || e.message));
window.addEventListener('unhandledrejection', (e) => reportError(e.reason));

// Nạp ngôn ngữ đã chọn (tiếng Anh tải động) trước khi vẽ để không nháy chữ tiếng Việt.
await i18n.init();
const app = mount(App, { target: document.getElementById('app') });
export default app;
