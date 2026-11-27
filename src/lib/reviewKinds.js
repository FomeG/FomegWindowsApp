// Bộ xử lý cho popup xem lại đề xuất của trợ lý (ActionReview.svelte), mỗi loại khối hành động một bộ (xem aiActions.js).
//
// Giao diện một bộ xử lý:
//   title, hint                 – tiêu đề + dòng giải thích của popup
//   async load(items)           – kiểm tra lại từng mục với backend -> hàng
//                                 [{ key, label, reason, sub?, side?, icon, blocked, checked }]
//                                 exe? = đường dẫn exe -> icon thật của ứng dụng thay cho Glyph `icon`
//                                 details? = [{ text, side?, warn? }] -> nút mở / thu gọn danh sách con (vd. từng PID)
//   options?                    – ô tích thêm: [{ id, label, warn?, init?() }] (vd. "Xoá vĩnh viễn"; init = giá trị đầu)
//   plainLabel?                 – nhãn hàng là tên thường (mặc định: chữ mono như đường dẫn)
//   confirmLabel(opts, n)       – chữ nút thực hiện
//   summary?(chosen, opts)      – chữ ở chân popup
//   view?(row, opts)            – hàng như hiển thị theo giá trị HIỆN TẠI của các ô tuỳ chọn (vd. side / size đổi theo "giữ tệp mới")
//   doneText                    – nhãn của hàng đã thực hiện xong
//   async run(chosen, opts)     – thực hiện, trả Map key -> { ok, error }
//   after?(okRows, failRows, opts) – thông báo / làm mới dữ liệu
//   title / hint có thể là hàm (review) -> chữ; allowEmpty = bấm thực hiện được khi không tích mục nào (vd. gỡ app mà
//   không xoá thư mục nào); emptyText = chữ khi không có hàng nào
import * as api from './api.js';
import { du } from './du.svelte.js';
import { ai } from './ai.svelte.js';
import { toasts } from './toasts.svelte.js';
import { fmtBytes, fmtInt, errText, KIND_LABELS, START_LABELS, STATE_LABELS, CRITICAL } from './format.js';
import { endBlocked, relayHint, REACH, PORTS_CHANGED } from './ports.js';
import { cleanup } from './cleanup.svelte.js';
import { store } from './store.svelte.js';
import { procs } from './procs.svelte.js';
import { OP_LABELS } from './aiActions.js';
import { apps } from './apps.svelte.js';
import { i18n, t } from './i18n.svelte.js';
import { env, entryKey } from './env.svelte.js';

// chữ đổi theo ngôn ngữ: title / hint / doneText / emptyText / options là getter (đọc lúc vẽ popup)
/** tham số số nhiều: count chọn dạng, n là số đã định dạng */
const cnt = (n, more) => ({ count: n, n: fmtInt(n), ...more });

