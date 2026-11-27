// Nhật ký thay đổi theo Keep a Changelog 1.1.0: CHANGELOG.md (English) + CHANGELOG.vi.md (tiếng Việt), cùng cấu trúc.
// Một bộ phân tích dùng chung cho app (Cài đặt → Có gì mới, toast sau khi cập nhật, thanh cập nhật) và
// scripts/release-notes.mjs (nội dung GitHub Release). Trong app, mỗi tệp được nhúng lúc build (?raw) trong chunk riêng:
// chỉ tải tệp của ngôn ngữ đang chọn, khi mở tab / khi vừa cập nhật, không cần mạng.
// Tệp này không import gì của Svelte để Node chạy được.

/** Đoạn của một ngôn ngữ trong ghi chú phát hành (<!-- vi --> … <!-- en --> … <!-- end -->), bỏ phần chân.
 *  Ghi chú kiểu cũ (không có dấu) -> trả nguyên văn. */
export function notesFor(md, locale) {
  const s = md ?? '';
  const m = s.match(new RegExp(`<!--\\s*${locale}\\s*-->([\\s\\S]*?)<!--\\s*(?:vi|en|end)\\s*-->`));
  return (m ? m[1] : s).trim();
}

/** CHANGELOG(.vi).md -> [{ version, date, summary, body, bullets }] theo thứ tự trong tệp (mới nhất trước).
 *  "## [Unreleased]" -> version 'unreleased'. summary = đoạn văn (tuỳ chọn) ngay dưới tiêu đề version;
 *  body = các mục "### Added…" (markdown); bullets = mỗi gạch đầu dòng cấp 1 (rút gọn, cho toast). Mục trống bị bỏ. */
export function parseChangelog(md) {
  const src = md
    .replace(/\r\n/g, '\n')
    .replace(/<!--[\s\S]*?-->/g, '')
    .replace(/^\[[^\]]+\]:\s+\S+.*$/gm, ''); // định nghĩa link cuối tệp ([0.3.0]: https://…)
  const out = [];
  for (const block of src.split(/^## /m).slice(1)) {
    const nl = block.indexOf('\n');
    const head = (nl < 0 ? block : block.slice(0, nl)).trim();
    const hm = head.match(/^\[?(unreleased|v?\d+\.\d+\.\d+[^\]\s]*)\]?(?:\s*[–-]\s*(\S+))?/i);
    if (!hm) continue;
    const lines = (nl < 0 ? '' : block.slice(nl + 1)).split('\n').map((l) => l.trimEnd());
    let summary = '';
    const si = lines.findIndex((l) => l.trim());
    if (si >= 0 && !/^\s*(#|[-*]\s)/.test(lines[si])) {
      summary = lines[si].trim();
      lines.splice(si, 1);
    }
    const bullets = lines
      .filter((l) => /^[-*]\s+\S/.test(l))
      .map((l) => {
        const b = l.match(/^[-*]\s+\*\*(.+?)\*\*/);
        return (b ? b[1] : l.replace(/^[-*]\s+/, '')).replace(/[`*]/g, '').replace(/[.:]\s*$/, '').trim();
      });
    if (!bullets.length && !summary) continue; // mục trống (Unreleased chưa có gì / vừa bump)
    const body = lines.join('\n').replace(/\n{3,}/g, '\n\n').trim();
    out.push({ version: hm[1].toLowerCase().replace(/^v/, ''), date: hm[2] || '', summary, body, bullets });
  }
  return out;
}

const cache = {};
/** Các bản đã phát hành (bỏ Unreleased) của ngôn ngữ đang chọn; đọc + phân tích một lần cho mỗi ngôn ngữ. */
export function loadChangelog(locale) {
  const key = locale === 'en' ? 'en' : 'vi';
  // đường dẫn phải viết thẳng để Vite tách chunk
  cache[key] ??= (key === 'en' ? import('../../CHANGELOG.md?raw') : import('../../CHANGELOG.vi.md?raw'))
    .then((m) => parseChangelog(m.default).filter((e) => e.version !== 'unreleased'))
    .catch(() => []);
  return cache[key];
}
