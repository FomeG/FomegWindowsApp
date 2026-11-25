import { ACTION_KINDS, isActionLang, parseActionItems } from './aiActions.js';
import { t } from './i18n.svelte.js';
import { fmtInt } from './format.js';
// Trình hiển thị markdown nhỏ gọn và AN TOÀN cho câu trả lời của AI (không thêm thư viện).
// Luôn escape HTML trước, sau đó mới thêm các thẻ do chính file này tạo ra -> model không thể chèn HTML/JS.
// Hỗ trợ (kiểu GitHub): đoạn văn, tiêu đề #..######, **đậm**, *nghiêng*, ***cả hai***, ~~gạch~~, `code`,
// khối ``` / ~~~, danh sách - * + / 1. (lồng nhau, checklist [ ] [x]), trích dẫn > (lồng nhau), bảng | a | b |
// (căn lề :---:), đường kẻ ---, ký tự thoát \*.
// Liên kết [chữ](url) hiển thị dạng chữ (url ở tooltip), không tạo <a> để webview không bị điều hướng đi.

const ESC = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' };
export function escapeHtml(s) {
  return String(s).replace(/[&<>"']/g, (c) => ESC[c]);
}

// ---------------- trong dòng ----------------

const PH = '\u0000'; // đánh dấu chỗ giữ (code, link, ký tự thoát) để không bị định dạng lại

/** Định dạng trong dòng; đầu vào ĐÃ được escape. */
function inline(s) {
  const keep = [];
  const hold = (html) => PH + (keep.push(html) - 1) + PH;

  // code span: `a`, ``a ` b``
  s = s.replace(/(`+)([\s\S]*?[^`])\1(?!`)/g, (_, _t, c) => {
    const body = c.length > 2 && c.startsWith(' ') && c.endsWith(' ') ? c.slice(1, -1) : c;
    return hold('<code>' + body + '</code>');
  });
  // ký tự thoát: \* \_ \| ...
  s = s.replace(/\\([\\`*_{}\[\]()#+\-.!|~])/g, (_, c) => hold(c));
  // liên kết [chữ](url) -> chữ, url ở tooltip
  s = s.replace(/\[([^\]\n]+)\]\(\s*([^)\s]+)(?:\s+&quot;[^\n]*?&quot;)?\s*\)/g, (_, label, url) =>
    hold(`<span class="md-link" title="${url}">` + emphasis(label) + '</span>'),
  );
  s = emphasis(s);
  return s.replace(/\u0000(\d+)\u0000/g, (_, n) => keep[+n]);
}

function emphasis(s) {
  return s
    .replace(/\*\*\*(?=\S)([\s\S]*?\S)\*\*\*/g, '<strong><em>$1</em></strong>')
    .replace(/\*\*(?=\S)([\s\S]*?\S)\*\*/g, '<strong>$1</strong>')
    .replace(/(^|[^\w])__(?=\S)([\s\S]*?\S)__(?!\w)/g, '$1<strong>$2</strong>')
    .replace(/(^|[^*\w])\*([^\s*](?:[^*\n]*?[^\s*])?)\*(?![*\w])/g, '$1<em>$2</em>')
    .replace(/(^|[^\w])_([^\s_](?:[^_\n]*?[^\s_])?)_(?!\w)/g, '$1<em>$2</em>')
    .replace(/~~(?=\S)([\s\S]*?\S)~~/g, '<del>$1</del>');
}

// ---------------- khối ----------------

const RE_FENCE = /^ {0,3}(`{3,}|~{3,})\s*([\w+#.-]*)/;
const RE_HR = /^ {0,3}([-*_])(?:\s*\1){2,}\s*$/;
const RE_HEAD = /^ {0,3}(#{1,6})\s+(.*?)(?:\s+#+)?\s*$/;
const RE_QUOTE = /^ {0,3}&gt;\s?/;
const RE_ITEM = /^(\s*)([-*+]|\d{1,9}[.)])(\s+|$)(.*)$/;

const indentOf = (l) => l.match(/^\s*/)[0].replace(/\t/g, '    ').length;

/** Bỏ tối đa n khoảng trắng đầu dòng (tab tính 4). */
function dedent(l, n) {
  let i = 0, w = 0;
  while (i < l.length && w < n && (l[i] === ' ' || l[i] === '\t')) { w += l[i] === '\t' ? 4 : 1; i++; }
  return l.slice(i);
}

/** Tách một dòng bảng thành các ô (bỏ | ở hai đầu, không cắt | nằm trong `code` hoặc \|). */
function cells(line) {
  let t = line.trim();
  if (t.startsWith('|')) t = t.slice(1);
  if (t.endsWith('|') && !t.endsWith('\\|')) t = t.slice(0, -1);
  const out = [];
  let cur = '', tick = false;
  for (let i = 0; i < t.length; i++) {
    const c = t[i];
    if (c === '\\' && t[i + 1] === '|') { cur += '\\|'; i++; continue; }
    if (c === '`') tick = !tick;
    if (c === '|' && !tick) { out.push(cur.trim()); cur = ''; continue; }
    cur += c;
  }
  out.push(cur.trim());
  return out;
}
const isSep = (l) => l != null && l.includes('-') && /^\s*\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)*\|?\s*$/.test(l);
const isTableStart = (lines, i) => lines[i].includes('|') && isSep(lines[i + 1]) && cells(lines[i]).length === cells(lines[i + 1]).length;