// ---------------- xoa: xoá thư mục / tệp ----------------
let inexactDeleted = []; // đường dẫn của lần xoá gần nhất mà số liệu cây quét có thể chưa chính xác
const del = {
  get title() { return t('ai.review.del.title'); },
  get hint() { return t('ai.review.del.hint'); },
  get doneText() { return t('ai.review.del.done'); },
  get options() { return [{ id: 'permanent', label: t('ai.review.del.opt_permanent'), warn: t('ai.review.del.opt_permanent_warn') }]; },
  async load(items) {
    const info = await api.duLookup(items.map((x) => x.path));
    return items.map((x, i) => {
      const d = info[i] || {};
      const size = d.size ?? null;
      return {
        key: (d.path || x.path).toLowerCase(),
        label: d.path || x.path,
        reason: x.reason,
        icon: d.is_dir === false ? 'document' : 'folder',
        blocked: d.blocked || '',
        checked: !d.blocked,
        size,
        side: size != null ? (d.files != null ? t('ai.review.size_files', cnt(d.files, { size: fmtBytes(size) })) : fmtBytes(size)) : d.exists ? t('ai.review.unknown') : '',
      };
    });
  },
  confirmLabel: (o, n) => t(o.permanent ? 'ai.review.del.confirm_permanent' : 'ai.review.del.confirm_bin') + (n ? ` (${fmtInt(n)})` : ''),
  summary: (chosen) => {
    const size = chosen.reduce((a, r) => a + (r.size || 0), 0);
    return t('ai.review.summary_items', cnt(chosen.length, { size: fmtBytes(size) + (chosen.some((r) => r.size == null) ? '+' : '') }));
  },
  async run(chosen, o) {
    const res = await api.duDeletePaths(chosen.map((r) => r.label), !!o.permanent);
    inexactDeleted = res.filter((x) => x.inexact).map((x) => x.path);
    return new Map(res.map((x) => [x.path.toLowerCase(), { ok: x.ok, error: x.error || t('ai.review.del.failed') }]));
  },
  after(ok, fail, o) {
    // backend đã cập nhật cây quét (kể cả xoá một phần): đọc lại cây đang mở + ổ trống, bỏ danh sách tệp cũ của các thư mục bị ảnh hưởng
    du.changed([...ok, ...fail].map((r) => r.label));
    if (inexactDeleted.length) toasts.warn(t('disk.toast.inexact'), [...inexactDeleted.slice(0, 3), t('disk.toast.inexact_detail')]);
    if (ok.length) {
      ai.diskChanged();
      const freed = ok.reduce((a, r) => a + (r.size || 0), 0);
      toasts.success(t(o.permanent ? 'ai.review.del.ok_permanent' : 'ai.review.del.ok_bin', cnt(ok.length, { freed: freed ? ` · ${fmtBytes(freed)}` : '' })));
    }
    if (fail.length) toasts.error(t('ai.review.del.fail', cnt(fail.length)), fail.slice(0, 5).map((r) => `${r.label}: ${r.error}`));
  },
};

// ---------------- dong_cong: kết thúc chương trình giữ cổng ----------------
const procText = (uses) => [...new Set(uses.map((u) => `${u.name || '?'} (${u.pid})`))].join(', ');
const ports = {
  get title() { return t('ai.review.ports.title'); },
  get hint() { return t('ai.review.ports.hint'); },
  get doneText() { return t('ai.review.ports.done'); },
  async load(items) {
    const r = await api.portsList(true);
    const by = new Map();
    for (const u of r?.uses || []) {
      if (!by.has(u.port)) by.set(u.port, []);
      by.get(u.port).push(u);
    }
    return items.map((x) => {
      const all = by.get(x.port) || [];
      const uses = all.some((u) => u.proto === 'tcp') ? all.filter((u) => u.proto === 'tcp') : all;
      const blocked = !uses.length ? t('ai.review.ports.empty') : uses.map((u) => endBlocked(u.pid, u.name)).find(Boolean) || '';
      const reach = uses.find((u) => u.reach === 'all')?.reach || uses[0]?.reach;
      const relay = uses.map((u) => relayHint(u.name)).find(Boolean);
      return {
        key: String(x.port),
        label: t('ai.act.port', { port: x.port }) + (uses.length ? ' · ' + procText(uses) : ''),
        reason: relay ? `${x.reason ? x.reason + ' · ' : ''}${relay}` : x.reason,
        icon: 'network',
        blocked,
        checked: !blocked,
        pids: [...new Set(uses.map((u) => u.pid))],
        owners: new Map(uses.map((u) => [u.pid, (u.name || '').toLowerCase()])),
        side: reach ? REACH[reach][0] : '',
      };
    });
  },
  confirmLabel: (o, n) => (n ? t('ai.review.ports.confirm_n', cnt(n)) : t('ai.review.ports.confirm')),
  summary: (chosen) => t('ai.review.ports.summary', cnt(chosen.length, { procs: fmtInt(new Set(chosen.flatMap((r) => r.pids)).size) })),
  async run(chosen) {
    const fresh = (await api.portsList(true))?.uses || [];
    const matching = new Map(chosen.map((r) => [r.key, new Set(r.pids.filter((pid) =>
      fresh.some((u) => u.port === Number(r.key) && u.pid === pid && u.name && u.name.toLowerCase() === r.owners.get(pid))
    ))]));
    const pids = [...new Set(chosen.flatMap((r) => [...matching.get(r.key)]))];
    const res = pids.length ? (await api.procKill(pids, false)) || [] : [];
    const byPid = new Map(res.map((x) => [x.pid, x]));
    const out = new Map();
    for (const r of chosen) {
      const bad = r.pids.find((pid) => !matching.get(r.key).has(pid) || !byPid.get(pid)?.ok);
      const error = matching.get(r.key).has(bad) ? byPid.get(bad)?.error || t('ai.review.ports.no_response') : t('ai.review.kill.not_found');
      out.set(r.key, bad == null ? { ok: true } : { ok: false, error: `PID ${bad}: ${error}` });
    }
    return out;
  },
  after(ok, fail) {
    window.dispatchEvent(new Event(PORTS_CHANGED)); // trang Cổng (nếu đang mở) đọc lại ngay
    if (ok.length) toasts.success(t('ai.review.ports.ok', cnt(ok.length)), ok.slice(0, 5).map((r) => r.label));
    if (fail.length) toasts.error(t('ai.review.ports.fail', cnt(fail.length)), fail.slice(0, 5).map((r) => `${r.label}: ${r.error}`));
  },
};

