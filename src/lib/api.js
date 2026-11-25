// Lớp mỏng bọc invoke/listen. Trong trình duyệt thường (không có Tauri) dùng backend giả lập.
import { errText } from './format.js';

export const isTauri = typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__;

let backendP = null;
function backend() {
  if (!backendP) {
    backendP = isTauri
      ? Promise.all([import('@tauri-apps/api/core'), import('@tauri-apps/api/event')]).then(([core, ev]) => ({
          invoke: core.invoke,
          listen: ev.listen,
        }))
      : import('./mock.js').then((m) => m.createMockBackend());
  }
  return backendP;
}

async function call(cmd, args) {
  const b = await backend();
  try {
    return await b.invoke(cmd, args);
  } catch (e) {
    throw new Error(errText(e));
  }
}

/** Đăng ký sự kiện; trả về hàm huỷ. cb nhận payload. */
export async function listen(name, cb) {
  const b = await backend();
  return b.listen(name, (e) => cb(e.payload));
}

export const scan = () => call('scan');
export const toggleItems = (items, on) => call('toggle_items', { items, on });
export const serviceSetMode = (names, mode) => call('service_set_mode', { names, mode });
export const serviceControl = (names, action) => call('service_control', { names, action });
export const openLocation = (item) => call('open_location', { item });
export const getSettings = () => call('get_settings');
export const setRestartApps = (on) => call('set_restart_apps', { on });
export const openLog = () => call('open_log');
/** MCP server chỉ đọc (fomeg-windows-app-mcp.exe cạnh exe của app): { path, exists }. */
export const mcpInfo = () => call('mcp_info');
export const ramHistory = () => call('ram_history');

// ---------- v2: hiệu năng, tiến trình, AI (xem docs/contract-v2.md) ----------
// Sự kiện: 'metrics' (mỗi tick), 'procs' (khi set_watch procs=true), 'ai' (luồng trả lời).
export const systemInfo = () => call('system_info');
export const metricsHistory = () => call('metrics_history');
/** ms ∈ {500, 1000, 2000, 5000}, 0 = tạm dừng */
export const setTickInterval = (ms) => call('set_interval', { ms });
export const setVisible = (visible) => call('set_visible', { visible });
/** chu kỳ lấy mẫu khi cửa sổ ẩn: 5000 (mặc định), 15000, 60000, 0 = không lấy mẫu */
export const setIdleInterval = (ms) => call('set_idle_interval', { ms });
/** ngôn ngữ của backend (thông báo lỗi, trợ lý AI): 'vi' | 'en' */
export const setLocale = (locale) => call('set_locale', { locale });
/** mở lại app bằng quyền Admin (UAC) rồi đóng bản này */
export const relaunchAdmin = () => call('relaunch_admin');
export const setWatch = (procs) => call('set_watch', { procs });

export const procKill = (pids, tree = false) => call('proc_kill', { pids, tree });
/** Kiểm tra lại tiến trình trợ lý đề xuất kết thúc (đích = PID hoặc tên exe): [{ target, pid, name, exe, desc, mem, children, blocked? }] */
export const procLookup = (targets) => call('proc_lookup', { targets });
export const procRestart = (pid) => call('proc_restart', { pid });
export const procOpenLocation = (path) => call('proc_open_location', { path });
export const procProperties = (path) => call('proc_properties', { path });
/** level ∈ idle|below|normal|above|high|realtime */
export const procSetPriority = (pid, level) => call('proc_set_priority', { pid, level });
export const getIcons = (paths) => call('get_icons', { paths });

export const aiGetConfig = () => call('ai_get_config');
/** api_key '' = xoá khoá của target (mặc định là nhà cung cấp đang dùng). */
export function aiSetConfig({ provider, include_context, tools, target, model, effort, base_url, api_key }) {
  const fields = { provider, include_context, tools, target, model, effort, base_url, api_key };
  return call('ai_set_config', Object.fromEntries(Object.entries(fields).filter(([, value]) => value !== undefined)));
}
export const aiListModels = (provider) => call('ai_list_models', { provider });
/** disk: null = không gửi; { focus: đường dẫn | null } = kèm tóm tắt lần quét ổ cứng hiện tại.
 *  ports: true = kèm danh sách cổng đang nghe (backend tự đọc, khối <ports>). */