/** Dòng này có mở đầu một khối khác không (dùng để kết thúc đoạn văn). */
function startsBlock(lines, i) {
  const l = lines[i];
  return RE_FENCE.test(l) || RE_HR.test(l) || RE_HEAD.test(l) || RE_QUOTE.test(l) || RE_ITEM.test(l) || isTableStart(lines, i);
}

function renderTable(lines, i, out) {
  const head = cells(lines[i]);
  const align = cells(lines[i + 1]).map((c) => (c.startsWith(':') && c.endsWith(':') ? 'center' : c.endsWith(':') ? 'right' : c.startsWith(':') ? 'left' : ''));
  const td = (tag, c, k) => `<${tag}${align[k] ? ` style="text-align:${align[k]}"` : ''}>` + inline(c) + `</${tag}>`;
  let html = '<div class="md-table"><table><thead><tr>' + head.map((c, k) => td('th', c, k)).join('') + '</tr></thead><tbody>';
  i += 2;
  while (i < lines.length && lines[i].trim() && lines[i].includes('|')) {
    const row = cells(lines[i]);
    html += '<tr>' + head.map((_, k) => td('td', row[k] ?? '', k)).join('') + '</tr>';
    i++;
  }
  out.push(html + '</tbody></table></div>');
  return i;
}

function renderList(lines, i, out) {
  const first = lines[i].match(RE_ITEM);
  const base = indentOf(first[1]);
  const ordered = /\d/.test(first[2]);
  const start = ordered ? parseInt(first[2], 10) : 1;
  const items = [];
  let loose = false;

  while (i < lines.length) {
    const m = lines[i].match(RE_ITEM);
    if (!m || indentOf(m[1]) > base + 1 || /\d/.test(m[2]) !== ordered) break;
    if (indentOf(m[1]) < base) break;
    // vị trí nội dung = thụt + ký hiệu + khoảng trắng; dòng con thụt từ đây trở vào
    const offset = indentOf(m[1]) + m[2].length + Math.max(1, Math.min(m[3].length, 4));
    const body = [m[4]];
    i++;
    let blank = false;
    while (i < lines.length) {
      const l = lines[i];
      if (!l.trim()) { blank = true; body.push(''); i++; continue; }
      const ind = indentOf(l);
      if (ind >= Math.min(offset, base + 2)) { // thuộc mục hiện tại (kể cả danh sách con)
        if (blank) loose = true;
        blank = false;
        body.push(dedent(l, offset));
        i++;
        continue;
      }
      if (blank) break; // dòng không thụt sau dòng trống -> hết mục
      if (RE_ITEM.test(l) || startsBlock(lines, i)) break;
      body.push(l.trim()); // dòng nối tiếp lười
      i++;
    }
    while (body.length && !body[body.length - 1].trim()) body.pop();
    if (!body.length) body.push(''); // mục rỗng (vd. "- " khi đang stream)
    // dòng trống giữa các mục -> danh sách "thưa"
    const next = blank && i < lines.length && lines[i].match(RE_ITEM);
    if (next && indentOf(next[1]) <= base + 1 && indentOf(next[1]) >= base && /\d/.test(next[2]) === ordered) loose = true;
    items.push(body);
  }

  const lis = items.map((body) => {
    let check = '';
    const t = body[0].match(/^\[([ xX])\]\s+(.*)$/);
    if (t) {
      check = `<span class="md-check${t[1] === ' ' ? '' : ' on'}" aria-hidden="true"></span>`;
      body = [t[2], ...body.slice(1)];
    }
    return `<li${check ? ' class="task"' : ''}>` + check + blocks(body, !loose) + '</li>';
  });
  out.push(ordered ? `<ol${start !== 1 ? ` start="${start}"` : ''}>` + lis.join('') + '</ol>' : '<ul>' + lis.join('') + '</ul>');
  return i;
}