// ---------------- don_dep: dọn mục của trang Dọn dẹp ----------------
// Dung lượng dọn được theo ô "giữ tệp mới" của popup (không theo cài đặt chung), cùng công thức với trang Dọn dẹp.
const freeableFor = (it, keep) => cleanup.freeable(it, keep);
const clean = {
  get title() { return t('ai.review.clean.title'); },
  plainLabel: true, // nhãn là tên mục, không phải đường dẫn
  get hint() { return t('ai.review.clean.hint'); },
  get doneText() { return t('ai.review.clean.done'); },
  get options() { return [{ id: 'keepRecent', label: t('ai.review.clean.opt_keep_recent'), init: () => cleanup.keepRecent }]; },
  async load(items) {
    if (!cleanup.scanId) await cleanup.scan();
    const by = new Map(cleanup.items.map((it) => [it.id.toLowerCase(), it]));
    return items.map((x) => {
      const it = by.get(x.key);
      const blocked = !it ? t('ai.review.clean.not_in_scan') : it.error && !it.paths.length && it.id !== 'recycle' ? it.error : !it.pending && it.bytes === 0 ? t('ai.review.clean.empty') : '';
      return {
        key: it?.id ?? x.key,
        label: it?.name ?? x.label,
        reason: x.reason,
        icon: it?.group === 'cache' ? 'folder' : 'delete',
        blocked,
        checked: !blocked,
        // dung lượng thật sự dọn được (trừ tệp mới khi "giữ tệp mới", tệp khoá lần trước): cùng số với trang Dọn dẹp
        it,
        size: it ? freeableFor(it, cleanup.keepRecent) : 0,
        side: !it ? '' : it.pending ? t('ai.review.measuring') : t('ai.review.size_files', cnt(it.files, { size: fmtBytes(freeableFor(it, cleanup.keepRecent)) })),
      };
    });
  },
  confirmLabel: (o, n) => t('ai.review.clean.confirm') + (n ? ` (${fmtInt(n)})` : ''),
  // kích thước hiện theo ô "giữ tệp mới" của popup NGAY (không đóng băng lúc mở popup)
  view(r, o) {
    if (!r.it || r.it.pending || r.blocked) return r;
    const size = freeableFor(r.it, !!o.keepRecent);
    return { ...r, size, side: t('ai.review.size_files', cnt(r.it.files, { size: fmtBytes(size) })) };
  },
  summary(chosen, o) {
    const size = chosen.reduce((a, r) => a + (clean.view(r, o).size || 0), 0);
    return t('ai.review.summary_items', cnt(chosen.length, { size: fmtBytes(size) }));
  },
  async run(chosen, o) {
    const res = await cleanup.clean(chosen.map((r) => r.key), !!o.keepRecent);
    return new Map(chosen.map((r) => {
      const x = res.get(r.key);
      return [r.key, x ? { ok: !x.error, error: x.error } : { ok: false, error: t('ai.review.no_result') }];
    }));
  },
  after() {
    du.changed(); // ổ trống (và cây Ổ cứng đang mở) đổi sau khi dọn
  },
};

