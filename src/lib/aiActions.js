// Đề xuất hành động của trợ lý AI – khung chung, thêm loại mới chỉ cần khai báo ở đây + một bộ xử lý ở reviewKinds.js.
//
// Luồng: model viết một khối mã có ngôn ngữ đã đăng ký (```xoa, ```dong_cong…), mỗi dòng `đích | lý do`.
// markdown.js vẽ khối thành THẺ (tối đa 4 dòng + nút "Xem lại…", data-act / data-idx). Bấm nút -> AiPanel đặt
// ai.review = { kind, items } -> ActionReview.svelte mở popup: kiểm tra lại từng mục với backend, người dùng tự tick,
// rồi mới thực hiện. Model KHÔNG BAO GIỜ tự thao tác; backend luôn kiểm tra lại lúc thực hiện.
//
// Mỗi loại: { title(n), button, parse(target) -> { key, label } | null }
//   key: khoá duy nhất (bỏ trùng), label: chữ hiển thị trong thẻ.

import { t, labels } from './i18n.svelte.js';
import { fmtInt } from './format.js';

export const ACTION_KINDS = {
  // Xoá thư mục / tệp (trang Ổ cứng). Đích: đường dẫn đầy đủ.
  xoa: {
    title: (n) => t('ai.act.xoa.title', { count: n, n: fmtInt(n) }),
    get button() { return t('ai.act.xoa.button'); },
    parse: (x) => {
      const path = x.replace(/^`+|`+$/g, '').trim();
      return path ? { key: path.toLowerCase(), label: path, path } : null;
    },
  },
  // Đóng cổng = kết thúc chương trình đang giữ cổng (trang Cổng). Đích: số cổng.
  dong_cong: {
    title: (n) => t('ai.act.dong_cong.title', { count: n, n: fmtInt(n) }),
    get button() { return t('ai.act.dong_cong.button'); },
    parse: (x) => {
      const m = x.replace(/`/g, '').match(/^(?:(?:cổng|port)\s*)?:?(\d{1,5})\b/i); // i18n-ignore: đọc chữ của model / backend
      const port = m ? Number(m[1]) : 0;
      return port >= 1 && port <= 65535 ? { key: String(port), label: t('ai.act.port', { port }), port } : null;
    },
  },
  // Dọn dẹp (trang Dọn dẹp). Đích: mã mục trong khối <cleanup> (user_temp, cache:google chrome…).
  don_dep: {
    title: (n) => t('ai.act.don_dep.title', { count: n, n: fmtInt(n) }),
    get button() { return t('ai.act.don_dep.button'); },
    parse: (x) => {
      const id = x.replace(/[`[\]]/g, '').trim().toLowerCase();
      return /^[a-z0-9_]+$|^cache:.+$/.test(id) ? { key: id, label: id } : null;
    },
  },
  // Tắt mục khởi động cùng Windows (trang Khởi động). Đích: tên mục (hoặc mã mục).
  tat_khoi_dong: {
    title: (n) => t('ai.act.tat_khoi_dong.title', { count: n, n: fmtInt(n) }),
    get button() { return t('ai.act.tat_khoi_dong.button'); },
    parse: (x) => {
      const name = x.replace(/[`[\]"]/g, '').trim();
      return name ? { key: name.toLowerCase(), label: name, name } : null;
    },
  },
  // Đổi dịch vụ (trang Dịch vụ). Đích: `tên dịch vụ: thao tác`.
  dich_vu: {
    title: (n) => t('ai.act.dich_vu.title', { count: n, n: fmtInt(n) }),
    get button() { return t('ai.act.dich_vu.button'); },
    parse: (x) => {
      const m = x.replace(/[`"]/g, '').match(/^(.+?)\s*(?::|->|→|=)\s*([^:→=]+)$/);
      const op = m && SERVICE_OPS[norm(m[2])];
      const name = m?.[1].trim();
      return op && name ? { key: `${name.toLowerCase()}:${op}`, label: `${name}: ${OP_LABELS[op]}`, name, op } : null;
    },
  },
  // Kết thúc tiến trình (trang Tiến trình). Đích: PID hoặc tên exe (mọi tiến trình cùng tên).
  ket_thuc: {
    title: (n) => t('ai.act.ket_thuc.title', { count: n, n: fmtInt(n) }),
    get button() { return t('ai.act.ket_thuc.button'); },
    parse: (x) => {
      const s = x.replace(/[`"]/g, '').trim();
      const pid = s.match(/^(?:pid\s*)?(\d{1,7})\b/i);
      if (pid) return { key: pid[1], label: `PID ${pid[1]}`, target: pid[1] };
      const exe = s.match(/^([\w .+-]+\.exe)\b/i);
      return exe ? { key: exe[1].toLowerCase(), label: exe[1], target: exe[1] } : null;
    },
  },
};

// thao tác dịch vụ: chữ trợ lý viết (không dấu / có dấu / tiếng Anh) -> mã
const norm = (s) => s.trim().toLowerCase().normalize('NFD').replace(/[̀-ͯ]/g, '').replace(/đ/g, 'd').replace(/[\s()-]+/g, '_').replace(/^_|_$/g, ''); // i18n-ignore: đọc chữ của model / backend
const SERVICE_OPS = {
  thu_cong: 'demand', manual: 'demand', demand: 'demand',
  tat_han: 'disabled', tat: 'disabled', disabled: 'disabled', disable: 'disabled',
  tu_dong: 'auto', auto: 'auto', automatic: 'auto',
  tu_dong_tre: 'delayed', tre: 'delayed', delayed: 'delayed', auto_delayed: 'delayed',
  dung: 'stop', stop: 'stop', dung_ngay: 'stop',
  chay: 'start', start: 'start',
};
export const OP_LABELS = labels({ demand: 'common.start.demand', disabled: 'ai.op.disabled', auto: 'common.start.auto', delayed: 'common.start.delayed', stop: 'ai.op.stop', start: 'ai.op.start' });

/** `- đích | lý do` -> { ...parse(đích), reason } | null */
export function parseActionLine(kind, line) {
  const t = line.trim().replace(/^[-*+]\s+/, '');
  if (!t) return null;
  const bar = t.indexOf('|');
  const target = (bar < 0 ? t : t.slice(0, bar)).trim();
  const reason = bar < 0 ? '' : t.slice(bar + 1).trim();
  const x = ACTION_KINDS[kind].parse(target);
  return x ? { ...x, reason } : null;
}

/** Các dòng của một khối -> mục (bỏ dòng hỏng, bỏ trùng theo key). */
export function parseActionItems(kind, lines) {
  const seen = new Set();
  const out = [];
  for (const l of lines) {
    const x = parseActionLine(kind, l);
    if (!x || seen.has(x.key)) continue;
    seen.add(x.key);
    out.push(x);
  }
  return out;
}

export const isActionLang = (lang) => Object.hasOwn(ACTION_KINDS, (lang || '').toLowerCase());