/** Danh sách dòng (đã escape) -> HTML. tight = trong mục danh sách sát nhau, đoạn văn không bọc <p>. */
function blocks(lines, tight = false) {
  const out = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (!line.trim()) { i++; continue; }

    const fence = line.match(RE_FENCE);
    if (fence) {
      const mark = fence[1];
      const close = new RegExp('^ {0,3}' + (mark[0] === '`' ? '`' : '~') + '{' + mark.length + ',}\\s*$');
      const ind = indentOf(line);
      const buf = [];
      i++;
      while (i < lines.length && !close.test(lines[i])) buf.push(dedent(lines[i++], ind));
      const closed = i < lines.length;
      i++; // bỏ dòng đóng (khối chưa đóng khi đang stream vẫn hiển thị)
      if (isActionLang(fence[2])) {
        out.push(actionCard(fence[2].toLowerCase(), buf, closed, actSeq++));
        continue;
      }
      out.push('<pre class="md-code"' + (fence[2] ? ` data-lang="${fence[2]}"` : '') + '><code>' + buf.join('\n') + '</code></pre>');
      continue;
    }
    if (RE_HR.test(line)) { // --- *** ___ (ưu tiên hơn danh sách như CommonMark)
      out.push('<hr>');
      i++;
      continue;
    }
    const h = line.match(RE_HEAD);
    if (h) {
      const lv = Math.min(6, h[1].length + 2); // # -> h3 (cỡ chữ vừa với khung chat)
      out.push(`<h${lv}>` + inline(h[2]) + `</h${lv}>`);
      i++;
      continue;
    }
    if (isTableStart(lines, i)) { i = renderTable(lines, i, out); continue; }
    if (RE_QUOTE.test(line)) {
      const inner = [];
      while (i < lines.length && lines[i].trim() && (RE_QUOTE.test(lines[i]) || !startsBlock(lines, i))) {
        inner.push(lines[i].replace(RE_QUOTE, ''));
        i++;
      }
      out.push('<blockquote>' + blocks(inner) + '</blockquote>');
      continue;
    }
    if (RE_ITEM.test(line)) {
      i = renderList(lines, i, out);
      continue;
    }
    // đoạn văn: gom tới dòng trống hoặc khối khác
    const para = [line.trim()];
    i++;
    while (i < lines.length && lines[i].trim() && !startsBlock(lines, i)) para.push(lines[i++].trim());
    const html = inline(para.join('\n')).replace(/\n/g, '<br>');
    out.push(tight ? html : '<p>' + html + '</p>');
  }
  return out.join('');
}

// ---------------- đề xuất hành động của trợ lý (khối ```xoa, ```dong_cong… – xem aiActions.js) ----------------
// Thẻ chỉ có nút "Xem lại…" (data-act = loại, data-idx = thứ tự khối hành động trong tin nhắn);
// việc thực hiện luôn qua popup chọn từng mục của ứng dụng, model không tự thao tác được.
let actSeq = 0;

function actionCard(kind, buf, closed, idx) {
  // buf đã được escape HTML (renderMarkdown escape trước) -> nhãn và lý do an toàn khi ghép chuỗi
  const k = ACTION_KINDS[kind];
  const items = parseActionItems(kind, buf);
  const shown = items.slice(0, 4).map((x) => `<li><code>${x.label}</code>${x.reason ? ` <span class="muted">– ${x.reason}</span>` : ''}</li>`).join('');
  const more = items.length > 4 ? `<li class="muted">${t('ai.card.more', { count: items.length - 4, n: fmtInt(items.length - 4) })}</li>` : '';
  const action = closed
    ? `<button type="button" class="btn sm danger" data-act="${kind}" data-idx="${idx}">${k.button}</button>`
    : `<div class="muted small">${t('ai.card.drafting')}</div>`;
  return `<div class="md-del"><div class="md-del-head">${k.title(items.length)}</div><ul>${shown}${more}</ul>${action}</div>`;
}

/** Các khối hành động đã đóng trong câu trả lời (chuỗi thô) -> [{ kind, items }], đúng thứ tự data-idx. */
export function parseActionBlocks(src) {
  const out = [];
  const lines = String(src || '').replace(/\r\n?/g, '\n').split('\n');
  for (let i = 0; i < lines.length; i++) {
    // thụt lề bất kỳ (khối nằm trong danh sách) để thứ tự khớp với các thẻ đã hiển thị
    const m = lines[i].match(/^\s*(`{3,}|~{3,})\s*([\w+#.-]*)/);
    if (!m) continue;
    const close = new RegExp('^\\s*' + (m[1][0] === '`' ? '`' : '~') + '{' + m[1].length + ',}\\s*$');
    const buf = [];
    let j = i + 1;
    while (j < lines.length && !close.test(lines[j])) buf.push(lines[j++]);
    if (isActionLang(m[2])) {
      const kind = m[2].toLowerCase();
      out.push({ kind, items: parseActionItems(kind, buf) });
    }
    i = j;
  }
  return out;
}

/** markdown (chuỗi thô từ model) -> HTML an toàn. */
export function renderMarkdown(src) {
  actSeq = 0;
  const lines = escapeHtml(src || '').replace(/\r\n?/g, '\n').split('\n');
  return blocks(lines);
}