// ---------------- tat_khoi_dong: tắt mục khởi động ----------------
/** Tìm mục theo đích trợ lý viết: mã mục, tên đúng (mọi mục cùng tên), rồi tên chứa đích nếu chỉ có một. */
function findStartup(x) {
  const list = store.startup;
  const byId = list.find((it) => it.id.toLowerCase() === x.key);
  if (byId) return [byId];
  const exact = list.filter((it) => it.name.toLowerCase() === x.key);
  if (exact.length) return exact;
  const part = list.filter((it) => it.name.toLowerCase().includes(x.key));
  return part.length === 1 ? part : [];
}
const ensureScan = async () => {
  if (!store.loaded) await store.scan();
};
const startup = {
  get title() { return t('ai.review.startup.title'); },
  plainLabel: true,
  get hint() { return t('ai.review.startup.hint'); },
  get doneText() { return t('ai.review.startup.done'); },
  async load(items) {
    await ensureScan();
    const rows = [];
    const seen = new Set();
    for (const x of items) {
      const found = findStartup(x);
      if (!found.length) {
        rows.push({ key: x.key, label: x.label, reason: x.reason, icon: 'startup', blocked: t('ai.review.startup.not_found'), checked: false, side: '' });
        continue;
      }
      for (const it of found) {
        if (seen.has(it.id)) continue;
        seen.add(it.id);
        const blocked = !it.on ? t('ai.review.startup.already_off') : it.critical ? CRITICAL.tip : '';
        const mb = store.ram.get(it.id);
        rows.push({
          key: it.id,
          label: it.name,
          reason: x.reason || it.publisher || '',
          icon: 'startup',
          blocked,
          checked: !blocked,
          item: it,
          side: [KIND_LABELS[it.kind] || it.kind, mb ? `${fmtInt(Math.round(mb))} MB` : ''].filter(Boolean).join(' · '),
        });
      }
    }
    return rows;
  },
  confirmLabel: (o, n) => t('ai.review.startup.confirm') + (n ? ` (${fmtInt(n)})` : ''),
  summary: (chosen) => t('ai.review.summary_n', cnt(chosen.length)),
  async run(chosen) {
    const res = await store.toggle(chosen.map((r) => store.startupById.get(r.key) || r.item), false, { quiet: true });
    return new Map(chosen.map((r) => {
      const e = res.get(r.key);
      return [r.key, e === '' ? { ok: true } : { ok: false, error: e || t('ai.review.no_result') }];
    }));
  },
  after(ok, fail) {
    if (ok.length) toasts.success(t('ai.review.startup.ok', cnt(ok.length)), ok.slice(0, 5).map((r) => r.label));
    if (fail.length) toasts.error(t('ai.review.startup.fail', cnt(fail.length)), fail.slice(0, 5).map((r) => `${r.label}: ${r.error}`));
  },
};

