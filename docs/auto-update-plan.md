# Kế hoạch: tự cập nhật trong app

Mục tiêu: người dùng cài app **một lần** bằng `setup.exe`. Từ đó, mỗi lần bạn merge một bản mới vào `main`, app của họ hiện thông báo. Họ chọn **Cập nhật**, **Để sau** hoặc **Bỏ qua bản này**. Bấm Cập nhật thì app tải bản mới, đóng lại, cài đè và tự mở lại. Họ không phải lên GitHub tải tay nữa.

Trạng thái: **mới là kế hoạch, chưa có code nào.**

---

## 1. Stack

| Phần | Dùng gì | Giá |
|---|---|---|
| Bộ cài Windows | **NSIS** (Tauri tự tải lúc build, ra `…-setup.exe`) | Miễn phí, mã nguồn mở |
| Cập nhật trong app | **`tauri-plugin-updater`** (plugin chính thức của Tauri 2, gọi từ Rust) | Miễn phí |
| Chữ ký bản update | Cặp khoá của Tauri (`npx tauri signer generate`) | Miễn phí |
| Nơi chứa bản phát hành | **GitHub Releases** (`setup.exe`, `.sig`, `latest.json`) | Miễn phí |
| Build + phát hành tự động | **GitHub Actions** + `tauri-apps/tauri-action` trên `windows-latest` | Miễn phí với repo public |
| Chữ ký Authenticode (tuỳ chọn, để bớt cảnh báo SmartScreen) | Azure Trusted Signing / SignPath | Có phí (SignPath miễn phí cho mã nguồn mở) |

Không cần thêm thư viện JS. Giao diện gọi command Rust của app như mọi trang khác (`api.js`).

---

## 2. Người dùng thấy gì

1. Mở app như bình thường. Khoảng 10 giây sau, app kiểm tra bản mới ở nền. Sau đó cứ 6 giờ kiểm tra lại một lần, nhưng chỉ khi cửa sổ đang mở.
2. Có bản mới thì hiện một dải thông báo mỏng ở đầu vùng nội dung (giống dải "không có quyền Admin"):

   ```
   ⓘ Có bản mới 0.3.1 (bạn đang dùng 0.3.0) · Có gì mới ▾        [Bỏ qua bản này] [Để sau] [Cập nhật]
   ```
   - **Có gì mới**: mở ra nội dung của GitHub Release, hiển thị bằng `markdown.js`.
   - **Để sau**: ẩn đến lần mở app sau.
   - **Bỏ qua bản này**: không báo bản `0.3.1` nữa (lưu `update:skip`). Khi có `0.3.2` thì báo lại.
   - **Cập nhật**: dải thông báo đổi thành thanh tiến trình kiểu `CleanupProgress`, ví dụ "Đang tải 4.2 / 9.8 MB", rồi chuyển sang "Đang cài…".
3. App đóng, bộ cài chạy ở chế độ passive (chỉ có thanh tiến trình, không bấm Next), rồi tự mở lại app.
4. Vì app đòi quyền Admin, lúc mở lại Windows sẽ hỏi UAC **như mỗi lần mở app bình thường**.

Trang **Cài đặt → Cập nhật & hiệu năng** có nhóm "Cập nhật", gồm các `OptionRow`:
- **Tự kiểm tra bản mới**: công tắc, mặc định bật. Hiện ở mục Thông tin đang là dòng `soon`; chuyển nó về đây và làm thật.
- **Kiểm tra ngay**: nút bấm. Dòng phụ ghi "Đang dùng 0.3.0 · kiểm tra lúc 14:20 · đã là bản mới nhất". Kiểm tra bằng tay thì vẫn báo cả bản đã "Bỏ qua".
- Ở bản tự build (không có repo hoặc khoá nhúng vào), hai dòng này bị khoá, kèm mô tả "Bản tự build, không tự cập nhật".

---

## 3. Thiết kế kỹ thuật

