// Trạng thái ngăn chi tiết (inspector) bên phải, dùng chung cho mọi trang dạng bảng + chi tiết
// (Tiến trình, Khởi động, Dịch vụ). Trang tạo một InspState, đưa cho <SplitView> và đọc
// `fits` / `on` / `shown` để vẽ nút ẩn/hiện ngăn.
//
// - Trang đủ rộng (bảng còn ≥ TABLE_MIN px): ngăn nằm cạnh bảng, kéo được độ rộng, ẩn/hiện được (nhớ theo trang).
// - Trang hẹp: ngăn không chiếm chỗ; Enter / bấm đúp mở dạng phủ (overlay) bên phải.
import { loadPref, savePref } from './prefs.svelte.js';

export const TABLE_MIN = 560;
export const INSP_MIN = 280;
export const INSP_MAX = 560;

export class InspState {
  pageW = $state(0);
  on = $state(true);
  w = $state(340);
  overlay = $state(false);

  /** key: tiền tố lưu tuỳ chọn ('proc' -> 'proc:insp', 'proc:inspW'). */
  constructor(key, width = 340) {
    this.key = key;
    this.on = loadPref(key + ':insp', true);
    this.w = loadPref(key + ':inspW', width);
  }

  get fits() {
    return this.pageW >= TABLE_MIN + INSP_MIN + 40;
  }
  get shown() {
    return this.on && this.fits;
  }
  get width() {
    return Math.max(INSP_MIN, Math.min(this.w, INSP_MAX, this.pageW - TABLE_MIN));
  }
  /** Độ rộng ngăn dạng phủ khi trang hẹp. */
  get overlayWidth() {
    return Math.min(this.w, Math.max(INSP_MIN, this.pageW - 48));
  }

  toggle() {
    this.on = !this.on;
    savePref(this.key + ':insp', this.on);
  }
  setWidth(w) {
    this.w = Math.max(INSP_MIN, Math.min(INSP_MAX, Math.round(w)));
  }
  saveWidth() {
    savePref(this.key + ':inspW', this.w);
  }
  /** "Xem chi tiết": ngăn đang hiện thì thôi; đủ chỗ thì bật lại; trang hẹp thì mở dạng phủ. */
  open() {
    if (this.shown) return;
    if (this.fits) {
      this.on = true;
      savePref(this.key + ':insp', true);
    } else this.overlay = true;
  }
  closeOverlay() {
    this.overlay = false;
  }
}

// Store dùng chung (một bản duy nhất): sửa file khi dev -> tải lại cả trang, tránh hai bản cũ / mới cùng chạy sau HMR.
if (import.meta.hot) import.meta.hot.decline();