// ---------------- dich_vu: đổi kiểu khởi động / dừng / chạy dịch vụ ----------------
const svcOf = (name) => {
  const n = name.toLowerCase();
// ---------------- bien_moi_truong: sửa biến môi trường ----------------
// Mỗi hàng là một thao tác; khi thực hiện, các thao tác đã tích của CÙNG một biến được gộp và ghi một lần
// (giá trị đọc lại ngay trước khi ghi, nên sửa tay ở trang trong lúc popup mở vẫn được giữ).
const envMatch = (v, value) => {
  const k = entryKey(value);
  return v.entries.findIndex((e) => e.raw.toLowerCase() === value.toLowerCase() || entryKey(e.expanded) === k || entryKey(e.raw) === k);
};
const envVars = {
  get title() { return t('ai.review.env.title'); },
  plainLabel: true,
  get hint() { return t('ai.review.env.hint'); },
  get doneText() { return t('ai.review.env.done'); },
  async load(items) {
    await env.load();
    return items.map((x) => {
      const v = env.find(x.scope, x.name);
      let blocked = '';
      let sub = '';
      if (x.scope === 'system' && !env.canSystem) blocked = t('env.need_admin');
      else if (x.op === 'del') blocked = !v ? t('ai.review.env.not_found') : v.protected ? t('env.protected_tip') : '';
      else if (x.op === 'remove') {
        const i = v ? envMatch(v, x.value) : -1;
        if (!v) blocked = t('ai.review.env.not_found');
        else if (i < 0) blocked = t('ai.review.env.no_entry');
        else {
          const e = v.entries[i];
          sub = e.dup ? t('env.entry.dup') : e.other ? t('env.entry.other') : e.exists === false ? t('env.entry.missing') : e.exists ? t('env.entry.ok') : '';
        }
      } else if (x.op === 'add') {
        if (v && !v.list && v.value.trim()) blocked = t('ai.review.env.not_list');
        else if (v && envMatch(v, x.value) >= 0) blocked = t('ai.review.env.already');
      } else if (x.op === 'set') {
        if (v && v.value === x.value) blocked = t('ai.review.env.already');
        else if (v?.secret) sub = t('ai.review.env.secret_set');
        else if (v) sub = t('ai.review.env.was', { value: v.value.length > 80 ? `${v.value.slice(0, 80)}…` : v.value || '–' });
      }
      if (!blocked && x.op === 'set' && x.name.toLowerCase() === 'path') sub = t('ai.review.env.path_set');
      return { key: x.key, label: x.label, reason: x.reason, sub, icon: x.op === 'del' ? 'delete' : 'bullets', blocked, checked: !blocked, side: t(`env.scope.${x.scope}`), x };
    });
  },
  confirmLabel: (o, n) => t('ai.review.env.confirm') + (n ? ` (${fmtInt(n)})` : ''),
  summary: (chosen) => t('ai.review.summary_n', cnt(chosen.length)),
  async run(chosen) {
    await env.load();
    const groups = new Map();
    for (const r of chosen) {
      const id = `${r.x.scope}:${r.x.name.toLowerCase()}`;
      if (!groups.has(id)) groups.set(id, []);
      groups.get(id).push(r);
    }
    const out = new Map();
    for (const rows of groups.values()) {
      const { scope, name } = rows[0].x;
      const v = env.find(scope, name);
      let entries = v ? (v.list ? v.entries.map((e) => e.raw) : v.value ? [v.value] : []) : [];
      let value = v ? v.value : null;
      let listy = !v || v.list;
      for (const { x } of rows) {
        if (x.op === 'del') { value = null; listy = false; }
        else if (x.op === 'set') { value = x.value; listy = false; }
        else if (x.op === 'add') { entries = [...entries, x.value]; listy = true; }
        else if (x.op === 'remove') {
          const k = entryKey(x.value);
          const exp = new Map((v?.entries || []).map((e) => [e.raw, e.expanded]));
          entries = entries.filter((raw) => raw.toLowerCase() !== x.value.toLowerCase() && entryKey(raw) !== k && entryKey(exp.get(raw) || '') !== k);
          listy = true;
        }
      }
      const final = listy && !rows.some((r) => r.x.op === 'set' || r.x.op === 'del') ? entries.join(';') : value;
      try {
        await env.set(scope, v?.name || name, final, { quiet: true });
        for (const r of rows) out.set(r.key, { ok: true });
      } catch (e) {
        for (const r of rows) out.set(r.key, { ok: false, error: errText(e) });
      }
    }
    return out;
  },
  after(ok, fail) {
    if (ok.length) toasts.success(t('ai.review.env.ok', cnt(ok.length)), [...ok.slice(0, 5).map((r) => r.label), t('env.toast.restart_hint')]);
    if (fail.length) toasts.error(t('ai.review.env.fail', cnt(fail.length)), fail.slice(0, 5).map((r) => `${r.label}: ${r.error}`));
  },
};

  const list = store.services;
  return list.find((s) => s.key1.toLowerCase() === n) || list.find((s) => s.name.toLowerCase() === n) || null;
};
const services = {
  get title() { return t('ai.review.svc.title'); },
  plainLabel: true,
  get hint() { return t('ai.review.svc.hint'); },
  get doneText() { return t('ai.review.svc.done'); },
  async load(items) {
    await ensureScan();
    return items.map((x) => {
      const s = svcOf(x.name);
      const isMode = x.op !== 'stop' && x.op !== 'start';
      let blocked = '';
      if (!s) blocked = t('ai.review.svc.not_found');
      else if (s.critical && (x.op === 'demand' || x.op === 'disabled' || x.op === 'stop')) blocked = CRITICAL.tip;
      else if (s.start_type === 'boot' || s.start_type === 'system') blocked = t('ai.review.svc.boot');
      else if (isMode && s.start_type === x.op) blocked = t('ai.review.svc.already_mode', { mode: START_LABELS[x.op] });
      else if (x.op === 'stop' && s.state === 'stopped') blocked = t('ai.review.svc.already_stopped');
      else if (x.op === 'start' && s.state === 'running') blocked = t('ai.review.svc.already_running');
      return {
        key: x.key,
        label: s ? `${s.name} → ${OP_LABELS[x.op]}` : x.label,
        reason: x.reason || (s ? s.key1 : ''),
        icon: 'services',
        blocked,
        checked: !blocked && x.op !== 'disabled', // "Tắt hẳn" phải tự tích
        svc: s,
        op: x.op,
        side: s ? `${START_LABELS[s.start_type] || s.start_type} · ${STATE_LABELS[s.state] || s.state}` : '',
      };
    });
  },
  confirmLabel: (o, n) => t('ai.review.svc.confirm') + (n ? ` (${fmtInt(n)})` : ''),
  summary: (chosen) => {
    const off = chosen.filter((r) => r.op === 'disabled').length;
    return off ? t('ai.review.svc.summary_off', cnt(chosen.length, { off: fmtInt(off) })) : t('ai.review.svc.summary', cnt(chosen.length));
  },
  async run(chosen) {
    const out = new Map();
    for (const op of new Set(chosen.map((r) => r.op))) {
      const rows = chosen.filter((r) => r.op === op);
      const svcs = rows.map((r) => store.servicesById.get(r.svc.id) || r.svc);
      const res = op === 'stop' || op === 'start'
        ? await store.control(svcs, op, { quiet: true })
        : await store.setMode(svcs, op, { confirmed: true, quiet: true });
      for (const r of rows) {
        const e = res.get(r.svc.key1);
        out.set(r.key, e === '' ? { ok: true } : { ok: false, error: e || t('ai.review.no_result') });
      }
    }
    return out;
  },
  after(ok, fail) {
    if (ok.length) toasts.success(t('ai.review.svc.ok', cnt(ok.length)), ok.slice(0, 5).map((r) => r.label));
    if (fail.length) toasts.error(t('ai.review.svc.fail', cnt(fail.length)), fail.slice(0, 5).map((r) => `${r.label}: ${r.error}`));
  },
};