### 3.1 Backend: `src-tauri/src/update.rs` (mới)

- **Không ghi repo và khoá cứng trong code.** CI nhúng chúng lúc build qua `option_env!`:
  - `FOMEG_UPDATE_REPO`: `owner/repo`, lấy từ `${{ github.repository }}`.
  - `FOMEG_UPDATE_PUBKEY`: nội dung tệp `.key.pub`, lưu ở GitHub Variables.

  Thiếu một trong hai thì `enabled: false`. Nhờ vậy bản `npm run tauri build` trên máy bạn không bao giờ tự cập nhật thành bản cài đặt.
- Đăng ký plugin: `tauri_plugin_updater::Builder::new().pubkey(PUBKEY).build()`.
  ⚠ `tauri.conf.json` **bắt buộc** phải có `plugins.updater`, vì plugin đòi có trường `pubkey`, thiếu là app lỗi lúc khởi động. Đặt như sau:
  ```json
  "plugins": { "updater": { "pubkey": "", "windows": { "installMode": "passive" } } }
  ```
- Ba command (chạy async, không chặn webview):

  | Command | Trả về |
  |---|---|
  | `update_info()` | `{ enabled, version, repo }` |
  | `update_check()` | `null` (đang là bản mới nhất), hoặc `{ version, current, notes, date_ms }`. Đặt endpoint `https://github.com/{repo}/releases/latest/download/latest.json`, timeout 30 s. Giữ đối tượng `Update` trong `State<Pending>` để lúc cài đúng bản người dùng đã thấy. |
  | `update_install()` | Gọi `download_and_install`, phát sự kiện `update_progress { stage: download\|install, got, total }` tối đa khoảng 7 lần/giây. Thành công thì plugin chạy bộ cài bằng `ShellExecuteW` rồi `exit(0)`, nên command không bao giờ trả về. Lỗi thì trả `Err`, giữ lại bản `Update` để bấm thử lại, và ghi `util::log_line`. |

- Plugin tự kiểm chữ ký trước khi cài. Bản bị tráo hoặc hỏng thì bị từ chối.

### 3.2 Frontend

- `src/lib/update.svelte.js` (store, có `import.meta.hot.decline()`):
  - Trạng thái: `info`, `avail`, `checking`, `error`, `lastCheck`, `phase` (idle | download | install), `got`, `total`, `hidden`.
  - `init()` được gọi trong `App.svelte`. Nó hẹn kiểm tra sau 10 giây và mỗi 6 giờ, bỏ qua nếu `document.hidden` hoặc đã tắt tự kiểm tra.
  - Các hàm: `check({ manual })`, `later()`, `skip()` (`savePref('update:skip', version)`), `install()`.
- `src/lib/UpdateBar.svelte`: dải thông báo và thanh tiến trình. Dùng lại class `.banner` và kiểu `.cp-track`/`.cp-fill` (thanh 4 px chạy bằng `transform`, không có hoạt ảnh liên tục). Thêm glyph `download` (Segoe Fluent `0xE896`) vào `glyphs.js`.
- `prefs.svelte.js`: thêm `updateAuto` (mặc định `true`).
- `api.js`: thêm `updateInfo`, `updateCheck`, `updateInstall`. `mock.js` giả lập có bản `0.3.1` kèm tiến trình tải giả, để xem giao diện trong trình duyệt.
- Cập nhật `docs/contract-v2.md` (command và sự kiện mới) và mục kiến trúc trong `CLAUDE.md`.

### 3.3 Cấu hình build phát hành: `src-tauri/tauri.release.conf.json` (mới)

Lệnh build trên máy giữ nguyên (chỉ ra exe portable, nhanh). Chỉ CI mới dùng tệp này qua `--config`:

```json
{
  "bundle": {
    "active": true,
    "targets": ["nsis"],
    "createUpdaterArtifacts": true,
    "windows": { "nsis": { "installMode": "perMachine" } }
  }
}
```

