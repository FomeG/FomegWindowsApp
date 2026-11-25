// Tuỳ chọn giao diện được nhớ trong localStorage (theme, sidebar thu gọn, AI panel, khoảng biểu đồ…).
function load(key, def) {
  try {
    const v = localStorage.getItem(key);
    return v == null ? def : JSON.parse(v);
  } catch {
    return def;
  }
}
function save(key, v) {
  try { localStorage.setItem(key, JSON.stringify(v)); } catch {}
}

class Prefs {
  theme = $state(typeof document !== 'undefined' && document.documentElement.dataset.theme === 'light' ? 'light' : 'dark');
  collapsed = $state(load('ui:collapsed', false));
  aiOpen = $state(load('ui:aiOpen', false));
  aiWidth = $state(load('ui:aiWidth', 380));
  perfSel = $state(load('ui:perfSel', 'cpu'));
  perfRange = $state(load('ui:perfRange', 1)); // phút
  perfCores = $state(load('ui:perfCores', false)); // true = từng CPU logic
  interval = $state(load('ui:interval', 1000));
  idleMs = $state(load('ui:idleMs', 5000)); // lấy mẫu khi cửa sổ ẩn; 0 = không
  staleDays = $state(load('ui:staleDays', 90)); // ngưỡng "Lâu không dùng" (trang Ứng dụng)
  appsMeasure = $state(load('ui:appsMeasure', true)); // tự đo dung lượng thật khi mở trang Ứng dụng
  startupAll = $state(load('ui:startupAll', false)); // trang Khởi động mở với mọi nguồn

  set(key, v) {
    this[key] = v;
    save('ui:' + key, v);
  }
  toggleTheme() {
    this.theme = this.theme === 'dark' ? 'light' : 'dark';
    document.documentElement.dataset.theme = this.theme;
    try { localStorage.setItem('theme', this.theme); } catch {}
  }
}

export const prefs = new Prefs();
export { load as loadPref, save as savePref };

// Store dùng chung (một bản duy nhất): sửa file khi dev -> tải lại cả trang, tránh hai bản cũ / mới cùng chạy sau HMR.
if (import.meta.hot) import.meta.hot.decline();
