// Đa ngôn ngữ: từ điển phẳng theo khoá (src/lib/i18n/<locale>/*.js), tham số {name}, số nhiều theo Intl.PluralRules
// (khoá `x_one` / `x_other`, như i18next), số / ngày theo Intl. Tiếng Việt là mặc định và luôn có sẵn; tiếng Anh tải
// khi cần (import động, không nằm trong gói chính). `t()` đọc `i18n.locale` ($state) nên giao diện đổi ngay, không tải lại.
import vi from './i18n/vi/index.js';

export const LOCALES = [
  ['vi', 'Tiếng Việt'],
  ['en', 'English'],
];
const LOADERS = { en: () => import('./i18n/en/index.js') };
const INTL = { vi: 'vi-VN', en: 'en-US' };
const KEY = 'ui:locale';

const dicts = { vi };

function saved() {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) || 'null');
    return LOCALES.some(([id]) => id === v) ? v : 'vi';
  } catch {
    return 'vi';
  }
}

class I18n {
  locale = $state('vi');
  /** gọi mỗi lần đổi ngôn ngữ (main.js: báo backend qua set_locale) */
  onChange = null;
  /** 'vi-VN' / 'en-US' cho Intl.* (số, ngày, so sánh chuỗi) */
  intl = $derived(INTL[this.locale] || 'vi-VN');
  #plural = { vi: new Intl.PluralRules('vi-VN') };

  /** Gọi một lần trước khi mount: nạp từ điển đã lưu và báo backend. */
  async init() {
    await this.set(saved(), { persist: false });
  }

  async set(locale, { persist = true } = {}) {
    if (!LOCALES.some(([id]) => id === locale)) locale = 'vi';
    if (!dicts[locale]) {
      try {
        dicts[locale] = (await LOADERS[locale]()).default;
      } catch (e) {
        console.error('i18n', e);
        locale = 'vi';
      }
    }
    this.#plural[locale] ??= new Intl.PluralRules(INTL[locale]);
    this.locale = locale;
    document.documentElement.lang = locale;
    if (persist) {
      try { localStorage.setItem(KEY, JSON.stringify(locale)); } catch {}
    }
    // backend (thông báo lỗi, trợ lý AI) theo cùng ngôn ngữ: main.js gắn hàm báo backend (tránh vòng import với api.js)
    this.onChange?.(locale);
  }

  plural(n) {
    return (this.#plural[this.locale] || this.#plural.vi).select(n);
  }
}

export const i18n = new I18n();

/**
 * Chuỗi theo khoá. `params.count` chọn dạng số nhiều (`key_one` / `key_other`, thiếu thì `key`).
 * Thiếu khoá ở ngôn ngữ hiện tại -> dùng tiếng Việt; thiếu cả hai -> trả về chính khoá (dễ thấy để bổ sung).
 */
export function t(key, params) {
  const d = dicts[i18n.locale] || vi;
  let s;
  if (params && typeof params.count === 'number') {
    const form = `${key}_${i18n.plural(params.count)}`;
    s = d[form] ?? d[`${key}_other`] ?? vi[form] ?? vi[`${key}_other`];
  }
  s ??= d[key] ?? vi[key];
  if (s == null) {
    if (import.meta.env.DEV) console.warn('[i18n] thiếu khoá', key);
    return key;
  }
  return params ? s.replace(/\{(\w+)\}/g, (m, k) => (params[k] ?? m)) : s;
}

/** Có khoá này không (cho nhãn tuỳ chọn, vd. tên công cụ AI). */
export const hasT = (key) => key in (dicts[i18n.locale] || vi) || key in vi;

if (import.meta.hot) import.meta.hot.decline();

/**
 * Bảng nhãn cố định theo mã (vd. START_LABELS): `labels({ auto: 'common.start.auto', … })` trả về đối tượng có getter,
 * nên `START_LABELS[x]` vẫn dùng như cũ mà tự đổi theo ngôn ngữ (đọc trong template là phản ứng theo `i18n.locale`).
 */
export function labels(keys) {
  const o = {};
  for (const [k, key] of Object.entries(keys)) Object.defineProperty(o, k, { get: () => t(key), enumerable: true });
  return o;
}
