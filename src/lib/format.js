// Nhãn & định dạng dùng chung. Chữ lấy từ từ điển (i18n.svelte.js, khoá common.*); số theo ngôn ngữ đang chọn.
import { i18n, t, labels } from './i18n.svelte.js';

export const KIND_LABELS = labels({
  Run: 'common.kind.run',
  RegMove: 'common.kind.regmove',
  Folder: 'common.kind.folder',
  Task: 'common.kind.task',
  Service: 'common.kind.service',
  Packaged: 'common.kind.packaged',
  Svc: 'common.kind.service',
});
export const KIND_ORDER = ['Run', 'RegMove', 'Folder', 'Task', 'Service', 'Packaged'];

export const START_LABELS = labels({
  auto: 'common.start.auto',
  delayed: 'common.start.delayed',
  demand: 'common.start.demand',
  disabled: 'common.start.disabled',
  boot: 'common.start.boot',
  system: 'common.start.system',
});
export const START_ORDER = { boot: 0, system: 1, auto: 2, delayed: 3, demand: 4, disabled: 5 };
export const MODE_OPTIONS = ['auto', 'delayed', 'demand', 'disabled'];

export const STATE_LABELS = labels({
  running: 'common.state.running',
  stopped: 'common.state.stopped',
  starting: 'common.state.starting',
  stopping: 'common.state.stopping',
  paused: 'common.state.paused',
  pending: 'common.state.pending',
});
export const STATE_ORDER = { running: 0, starting: 1, stopping: 2, pending: 3, paused: 4, stopped: 5 };

/** chuỗi phụ thuộc ngôn ngữ -> getter của đối tượng; dùng `CRITICAL.tip` */
export const CRITICAL = labels({ tip: 'common.critical_tip' });

// Bộ định dạng số theo ngôn ngữ, tạo một lần cho mỗi (ngôn ngữ, số chữ số thập phân).
const nfCache = new Map();
function nf(d) {
  const k = i18n.intl + d;
  let f = nfCache.get(k);
  if (!f) {
    f = new Intl.NumberFormat(i18n.intl, { maximumFractionDigits: d, minimumFractionDigits: d });
    nfCache.set(k, f);
  }
  return f;
}
const nf0 = { format: (v) => nf(0).format(v) };
const nf1 = { format: (v) => nf(1).format(v) };

/** So sánh chữ (sắp xếp tên): giữ quy tắc tiếng Việt cho mọi ngôn ngữ để thứ tự không nhảy khi đổi ngôn ngữ. */
export const collator = new Intl.Collator('vi', { sensitivity: 'base', numeric: true });

/** MB -> "45 MB" / "1,6 GB" / "–" */
export function fmtMB(mb) {
  if (!(mb >= 0.5)) return '–';
  if (mb >= 1024) return nf1.format(mb / 1024) + ' GB';
  return nf0.format(mb) + ' MB';
}

export function ramClass(mb) {
  if (mb >= 1500) return 'ram-hi';
  if (mb >= 400) return 'ram-mid';
  return '';
}

export function fmtGB(bytes, digits = 1) {
  if (!bytes) return '0 GB';
  const v = bytes / 1073741824;
  return nf(digits).format(v) + ' GB';
}

export function fmtInt(n) {
  return nf0.format(n || 0);
}

export function fmtTime(t, seconds = true) {
  const d = new Date(t);
  const p = (n) => String(n).padStart(2, '0');
  return p(d.getHours()) + ':' + p(d.getMinutes()) + (seconds ? ':' + p(d.getSeconds()) : '');
}

export function errText(e) {
  if (e == null) return t('common.unknown_error');
  if (typeof e === 'string') return e;
  if (e.message) return e.message;
  try { return JSON.stringify(e); } catch { return String(e); }
}

export async function copyText(text) {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    const ta = document.createElement('textarea');
    ta.value = text;
    ta.style.position = 'fixed';
    ta.style.opacity = '0';
    document.body.appendChild(ta);
    ta.select();
    let ok = false;
    try { ok = document.execCommand('copy'); } catch {}
    ta.remove();
    return ok;
  }
}

// Chuỗi tìm kiếm của mỗi mục (cache theo đối tượng; mục mới -> chuỗi mới).
const hayCache = new WeakMap();
export function haystack(it) {
  let h = hayCache.get(it);
  if (h === undefined) {
    h = [it.name, it.publisher, it.command, it.source, it.key1, it.key2, it.file_path, it.description]
      .filter(Boolean)
      .join('\n')
      .toLowerCase();
    hayCache.set(it, h);
  }
  return h;
}