- **Vì sao `perMachine` (cài vào Program Files):** app chạy bằng quyền Admin. Nếu exe nằm trong `%LOCALAPPDATA%`, bất kỳ chương trình thường nào cũng ghi đè được nó, rồi đợi bạn mở app với quyền Admin. Đó là lỗ hổng leo thang quyền. Exe trong Program Files thì chỉ Admin mới sửa được.
- `createUpdaterArtifacts` cần `TAURI_SIGNING_PRIVATE_KEY` lúc build, nên chỉ bật trong CI.
- Bộ cài sẽ bằng tiếng Anh: NSIS của Tauri không có sẵn bản dịch tiếng Việt. Trong app vẫn là tiếng Việt.
- Có thể thêm script `npm run build:setup` để build bộ cài trên máy khi cần thử.

---

## 4. CI/CD: `.github/workflows/release.yml`

Nguyên tắc: **chỉ ra bản mới khi version thay đổi.** Merge sửa lỗi chính tả không làm người dùng nhận thông báo.

```yaml
name: Phát hành
on:
  push:
    branches: [main]
    paths: ['src/**', 'src-tauri/**', 'mcp/**', 'package.json', 'package-lock.json']
  workflow_dispatch:

permissions:
  contents: write

jobs:
  check:
    runs-on: ubuntu-latest
    outputs:
      version: ${{ steps.v.outputs.version }}
      release: ${{ steps.v.outputs.release }}
    steps:
      - uses: actions/checkout@v4
      - id: v
        env: { GH_TOKEN: '${{ github.token }}' }
        run: |
          ver=$(jq -r .version src-tauri/tauri.conf.json)
          echo "version=$ver" >> "$GITHUB_OUTPUT"
          if gh release view "v$ver" >/dev/null 2>&1; then echo "release=false"; else echo "release=true"; fi >> "$GITHUB_OUTPUT"

  release:
    needs: check
    if: needs.check.outputs.release == 'true'
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
        with: { fetch-depth: 0 }
      - uses: actions/setup-node@v4
        with: { node-version: 22, cache: npm, cache-dependency-path: package-lock.json }
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with: { workspaces: 'src-tauri -> target' }
      - run: npm ci
        working-directory: tauri
      - name: Ghi chú phát hành
        shell: bash
        run: |
          prev=$(git describe --tags --abbrev=0 2>/dev/null || true)
          { echo "## Có gì mới"; git log --no-merges --pretty='- %s' ${prev:+$prev..}HEAD -- tauri | head -30; } > notes.md
      - uses: tauri-apps/tauri-action@v0
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
          TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
          FOMEG_UPDATE_REPO: ${{ github.repository }}
          FOMEG_UPDATE_PUBKEY: ${{ vars.TAURI_UPDATER_PUBKEY }}
        with:
          projectPath: tauri
          args: --config src-tauri/tauri.release.conf.json
          tagName: v__VERSION__
          releaseName: 'v__VERSION__'
          releaseBodyPath: notes.md   # kiểm tra lại tên tham số theo bản tauri-action đang dùng
          releaseDraft: false
          includeUpdaterJson: true
```

Ghi chú:
- `beforeBuildCommand` đã chạy `build:mcp`, nên `fomeg-windows-app-mcp.exe` vẫn được nhúng như hiện nay.
- Trên Release cần có: `…_x64-setup.exe`, `…_x64-setup.exe.sig` và `latest.json` (do tauri-action tạo).
- `releases/latest/download/latest.json` chỉ trỏ tới bản **đã phát hành, không phải pre-release**. Muốn thử trước thì dùng `releaseDraft: true`, kiểm tra xong rồi mới bấm Publish.
- PR chưa merge có thể chạy thêm một job kiểm tra build (`cargo check`, `npm run build`). Job này không phát hành.

### Tăng version: `scripts/bump.mjs` (mới)

