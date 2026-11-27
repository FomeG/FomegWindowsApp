// Tăng version: npm run bump 0.3.1   (hoặc: npm run bump patch | minor | major)
// Nguồn duy nhất là package.json (tauri.conf.json và giao diện đọc từ đó). Script còn sửa version của 2 crate Rust
// (src-tauri, mcp) và Cargo.lock (chung cho workspace) / package-lock.json để CI không phải sửa gì. Giữ nguyên CRLF / LF của từng tệp.
import { readFileSync, writeFileSync, existsSync } from 'node:fs';

const arg = process.argv[2];
const pkg = JSON.parse(readFileSync('package.json', 'utf8'));
const cur = pkg.version;
const SEMVER = /^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/;

function next(v, kind) {
  const [a, b, c] = v.split('-')[0].split('.').map(Number);
  if (kind === 'major') return `${a + 1}.0.0`;
  if (kind === 'minor') return `${a}.${b + 1}.0`;
  return `${a}.${b}.${c + 1}`;
}
const to = ['patch', 'minor', 'major'].includes(arg) ? next(cur, arg) : arg;
if (!to || !SEMVER.test(to)) {
  console.error(`Cách dùng: npm run bump <x.y.z | patch | minor | major>   (hiện tại: ${cur})`);
  process.exit(1);
}
const cmp = (x, y) => x.split('-')[0].split('.').map(Number).reduce((r, n, i) => r || n - y.split('-')[0].split('.').map(Number)[i], 0);
if (cmp(to, cur) <= 0) {
  console.error(`Version mới (${to}) phải lớn hơn version hiện tại (${cur}): người dùng chỉ nhận bản lớn hơn.`);
  process.exit(1);
}

/** Sửa một tệp: hàm f nhận nội dung (đã đổi CRLF -> LF), trả nội dung mới. Không đổi được thì báo lỗi. */
function edit(file, f) {
  if (!existsSync(file)) return;
  const raw = readFileSync(file, 'utf8');
  const crlf = raw.includes('\r\n');
  const src = raw.replace(/\r\n/g, '\n');
  const out = f(src);
  if (out === src) throw new Error(`Không tìm thấy version ${cur} trong ${file}`);
  writeFileSync(file, crlf ? out.replace(/\n/g, '\r\n') : out);
  console.log(`  ${file}`);
}

// version trong [package] của Cargo.toml (dòng version đầu tiên sau [package])
const cargoToml = (s) => s.replace(/(\[package\][^[]*?\nversion\s*=\s*")([^"]+)(")/, `$1${to}$3`);
// mục [[package]] name = "<crate>" trong Cargo.lock
const cargoLock = (names) => (s) =>
  names.reduce((acc, n) => acc.replace(new RegExp(`(\\[\\[package\\]\\]\\nname = "${n}"\\nversion = ")([^"]+)(")`), `$1${to}$3`), s);

console.log(`Đổi version ${cur} -> ${to}:`);
edit('package.json', (s) => s.replace(/("version":\s*")[^"]+(")/, `$1${to}$2`));
// package-lock.json: version ở gốc và ở packages[""]
edit('package-lock.json', (s) => {
  const j = JSON.parse(s);
  j.version = to;
  if (j.packages?.['']) j.packages[''].version = to;
  return JSON.stringify(j, null, 2) + '\n';
});
edit('src-tauri/Cargo.toml', cargoToml);
edit('mcp/Cargo.toml', cargoToml);
edit('Cargo.lock', cargoLock(['fomeg-windows-app', 'fomeg-windows-app-mcp']));
// CHANGELOG.md + CHANGELOG.vi.md (Keep a Changelog): "## [Unreleased]" -> "## [to] - YYYY-MM-DD", thêm "## [Unreleased]"
// trống ở trên, sửa link so sánh cuối tệp. Unreleased chưa có gì -> vẫn tạo mục version (trống: release lấy commit).
const date = new Date().toISOString().slice(0, 10);
const esc = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
let emptyNotes = false;
for (const file of ['CHANGELOG.md', 'CHANGELOG.vi.md']) {
  // đã có mục version này (viết tay) -> để nguyên
  if (!existsSync(file) || new RegExp(`^## \\[${esc(to)}\\]`, 'm').test(readFileSync(file, 'utf8'))) continue;
  edit(file, (s) => {
    const un = s.match(/^## \[Unreleased\][^\n]*\n([\s\S]*?)(?=^## |^\[[^\]]+\]:|(?![\s\S]))/im);
    if (un && !/^[-*]\s+\S/m.test(un[1])) emptyNotes = true;
    let out = un
      ? s.replace(/^## \[Unreleased\][^\n]*\n/im, `## [Unreleased]\n\n## [${to}] - ${date}\n`)
      : s.replace(/^(## )/m, `## [${to}] - ${date}\n\n$1`);
    // [unreleased]: …/compare/vCUR...HEAD -> vTO...HEAD, thêm [to]: …/compare/vCUR...vTO
    out = out.replace(/^\[unreleased\]:\s*(\S+)\/compare\/v([^.\s]+\.[^.\s]+\.[^.\s]+)\.\.\.HEAD$/im, (_m, base, prev) =>
      `[unreleased]: ${base}/compare/v${to}...HEAD\n[${to}]: ${base}/compare/v${prev}...v${to}`);
    return out;
  });
}
console.log(
  `\nXong. Tiếp theo:\n` +
    (emptyNotes ? `  ⚠ [Unreleased] đang trống: viết mục [${to}] trong CHANGELOG.vi.md + CHANGELOG.md (không thì release lấy tiêu đề commit)\n` : '') +
    `  xem trước ghi chú: npm run notes\n` +
    `  git commit -am "chore: release v${to}"\n  git push   rồi mở PR dev -> main; merge xong CI tự build và phát hành v${to}.`,
);