// ---------- v2: byte, tốc độ, phần trăm, thời lượng ----------
const UNITS = ['B', 'KB', 'MB', 'GB', 'TB'];

/** Byte -> "512 B" / "5,4 KB" / "12,9 GB" (hệ 1024 như Task Manager). */
export function fmtBytes(b, digits = 1) {
  if (!(b > 0)) return '0 B';
  let i = 0;
  let v = b;
  while (v >= 1024 && i < UNITS.length - 1) { v /= 1024; i++; }
  return nf(i === 0 ? 0 : v >= 100 ? 0 : digits).format(v) + ' ' + UNITS[i];
}
/** Byte/giây -> "5,5 MB/s" */
export function fmtRate(b) {
  return fmtBytes(b) + '/s';
}
/** Byte/giây -> bit/giây kiểu Task Manager: "48 Kbps", "1,2 Mbps" */
export function fmtBits(b) {
  const bits = (b || 0) * 8;
  if (bits < 1000) return nf(0).format(bits) + ' bps';
  if (bits < 1e6) return nf(bits < 1e4 ? 1 : 0).format(bits / 1e3) + ' Kbps';
  if (bits < 1e9) return nf(bits < 1e7 ? 1 : 0).format(bits / 1e6) + ' Mbps';
  return nf(1).format(bits / 1e9) + ' Gbps';
}
/** 27.43 -> "27%"; digits=1 -> "27,4%" */
export function fmtPct(v, digits = 0) {
  return nf(digits).format(v || 0) + '%';
}
/** Số giây -> "d:hh:mm:ss" (Task Manager) */
export function fmtUptime(s) {
  s = Math.max(0, Math.floor(s || 0));
  const d = Math.floor(s / 86400);
  const p = (n) => String(n).padStart(2, '0');
  return `${d}:${p(Math.floor((s % 86400) / 3600))}:${p(Math.floor((s % 3600) / 60))}:${p(s % 60)}`;
}
/** Số giây -> "3 ngày 4 giờ" / "2 giờ 5 phút" / "45 giây" (theo ngôn ngữ) */
export function fmtDuration(s) {
  s = Math.max(0, Math.floor(s || 0));
  if (s < 60) return t('common.dur.s', { s });
  if (s < 3600) return t('common.dur.m', { m: Math.floor(s / 60) });
  if (s < 86400) return t('common.dur.hm', { h: Math.floor(s / 3600), m: Math.floor((s % 3600) / 60) });
  return t('common.dur.dh', { d: Math.floor(s / 86400), h: Math.floor((s % 86400) / 3600) });
}
export function fmtGHz(mhz) {
  return nf(2).format((mhz || 0) / 1000) + ' GHz';
}

/** Thanh RAM (system_info.mem_modules) -> các dòng đã gộp thanh giống nhau:
 *  "2 × Micron Technology 4ATF1G64HZ-3G2F1 · DDR4 · SODIMM · 8 GB · 3200 MT/s" */
export function memModuleLines(info) {
  const groups = new Map();
  for (const m of info?.mem_modules || []) {
    const parts = [[m.maker, m.part].filter(Boolean).join(' '), m.kind, m.form, fmtGB(m.size, 0), m.speed ? `${m.speed} MT/s` : ''].filter(Boolean);
    const key = parts.join(' · ');
    groups.set(key, (groups.get(key) || 0) + 1);
  }
  return [...groups].map(([k, n]) => (n > 1 ? `${n} × ${k}` : k));
}
/** Tóm tắt RAM: { speed, kind, form, used, slots } (giá trị rỗng/0 nếu firmware không khai báo). */
export function memSpec(info) {
  const ms = info?.mem_modules || [];
  const uniq = (k) => [...new Set(ms.map((m) => m[k]).filter(Boolean))].join(' / ');
  return { speed: Math.max(0, ...ms.map((m) => m.speed || 0)), kind: uniq('kind'), form: uniq('form'), used: ms.length, slots: info?.mem_slots || 0 };
}

// ---------- phạm vi của mục khởi động (ai bị ảnh hưởng) ----------
/** 'user' | 'machine' | 'system' | '' – suy ra từ khoá registry / loại mục. */
export function scopeOf(it) {
  if (it.kind === 'Service' || it.kind === 'Svc') return 'system';
  if (it.kind === 'Task') return '';
  const k = (it.key1 || '').slice(0, 4).toUpperCase();
  if (k === 'HKCU') return 'user';
  if (k === 'HKLM') return 'machine';
  return '';
}
export const SCOPE_LABELS = labels({ user: 'common.scope.user', machine: 'common.scope.machine', system: 'common.scope.system' });
