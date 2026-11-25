// Store trang Ổ cứng (phân tích dung lượng kiểu TreeSize).
//
// Cây đầy đủ nằm ở backend (core/du.rs). Frontend chỉ giữ các nút đã hiện (con của thư mục đang mở):
// - nodes: id -> NodeOut mới nhất; kids: id -> { ids, more }; paths: id -> đường dẫn đầy đủ
// - mỗi sự kiện `du` (~4 lần/giây khi quét) gọi du_children MỘT lần cho mọi thư mục đang mở -> cây lớn dần
//   trên màn hình trong lúc quét. Không chồng yêu cầu: đang chờ thì chỉ đánh dấu "cần làm mới".
// Map thường + `version` ($state) giống procs.svelte.js để không biến cả cây thành reactive.
import * as api from './api.js';
import { toasts } from './toasts.svelte.js';
import { errText } from './format.js';
import { loadPref, savePref } from './prefs.svelte.js';
import { t } from './i18n.svelte.js';

// Lỗi "lần quét đã kết thúc / bị thay" của du_children -> không báo. Chuỗi backend theo ngôn ngữ nên khớp cả hai
// (viết bằng mã \u để công cụ kiểm tra i18n không coi là chữ hiển thị).
const SCAN_GONE = /\u0111\u00e3 k\u1ebft th\u00fac|\bended\b|\bno longer\b|\breplaced\b/i;
const LIMIT = 500; // tối đa thư mục con mỗi nút gửi về (backend chọn top-k)

class Du {
  version = $state(0);
  progress = $state(null); // Progress mới nhất
  drives = $state([]);
  drivesLoading = $state(false);
  starting = $state(false);
  sort = $state(loadPref('du:sort', { key: 'size', desc: true }));

  scan = $state(0); // id lần quét hiện tại
  root = $state(''); // đường dẫn gốc
  nodes = new Map();
  kids = new Map();
  paths = new Map();
  expanded = new Set();
  files = new Map(); // dirId -> { total, items, loading, error }
  filesOpen = new Set();

  #unlisten = null;
  #busy = false;
  #dirty = false;
  #listening = null;

  get scanning() {
    return !!this.progress && !this.progress.done;
  }

  async init() {
    if (!this.#listening) {
      this.#listening = api.listen('du', (p) => this.#onProgress(p)).then((u) => (this.#unlisten = u));
    }
    await this.#listening;
    if (!this.drives.length) this.loadDrives();
    // quay lại trang sau khi đã quét: lấy lại trạng thái từ backend
    if (!this.scan) {
      try {
        const p = await api.duProgress();
        if (p) this.#adopt(p);
      } catch {}
    }
  }

  async loadDrives() {
    this.drivesLoading = true;
    try {
      this.drives = (await api.duDrives()) || [];
    } catch (e) {
      toasts.error(t('disk.toast.drives_failed'), [errText(e)]);
    } finally {
      this.drivesLoading = false;
    }
  }

  #reset() {
    this.nodes.clear();
    this.kids.clear();
    this.paths.clear();
    this.expanded.clear();
    this.files.clear();
    this.filesOpen.clear();
  }

  #adopt(p) {
    this.#reset();
    this.scan = p.scan;
    this.root = p.root;
    this.progress = p;
    this.paths.set(0, p.root);
    this.expanded.add(0);
    this.refresh();
  }

  async start(root) {
    root = (root || '').trim().replace(/^"|"$/g, '');
    if (!root) return;
    this.starting = true;
    try {
      const p = await api.duStart(root);
      this.#adopt(p);
    } catch (e) {
      toasts.error(t('disk.toast.scan_failed', { root }), [errText(e)]);
    } finally {
      this.starting = false;
    }
  }

  async cancel() {
    try { await api.duCancel(); } catch {}
  }

  async close() {
    try { await api.duClose(); } catch {}
    this.#reset();
    this.scan = 0;
    this.root = '';
    this.progress = null;
    this.version++;
    this.loadDrives();
  }

  #onProgress(p) {
    if (!p || p.scan !== this.scan) return;
    this.progress = p;
    this.refresh();
  }

  /** Lấy lại con của mọi thư mục đang mở (một lần gọi). */
  async refresh(only) {
    if (!this.scan) return;
    if (this.#busy) { this.#dirty = true; return; }
    this.#busy = true;
    const scan = this.scan;
    try {
      const ids = only ? [only] : [...this.expanded];
      const res = (await api.duChildren(scan, ids, this.sort.key, this.sort.desc, LIMIT)) || [];
      if (scan !== this.scan) return;
      for (const r of res) {
        this.nodes.set(r.node.id, r.node);
        this.paths.set(r.node.id, r.path);
        this.kids.set(r.node.id, { ids: r.children.map((c) => c.id), more: r.more });
        const base = r.path.endsWith('\\') ? r.path : r.path + '\\';
        for (const c of r.children) {
          this.nodes.set(c.id, c);
          this.paths.set(c.id, base + c.name);
        }
      }
      this.version++;
    } catch (e) {
      if (scan === this.scan && !SCAN_GONE.test(String(errText(e)))) toasts.error(t('disk.toast.read_failed'), [errText(e)]);
    } finally {
      this.#busy = false;
      if (this.#dirty) { this.#dirty = false; this.refresh(); }
    }
  }

  toggle(id) {
    if (this.expanded.has(id)) this.expanded.delete(id);
    else { this.expanded.add(id); this.refresh(id); }
    this.version++;
  }
  expand(id) {
    if (!this.expanded.has(id)) this.toggle(id);
  }

  setSort(key) {
    this.sort = this.sort.key === key ? { key, desc: !this.sort.desc } : { key, desc: key !== 'name' };
    savePref('du:sort', this.sort);
    this.refresh();
  }

  async toggleFiles(dirId) {
    if (this.filesOpen.has(dirId)) {
      this.filesOpen.delete(dirId);
      this.version++;
      return;
    }
    this.filesOpen.add(dirId);
    const path = this.paths.get(dirId);
    const cur = this.files.get(dirId);
    if (!cur || cur.error || (this.nodes.get(dirId)?.own_files ?? 0) !== cur.total) {
      this.files.set(dirId, { total: 0, items: [], loading: true });
      this.version++;
      try {
        const r = await api.duFiles(path, 2000);
        this.files.set(dirId, { total: r.total, items: r.items, loading: false });
      } catch (e) {
        this.files.set(dirId, { total: 0, items: [], loading: false, error: errText(e) });
      }
    }
    this.version++;
  }

  /** Sau khi xoá trên đĩa: bỏ khỏi cây (backend trừ số liệu khỏi thư mục cha) rồi làm mới. */
  async forgetDir(id) {
    try { await api.duForget(this.scan, id); } catch {}
    this.expanded.delete(id);
    this.filesOpen.delete(id);
    this.nodes.delete(id);
    this.refresh();
  }
  async forgetFile(dirId, file) {
    try { await api.duForgetFile(this.scan, dirId, file.size, file.alloc); } catch {}
    const f = this.files.get(dirId);
    if (f) {
      f.items = f.items.filter((x) => x !== file);
      f.total = Math.max(0, f.total - 1);
    }
    this.refresh();
  }
}

export const du = new Du();
if (import.meta.env.DEV && typeof window !== 'undefined') window.__fomegDu = du;

// Store dùng chung (một bản duy nhất): sửa file khi dev -> tải lại cả trang, tránh hai bản cũ / mới cùng chạy sau HMR.
if (import.meta.hot) import.meta.hot.decline();