export const aiSend = (chat_id, text, disk = null, ports = false, cleanup = false) => call('ai_send', { chat_id, text, disk, ports, cleanup });
export const aiCancel = (chat_id) => call('ai_cancel', { chat_id });
export const aiReset = (chat_id) => call('ai_reset', { chat_id });

// ---------- Ổ cứng (phân tích dung lượng, xem docs/contract-v2.md "Disk usage") ----------
// Sự kiện: 'du' (Progress, ~4 lần/giây trong lúc quét và một lần khi xong).
export const duDrives = () => call('du_drives');
export const duStart = (root) => call('du_start', { root });
export const duProgress = () => call('du_progress');
export const duChildren = (scan, ids, sort = 'size', desc = true, limit = 500) => call('du_children', { scan, ids, sort, desc, limit });
export const duFiles = (path, limit = 2000) => call('du_files', { path, limit });
export const duCancel = () => call('du_cancel');
export const duClose = () => call('du_close');
export const duForget = (scan, id) => call('du_forget', { scan, id });
export const duForgetFile = (scan, dir, size, alloc) => call('du_forget_file', { scan, dir, size, alloc });
export const duDelete = (paths, permanent) => call('du_delete', { paths, permanent });
/** Thông tin các đường dẫn trợ lý đề xuất xoá: [{ path, exists, is_dir, size, files, blocked? }] */
export const duLookup = (paths) => call('du_lookup', { paths });
/** Xoá nhiều mục đã chọn, trả [{ path, ok, error? }] và bỏ chúng khỏi cây quét. */
export const duDeletePaths = (paths, permanent) => call('du_delete_paths', { paths, permanent });
export const duExists = (path) => call('du_exists', { path });
export const duOpen = (path, isDir) => call('du_open', { path, is_dir: isDir });
export const duMenu = (path, isDir, shift) => call('du_menu', { path, is_dir: isDir, shift });
export const duPickFolder = () => call('du_pick_folder');
// Dọn dẹp: quét trả kết quả sau ~2 s; mục đo xong sau đó đến qua sự kiện 'cleanup_item' { scan, item }.
export const cleanupScan = () => call('cleanup_scan');
/** Dọn các mục (theo mã) của lần quét `id`. */
export const cleanupApply = (id, items, keepRecent) => call('cleanup_apply', { id, items, keepRecent });

// ---------- Ứng dụng đã cài (docs/contract-v2.md "Apps") ----------
/** { apps: [App], took_ms, store_error } */
/** force = bỏ bản nhớ, đọc lại hết (nút Làm mới). Không force: backend trả bản nhớ nếu chưa cài / gỡ gì (~1 ms). */
export const appsList = (force = false) => call('apps_list', { force });
/** Đo thư mục cài đặt; từng thư mục xong -> sự kiện 'app_size' { gen, path, bytes, files, done }. Gọi lại = huỷ lần trước. */
export const appsMeasure = (gen, paths) => call('apps_measure', { gen, paths });
export const appsMeasureStop = () => call('apps_measure_stop');
/** Chạy trình gỡ của app; trả tên app. Xong -> sự kiện 'app_uninstall' { id, name, removed, error, took_ms }. */
export const appsUninstall = (id) => call('apps_uninstall', { id });
export const appsModify = (id) => call('apps_modify', { id });
/** Thôi chờ trình gỡ đang chạy (app vẫn đọc lại registry và báo 'app_uninstall'). */
export const appsUninstallStop = () => call('apps_uninstall_stop');
/** what: 'folder' | 'registry' */
export const appsOpen = (id, what) => call('apps_open', { id, what });
/** Thư mục cài đặt + thư mục liên quan (đã đo): [Found] */
export const appsRelated = (id) => call('apps_related', { id });
/** Phần còn sót: { items: [Found], took_ms, removed_known, windowsapps_error }.
 *  Found = { path, package, kind: 'folder'|'package'|'info', area, owner, reason, sure, shared, bytes, files, newest, partial } */
export const leftoversScan = () => call('leftovers_scan');
export const leftoversRemovePackages = (names) => call('leftovers_remove_packages', { names });
export const leftoversForget = (name) => call('leftovers_forget', { name });

// ---------- Cổng (xem docs/contract-v2.md "Ports") ----------
/** { uses: [{ proto, port, addrs, pid, conns, reach, name, exe, cpu, mem }], took_ms, pending } */
export const portsList = (udp) => call('ports_list', { udp });
export const openPort = (port) => call('open_port', { port });
/** name ∈ services | tasks | startup_folder */
export const openTool = (name) => call('open_tool', { name });
