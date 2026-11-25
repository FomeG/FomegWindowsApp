// Kiểm tra đa ngôn ngữ: node scripts/i18n-check.mjs [tệp…]  (chạy ở thư mục gốc)
// 1) biên dịch các .svelte đã cho (bắt lỗi cú pháp), 2) khoá t('…') thiếu ở vi / en,
// 3) khoá lệch giữa vi và en (toàn bộ từ điển), 4) dòng còn chữ tiếng Việt viết cứng (ngoài comment).
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join, relative, extname } from 'node:path';
import { pathToFileURL } from 'node:url';
import { compile } from 'svelte/compiler';

const ROOT = process.cwd();
const SRC = join(ROOT, 'src');
const VN = /[àáảãạăằắẳẵặâầấẩẫậèéẻẽẹêềếểễệìíỉĩịòóỏõọôồốổỗộơờớởỡợùúủũụưừứửữựỳýỷỹỵđ]/i;
const SKIP = /(^|[\\/])(i18n|paraglide)[\\/]|mock[^\\/]*\.js$|i18n\.svelte\.js$/;

function walk(dir, out = []) {
  for (const n of readdirSync(dir)) {
    const p = join(dir, n);
    if (statSync(p).isDirectory()) walk(p, out);
    else if (['.svelte', '.js'].includes(extname(p))) out.push(p);
  }
  return out;
}

async function dict(locale) {
  const files = readdirSync(join(SRC, 'lib/i18n', locale)).filter((f) => f !== 'index.js');
  const all = {};
  const owner = {};
  for (const f of files) {
    const mod = await import(pathToFileURL(join(SRC, 'lib/i18n', locale, f)).href + '?t=' + Date.now());
    for (const k of Object.keys(mod.default)) {
      if (k in all) console.log(`TRÙNG KHOÁ ${locale}: ${k} (${owner[k]} và ${f})`);
      all[k] = mod.default[k];
      owner[k] = f;
    }
  }
  return all;
}

/** Bỏ comment (// … và /* … *\/ và <!-- … -->) nhưng giữ số dòng. */
function stripComments(src) {
  return src
    .replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
    .replace(/<!--[\s\S]*?-->/g, (m) => m.replace(/[^\n]/g, ' '))
    .replace(/(^|[^:'"`\\])\/\/[^\n]*/g, (m, a) => a + ' '.repeat(m.length - a.length));
}

const args = process.argv.slice(2);
const files = (args.length ? args.map((a) => join(ROOT, a)) : walk(SRC)).filter((f) => !SKIP.test(relative(SRC, f)) || args.length);
const vi = await dict('vi');
const en = await dict('en');
let problems = 0;
const used = new Set();

for (const f of files) {
  const rel = relative(ROOT, f);
  const src = readFileSync(f, 'utf8');
  if (f.endsWith('.svelte')) {
    try {
      compile(src, { filename: f, generate: 'client' });
    } catch (e) {
      problems++;
      console.log(`LỖI BIÊN DỊCH ${rel}: ${e.message}`);
    }
  }
  for (const m of src.matchAll(/\bt\(\s*['"`]([a-z0-9_.]+)['"`]/g)) {
    const k = m[1];
    used.add(k);
    const has = (d) => k in d || `${k}_other` in d || `${k}_one` in d;
    if (!has(vi)) { problems++; console.log(`THIẾU vi  ${rel}: ${k}`); }
    if (!has(en)) { problems++; console.log(`THIẾU en  ${rel}: ${k}`); }
  }
  if (!SKIP.test(relative(SRC, f))) {
    const raw = src.split('\n');
    const lines = stripComments(src).split('\n');
    lines.forEach((l, i) => {
      // dòng cố ý đọc chữ tiếng Việt (regex đọc câu trả lời của model / lỗi backend): ghi chú i18n-ignore
      if (/i18n-ignore/.test(raw[i] || '')) return;
      if (VN.test(l) && !/console\.(log|warn|error|info)/.test(l)) {
        problems++;
        console.log(`CHỮ VIỆT ${rel}:${i + 1}: ${l.trim().slice(0, 140)}`);
      }
    });
  }
}

if (!args.length) {
  // số nhiều: 'x' ở ngôn ngữ này tương ứng 'x_one' / 'x_other' ở ngôn ngữ kia
  const base = (k) => k.replace(/_(one|other)$/, '');
  const has = (d, k) => k in d || base(k) in d || `${base(k)}_other` in d;
  for (const k of Object.keys(vi)) if (!has(en, k)) { problems++; console.log(`CHỈ CÓ vi: ${k}`); }
  for (const k of Object.keys(en)) if (!has(vi, k)) { problems++; console.log(`CHỈ CÓ en: ${k}`); }
}
console.log(problems ? `\n${problems} vấn đề.` : 'OK – không có vấn đề.');
process.exit(problems ? 1 : 0);