Version hiện nằm ở **5 chỗ**: `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, `mcp/Cargo.toml`, `package.json` và `src/lib/version.js` (`APP_VERSION`). Script `npm run bump 0.3.1` sửa cả năm chỗ (giữ nguyên CRLF) để không bỏ sót. Nên cân nhắc để giao diện lấy version từ `update_info` thay cho `version.js`, để bớt một chỗ phải sửa.

---

## 5. Việc bạn tự làm một lần

1. Tạo repo GitHub và đẩy thư mục dự án lên (thư mục này hiện chưa là git repo). Nên để **public** thì GitHub Actions miễn phí và người dùng tải được bản phát hành.
2. Tạo khoá ký update trên máy bạn:
   ```powershell
   npx tauri signer generate -w "$env:USERPROFILE\.tauri\fomeg-updater.key"
   ```
   Đặt mật khẩu khi được hỏi. Lệnh tạo ra `fomeg-updater.key` (bí mật) và `fomeg-updater.key.pub` (công khai).
3. Trên GitHub, vào repo → Settings → Secrets and variables → Actions:
   - **Secrets**: `TAURI_SIGNING_PRIVATE_KEY` (nội dung tệp `.key`) và `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
   - **Variables**: `TAURI_UPDATER_PUBKEY` (nội dung tệp `.key.pub`).
4. **Sao lưu tệp `.key` và mật khẩu ở chỗ an toàn.** Mất khoá thì các bản đã cài không nhận update mới được nữa, người dùng phải tự tải lại bộ cài.

## 6. Mỗi lần ra bản mới

1. `npm run bump 0.3.1`, commit, mở PR, merge vào `main`.
2. CI build khoảng 10–15 phút, rồi tạo Release `v0.3.1` kèm ghi chú.
3. Người dùng thấy thông báo ở lần kiểm tra tiếp theo (lúc mở app, hoặc trong vòng 6 giờ).

---

## 7. Hạn chế và rủi ro đã biết

- **SmartScreen**: lần tải `setup.exe` đầu tiên bằng trình duyệt vẫn bị cảnh báo "Unknown publisher" vì exe chưa có chữ ký Authenticode. Các lần update sau tải từ trong app, file không bị gắn dấu "tải từ Internet", nên thường không bị cảnh báo.
- **UAC khi mở lại sau cập nhật**: không tránh được, vì app đòi quyền Admin. Cần thử thật một lần xem bộ cài NSIS (`RunAsUser`) có mở lại app được không. Nếu không, người dùng tự mở lại app.
- **Bản portable / tự build** không tự cập nhật (`enabled: false`). Người dùng bản portable cũ cần cài `setup.exe` một lần.
- **Chống hạ version**: plugin có tuỳ chọn `requireSignedVersion` (buộc chữ ký ghi đúng version). Chỉ bật sau khi kiểm tra được rằng CLI dùng trong CI có ghi version vào chữ ký. Bật mà CLI không ghi thì mọi bản update sẽ bị từ chối.
- **Đang dọn dẹp hoặc gỡ ứng dụng** thì không nên cập nhật giữa chừng. Lúc đó nút Cập nhật nên hỏi lại qua `store.confirm`.

## 8. Cách kiểm tra

1. Build bản `0.3.0` có nhúng repo và khoá, cài bằng `setup.exe`.
2. Phát hành `0.3.1` lên repo (có thể là một repo thử).
3. Mở bản `0.3.0` và lần lượt kiểm tra: thông báo hiện lên, **Để sau** (mở lại app thì báo lại), **Bỏ qua bản này** (không báo nữa), **Kiểm tra ngay** trong Cài đặt (vẫn báo), **Cập nhật** (tải, cài, mở lại, đúng version `0.3.1`).
4. Thử tráo `setup.exe` trên Release mà không ký lại: app phải từ chối cài.
5. Tắt mạng rồi kiểm tra: phải có thông báo lỗi dễ hiểu, app không bị treo.
