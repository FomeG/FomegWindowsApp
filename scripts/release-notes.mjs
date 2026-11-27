// Ghi chú phát hành cho một version: node scripts/release-notes.mjs [version]   (npm run notes = bản trong package.json)
// release.yml dùng kết quả làm nội dung GitHub Release; latest.json mang lại nội dung đó nên thanh cập nhật trong app
// hiện được (chỉ phần đúng ngôn ngữ, nhờ các dấu <!-- vi --> / <!-- en --> / <!-- end -->).
//
// Nguồn: mục "## [<version>]" của CHANGELOG.vi.md (phần trên) và CHANGELOG.md (phần dưới), Keep a Changelog, phân tích bằng
// src/lib/changelog.js (cùng bộ phân tích với app). Thiếu một ngôn ngữ -> dùng tạm ngôn ngữ kia; thiếu cả hai -> nhóm các
// commit feat/perf/fix kể từ tag trước (dự phòng, nên viết tay). Phần cuối (cài đặt, SHA-256 do release.yml điền) cố định.
import { readFileSync, existsSync } from 'node:fs';
import { execSync } from 'node:child_process';
import { parseChangelog } from '../src/lib/changelog.js';

const ver = process.argv[2] || JSON.parse(readFileSync('package.json', 'utf8')).version;
const setup = `fomeg-windows-app_${ver}_x64-setup.exe`;

function fromCommits() {
  const sh = (c) => {
    try { return execSync(c, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim(); } catch { return ''; }
  };
  const tags = sh('git tag --list "v*" --sort=-v:refname').split('\n').filter((t) => t && t !== `v${ver}`);
  const log = sh(`git log --no-merges --pretty=%s ${tags[0] ? `${tags[0]}..HEAD` : 'HEAD'} -- src src-tauri mcp`);
  const groups = { feat: [], perf: [], fix: [] };
  for (const s of new Set(log.split('\n'))) {
    const m = s.match(/^(feat|fix|perf)(\([^)]*\))?!?:\s*(.+)/i);
    if (m) groups[m[1].toLowerCase()].push(`- ${m[3][0].toUpperCase()}${m[3].slice(1)}`);
  }
  const body = (names) =>
    [['feat', names[0]], ['perf', names[1]], ['fix', names[2]]]
      .filter(([k]) => groups[k].length)
      .map(([k, title]) => `### ${title}\n\n${groups[k].slice(0, 15).join('\n')}`)
      .join('\n\n');
  const vi = body(['Thêm mới', 'Thay đổi', 'Sửa lỗi']) || '- Cải thiện và sửa lỗi nhỏ.'; // i18n-ignore
  const en = body(['Added', 'Changed', 'Fixed']) || '- Minor fixes and improvements.';
  return { vi: { summary: '', body: vi }, en: { summary: '', body: en } };
}

const entry = (file) => (existsSync(file) ? parseChangelog(readFileSync(file, 'utf8')).find((e) => e.version === ver) : null);
const vi0 = entry('CHANGELOG.vi.md');
const en0 = entry('CHANGELOG.md');
const n = vi0 || en0 ? { vi: vi0 ?? en0, en: en0 ?? vi0 } : fromCommits();
// "### Added" trong tệp -> "#### Added" dưới tiêu đề "## <tóm tắt>" của mỗi ngôn ngữ trên trang phát hành
const down = (s) => s.replace(/^###\s/gm, '#### ');

console.log(
  [
    '<!-- vi -->',
    `## ${n.vi.summary.replace(/\.$/, '') || 'Có gì mới'}`, // i18n-ignore
    '',
    down(n.vi.body),
    '',
    '<!-- en -->',
    `## ${n.en.summary.replace(/\.$/, '') || 'What’s new'}`,
    '',
    down(n.en.body),
    '<!-- end -->',
    '',
    '---',
    '',
    '### Cài đặt · Install',
    '', // i18n-ignore (3 dòng dưới)
    `- **Cài lần đầu:** tải \`${setup}\` bên dưới rồi chạy (cài vào Program Files, cần quyền Admin). Nếu SmartScreen báo "Windows protected your PC", chọn **More info → Run anyway**.`,
    '- **Đã cài:** app tự báo khi có bản mới và cập nhật trong một cú bấm (hoặc Cài đặt → Thông tin → Kiểm tra ngay).',
    `- **First install:** download \`${setup}\` below and run it (installs into Program Files, needs admin rights). If SmartScreen shows "Windows protected your PC", choose **More info → Run anyway**.`,
    '- **Already installed:** the app tells you about new versions and updates in one click (or Settings → About → Check now).',
    '<!-- sha256 -->',
  ].join('\n'),
);
