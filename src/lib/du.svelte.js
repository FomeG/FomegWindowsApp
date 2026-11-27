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
    this.loadDrives(true); // luôn lấy lại (~1 ms): ổ có thể đã đổi từ lần mở trước (xoá / dọn ở trang khác)
    // quay lại trang sau khi đã quét: lấy lại trạng thái từ backend
    if (!this.scan) {
      try {
        const p = await api.duProgress();
        if (p) this.#adopt(p);
      } catch {}
    }
  }

  /** Đọc lại ổ đĩa (rẻ, ~1 ms). `silent`: không báo lỗi bằng toast (gọi nền sau khi xoá / mở trang). */
  async loadDrives(silent = false) {
    this.drivesLoading = true;
    try {
      this.drives = (await api.duDrives()) || [];
    } catch (e) {
      if (!silent) toasts.error(t('disk.toast.drives_failed'), [errText(e)]);
    } finally {
      this.drivesLoading = false;
    }
  }

  /** Ổ chứa thư mục gốc đang quét (để hiện dung lượng trống bên cạnh số liệu của cây). */
  get rootDrive() {
    const r = (this.root || '').toLowerCase();
    return this.drives.find((d) => d.ready && d.kind !== 'network' && r.startsWith(d.root.toLowerCase())) || null;
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
    const wasDone = !!this.progress?.done;
    this.progress = p;
    if (p.done && !wasDone) this.loadDrives(true); // quét xong: số ổ trống hiện cạnh số liệu của cây phải mới
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
      // nút đã bị gỡ khỏi cây (xoá / đo lại một phần): backend không trả nữa -> bỏ trạng thái cục bộ của nó
      const got = new Set(res.map((r) => r.node.id));
      for (const id of ids) if (id !== 0 && !got.has(id)) this.#drop(id);
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

  /** Số liệu ngoài cây sau một thay đổi trên đĩa: ổ trống (du_drives, ~1 ms) và bộ đếm "N tệp · M thư mục · X" của lần quét. */
  async #numbers() {
    this.loadDrives(true);
    const scan = this.scan;
    try {
      const p = await api.duProgress();
      if (p && p.scan === scan && scan === this.scan) this.progress = p;
    } catch {}
  }

  /** Bỏ trạng thái cục bộ của nút `id` và mọi nhánh dưới nó (đã bị gỡ khỏi cây). */
  #drop(id) {
    const stack = [id];
    while (stack.length) {
      const c = stack.pop();
      for (const g of this.kids.get(c)?.ids || []) stack.push(g);
      this.nodes.delete(c);
      this.paths.delete(c);
      this.kids.delete(c);
      this.expanded.delete(c);
      this.filesOpen.delete(c);
      this.files.delete(c);
    }
  }

  /** Danh sách tệp đã đọc của `id` và các thư mục dưới nó có thể cũ (nhánh vừa được đo lại): đọc lại khi mở. */
  #dropFiles(id) {
    const stack = [id];
    while (stack.length) {
      const c = stack.pop();
      for (const g of this.kids.get(c)?.ids || []) stack.push(g);
      this.filesOpen.delete(c);
      this.files.delete(c);
    }
  }

  async #dirChanged(id, gone) {
    if (gone) this.#drop(id);
    else this.#dropFiles(id); // còn một phần: id các nút còn lại giữ nguyên, nút đã mất sẽ bị bỏ ở lần làm mới
    this.version++;
    await this.refresh();
    this.#numbers();
  }
  async #fileGone(dirId, file) {
    const f = this.files.get(dirId);
    if (f) {
      f.items = f.items.filter((x) => x !== file);
      f.total = Math.max(0, f.total - 1);
    }
    this.version++;
    await this.refresh();
    this.#numbers();
  }

  /** Thay đổi trên đĩa do nơi khác gây ra (trợ lý AI xoá, dọn dẹp…): làm mới cây đang mở (nếu có) và ổ trống, không quét lại. */
  async changed(paths = []) {
    if (paths.length) this.#dropFilesAt(paths);
    if (this.scan) await this.refresh();
    this.#numbers();
  }

  /** Bỏ danh sách tệp đã đọc của các thư mục bị ảnh hưởng bởi `paths` (chính nó, bên dưới nó, hoặc thư mục chứa nó). */
  #dropFilesAt(paths) {
    const norm = (p) => String(p).replace(/[\\/]+$/, '').toLowerCase();
    const del = paths.map(norm);
    const parentOf = (p) => p.slice(0, Math.max(0, p.lastIndexOf('\\')));
    for (const [id, p0] of [...this.paths]) {
      const p = norm(p0);
      if (del.some((d) => p === d || p.startsWith(d + '\\') || p === parentOf(d))) {
        this.files.delete(id);
        this.filesOpen.delete(id);
      }
    }
    this.version++;
  }

  /**
   * Sau khi backend đã xoá (`du_delete` tự cập nhật cây): chỉ làm mới các nút bị ảnh hưởng, không quét lại.
   * `gone` = đường dẫn đã mất; còn lại (xoá một phần) thì backend đã đo lại riêng nhánh đó, đồng bộ tại chỗ (id các nút còn lại giữ nguyên).
   */
  deletedDir(id, gone) {
    return this.#dirChanged(id, gone);
  }
  deletedFile(dirId, file) {
    return this.#fileGone(dirId, file);
  }

  /** Danh sách tệp của thư mục `dirId` có thể đã đổi (thao tác ngoài app): bỏ bản đã đọc, đọc lại khi mở. */
  async dirContentsChanged(dirId) {
    this.files.delete(dirId);
    this.filesOpen.delete(dirId);
    this.version++;
    await this.refresh();
    this.#numbers();
  }
}

export const du = new Du();
if (import.meta.env.DEV && typeof window !== 'undefined') window.__fomegDu = du;

// Store dùng chung (một bản duy nhất): sửa file khi dev -> tải lại cả trang, tránh hai bản cũ / mới cùng chạy sau HMR.
if (import.meta.hot) import.meta.hot.decline();