// ---------------- ket_thuc: kết thúc tiến trình ----------------
// Gom theo ỨNG DỤNG (tên exe): trình duyệt có hàng chục tiến trình, liệt kê từng PID trông như bị lặp. Mỗi app một
// hàng (icon exe, số tiến trình, tổng RAM, lý do một lần); bấm mũi tên để xem từng PID. Kết thúc = mọi PID không bị chặn.
const kill = {
  get title() { return t('ai.review.kill.title'); },
  plainLabel: true,
  get hint() { return t('ai.review.kill.hint'); },
  get doneText() { return t('ai.review.kill.done'); },
  get options() { return [{ id: 'tree', label: t('ai.review.kill.opt_tree') }]; },
  async load(items) {
    const found = (await api.procLookup(items.map((x) => x.target))) || [];
    const reasons = new Map(items.map((x) => [x.target, x.reason]));
    const groups = new Map();
    const seenPid = new Set();
    for (const p of found) {
      if (p.pid && seenPid.has(p.pid)) continue;
      if (p.pid) seenPid.add(p.pid);
      const key = (p.name || p.target).toLowerCase();
      let g = groups.get(key);
      if (!g) {
        g = { key, name: p.name || (/^\d+$/.test(p.target.trim()) ? `PID ${p.target.trim()}` : p.target), exe: p.exe, desc: p.desc, reasons: [], procs: [] };
        groups.set(key, g);
      }
      g.exe ||= p.exe;
      g.desc ||= p.desc;
      const why = reasons.get(p.target);
      if (why && !g.reasons.includes(why)) g.reasons.push(why);
      if (p.name) g.procs.push(p);
    }
    procs.wantIcons([...groups.values()].map((g) => g.exe).filter(Boolean));
    return [...groups.values()].map((g) => {
      const live = g.procs.filter((p) => !p.blocked);
      const mem = live.reduce((a, p) => a + (p.mem || 0), 0);
      const blocked = !g.procs.length ? t('ai.review.kill.not_found') : !live.length ? g.procs[0].blocked : '';
      const skipped = g.procs.length - live.length;
      return {
        key: g.key,
        label: g.name,
        reason: [g.desc && g.desc.toLowerCase() !== g.name.toLowerCase().replace(/\.exe$/, '') ? g.desc : '', g.reasons[0]].filter(Boolean).join(' · '),
        icon: 'app',
        exe: g.exe,
        blocked,
        checked: !blocked,
        pids: live.map((p) => p.pid),
        lookupTargets: [...new Set(live.map((p) => p.target))],
        mem,
        side: !g.procs.length
          ? ''
          : !live.length
            ? t('ai.review.kill.side', cnt(g.procs.length))
            : skipped
              ? t('ai.review.kill.side_skip', cnt(g.procs.length, { mem: fmtBytes(mem), skip: fmtInt(skipped) }))
              : t('ai.review.kill.side_mem', cnt(g.procs.length, { mem: fmtBytes(mem) })),
        // phần mở rộng: từng PID, nặng nhất trước
        details: g.procs
          .slice()
          .sort((a, b) => (b.mem || 0) - (a.mem || 0))
          .map((p) => ({ text: p.children ? t('ai.review.kill.detail_children', cnt(p.children, { pid: p.pid })) : `PID ${p.pid}`, side: fmtBytes(p.mem || 0), warn: p.blocked || '' })),
      };
    });
  },
  confirmLabel: (o, n) =>
    n
      ? t(o.tree ? 'ai.review.kill.confirm_tree_n' : 'ai.review.kill.confirm_n', cnt(n))
      : t(o.tree ? 'ai.review.kill.confirm_tree' : 'ai.review.kill.confirm'),
  summary: (chosen) =>
    t('ai.review.kill.summary', cnt(chosen.length, {
      procs: fmtInt(chosen.reduce((a, r) => a + r.pids.length, 0)),
      mem: fmtBytes(chosen.reduce((a, r) => a + r.mem, 0)),
    })),
  async run(chosen, o) {
    const fresh = (await api.procLookup([...new Set(chosen.flatMap((r) => r.lookupTargets))])) || [];
    const names = new Map(fresh.filter((p) => p.name && !p.blocked).map((p) => [p.pid, p.name.toLowerCase()]));
    const same = (r, pid) => names.get(pid) === r.label.toLowerCase();
    const pids = [...new Set(chosen.flatMap((r) => r.pids.filter((pid) => same(r, pid))))];
    const res = pids.length ? (await api.procKill(pids, !!o.tree)) || [] : [];
    const by = new Map(res.map((x) => [x.pid, x]));
    return new Map(chosen.map((r) => {
      // tiến trình con có thể đã thoát cùng tiến trình cha -> "không tìm thấy" không tính là lỗi
      // (so với lỗi của backend, tiếng Việt hoặc tiếng Anh)
      const gone = r.pids.filter((pid) => !same(r, pid)).length;
      const bad = r.pids.map((pid) => by.get(pid)).filter((x) => x && !x.ok && !/không tìm thấy|đã thoát|không còn|not found|exited|no longer/i.test(x.error || '')); // i18n-ignore: đọc chữ của model / backend
      const failed = gone + bad.length;
      const error = gone ? t('ai.review.kill.not_found') : bad[0]?.error;
      return [r.key, failed ? { ok: false, error: t('ai.review.kill.partial', cnt(r.pids.length, { bad: fmtInt(failed), error })) } : { ok: true }];
    }));
  },
  after(ok, fail) {
    if (ok.length) toasts.success(t('ai.review.kill.ok', cnt(ok.length)), ok.slice(0, 5).map((r) => t('ai.review.kill.ok_line', cnt(r.pids.length, { label: r.label }))));
    if (fail.length) toasts.error(t('ai.review.kill.fail', cnt(fail.length)), fail.slice(0, 5).map((r) => `${r.label}: ${r.error}`));
  },
};

// ---------------- go_cai: gỡ cài đặt một ứng dụng + dọn thư mục liên quan ----------------
// Mở từ trang Ứng dụng (apps.uninstall), không phải đề xuất của trợ lý. Hàng = thư mục cài đặt + thư mục liên quan
// (leftovers.rs::related). Thư mục được tích chỉ chuyển vào Thùng rác SAU KHI trình gỡ xong và app đã thật sự biến mất.
let goApp = null;
const fmtMonth = (ms) => (ms ? new Date(ms).toLocaleDateString(i18n.intl, { month: '2-digit', year: 'numeric' }) : '');
/** Một mục tìm thấy (Found) -> hàng popup. */
export function foundRow(f) {
  return {
    key: (f.package || f.path).toLowerCase(),
    label: f.path,
    reason: `${f.area} · ${f.shared ? t('ai.review.uninstall.shared', { name: f.shared }) : f.reason}`,
    icon: 'folder',
    blocked: f.kind === 'info' ? f.reason : '',
    checked: f.kind !== 'info' && f.sure,
    size: f.bytes,
    side: `${f.partial ? '≥ ' : ''}${fmtBytes(f.bytes)}${f.newest ? ` · ${t('ai.review.uninstall.modified', { date: fmtMonth(f.newest) })}` : ''}`,
  };
}
const uninstall = {
  title: (r) => t('ai.review.uninstall.title', { name: r.app.name }),
  hint: (r) =>
    r.app.kind === 'store'
      ? t('ai.review.uninstall.hint_store')
      : t('ai.review.uninstall.hint', { name: r.app.name }),
  allowEmpty: true,
  get emptyText() { return t('ai.review.uninstall.empty'); },
  get doneText() { return t('ai.review.uninstall.done'); },
  async load(items) {
    goApp = items[0].app;
    return (await api.appsRelated(goApp.id)).map(foundRow);
  },
  confirmLabel: (o, n) => (n ? t('ai.review.uninstall.confirm_n', cnt(n)) : t('ai.review.uninstall.confirm')),
  summary: (chosen) => t('ai.review.uninstall.summary', cnt(chosen.length, { size: fmtBytes(chosen.reduce((a, r) => a + (r.size || 0), 0)) })),
  async run(chosen) {
    await apps.start(goApp, chosen.map((r) => ({ path: r.label, bytes: r.size || 0 })));
    return new Map(chosen.map((r) => [r.key, { ok: true }]));
  },
};

export const REVIEW_KINDS = { go_cai: uninstall, xoa: del, dong_cong: ports, don_dep: clean, tat_khoi_dong: startup, dich_vu: services, ket_thuc: kill, bien_moi_truong: envVars };
