# Tự cập nhật, phát hành và CI/CD

Người dùng cài app **một lần** bằng `…_x64-setup.exe`. Từ đó, mỗi lần bạn đưa một version mới vào `main`, GitHub Actions tự build và phát hành; app của họ hiện thông báo, họ chọn **Cập nhật**, **Để sau** hoặc **Bỏ qua bản này**. Bấm Cập nhật thì app tải bản mới, kiểm chữ ký, đóng lại, cài đè và tự mở lại.

Trạng thái: **đã làm xong** (code + CI). Còn các bước một lần ở mục 5 do bạn tự làm (tạo khoá ký, thêm secrets).

---

## 1. Stack

| Phần | Dùng gì |
|---|---|
| Bộ cài Windows | **NSIS** (Tauri tự tải lúc build), cài cho toàn máy vào Program Files |
| Cập nhật trong app | **`tauri-plugin-updater` 2** gọi từ Rust (`src-tauri/src/update.rs`), không dùng API JS của plugin |
| Chữ ký bản cập nhật | Cặp khoá minisign của Tauri (`npx tauri signer generate`) |
| Nơi chứa bản phát hành | **GitHub Releases**: `…_x64-setup.exe`, `…_x64-setup.exe.sig`, `latest.json` |
| Build + phát hành | **GitHub Actions**: `ci.yml` (kiểm tra) + `release.yml` (`tauri-apps/tauri-action@v0`) trên `windows-latest` |
| Phụ thuộc | Dependabot mở PR vào `dev` mỗi tháng (`.github/dependabot.yml`) |

## 2. Luồng nhánh

```
dev (push: không chạy gì)  ──►  PR dev → main  ──(PR: CI)──►  merge vào main
                                                    │
                                  version trong package.json đã có Release?
                                     ├─ rồi  → không làm gì (merge sửa chữ, tài liệu… không làm phiền người dùng)
                                     └─ chưa → build bộ cài có chữ ký → Release v<version> → app tự báo
```

- Làm việc trên `dev`; push lên `dev` **không** chạy CI (đỡ chờ). Trước khi push có thể tự kiểm tra nhanh trên máy: `npm run check:i18n`, `npm run build`, `cargo clippy --release` (trong `src-tauri`).
- **CI** (`.github/workflows/ci.yml`) chạy khi mở / cập nhật PR vào `main` (và PR vào `dev` của Dependabot), sau khi merge vào `main` (chỉ để lưu cache cho các PR sau, không ai phải chờ), hoặc bấm tay ở tab Actions: kiểm tra đa ngôn ngữ, build giao diện, `cargo test` + `cargo clippy -D warnings` với profile `ci` (= release bỏ LTO, nhanh hơn nhiều) cho cả workspace (app + MCP, chung một Cargo.lock nên PR của Dependabot cũng xanh). Lần đầu khoảng 10–15 phút (chưa có cache), sau đó vài phút.
- Muốn phát hành: trên `dev` chạy `npm run bump 0.3.1` (hoặc `patch` / `minor` / `major`), commit, mở PR `dev → main`, merge.
- **Release** (`.github/workflows/release.yml`) chạy khi `main` thay đổi: nếu chưa có Release `v<version>` thì build và phát hành (không chạy lại CI: code đã qua CI ở PR, nên bật branch protection bắt buộc CI xanh); có rồi thì bỏ qua. Có thể chạy tay ở tab Actions (`workflow_dispatch`).
- Nên bật **branch protection** cho `main` (Settings → Branches): bắt buộc PR + CI xanh (`Build + lint + test (Windows)`), cấm push thẳng.

## 3. Người dùng thấy gì

1. Mở app. 10 giây sau app kiểm tra bản mới ở nền, rồi cứ 6 giờ một lần, **chỉ khi cửa sổ đang hiện** và người dùng bật "Tự kiểm tra bản mới" (mặc định bật).
2. Có bản mới → dải `UpdateBar` ở đầu vùng nội dung: "Có bản mới 0.3.1 (bạn đang dùng 0.3.0) · Có gì mới ▾ · [Bỏ qua bản này] [Để sau] [Cập nhật]".
   - **Có gì mới**: nội dung Release, hiển thị bằng `markdown.js`.
   - **Để sau**: ẩn tới lần mở app sau. **Bỏ qua bản này**: không báo bản đó nữa (`update:skip`); bản sau báo lại.
   - **Cập nhật**: hỏi lại (`store.confirm`, vì việc đang chạy sẽ dừng), rồi thanh tiến trình 4 px "Đang tải 4.2 / 9.8 MB" → "Đang cài…".
3. App đóng; bộ cài chạy chế độ passive (`/P /UPDATE /R`: chỉ có thanh tiến trình) rồi tự mở lại app. Windows hỏi UAC như mỗi lần mở app.
4. **Cài đặt → Thông tin**: công tắc "Tự kiểm tra bản mới", dòng "Kiểm tra bản mới" (trạng thái + nút **Kiểm tra ngay**, kiểm tra tay vẫn báo bản đã Bỏ qua, có nút Cập nhật khi có bản mới). Bản tự build: công tắc bị khoá, ghi "Bản tự build, không tự cập nhật".

## 4. Thiết kế kỹ thuật

### Backend: `src-tauri/src/update.rs`
- Repo và khoá công khai **không ghi trong code**: CI nhúng lúc build bằng `option_env!("FOMEG_UPDATE_REPO")` (= `${{ github.repository }}`) và `option_env!("FOMEG_UPDATE_PUBKEY")` (= variable `TAURI_UPDATER_PUBKEY`). Thiếu một trong hai → `enabled: false`: bản `npm run tauri build` / `tauri dev` trên máy không bao giờ tự cập nhật. `build.rs` có `rerun-if-env-changed` cho cả hai.
- `tauri.conf.json` **bắt buộc** có `plugins.updater` (plugin đòi trường `pubkey`, thiếu là app lỗi lúc mở): `{ "pubkey": "", "windows": { "installMode": "passive" } }`. Khoá thật được đặt lúc chạy qua `Builder::pubkey`.
- Command (async, không chặn webview):

  | Command | Trả về |
  |---|---|
  | `update_info()` | `{ enabled, version, repo }` |
  | `update_check()` | `null` (đã mới nhất) hoặc `{ version, current, notes, date_ms }`. Endpoint `https://github.com/{repo}/releases/latest/download/latest.json`, timeout 30 s. Bản tìm được giữ trong `State<Pending>` để cài đúng bản người dùng đã thấy. |
  | `update_install()` | `download_and_install`; sự kiện `update_progress { stage: download\|install, got, total }` tối đa ~7 lần/giây. Thành công: plugin chạy bộ cài rồi `exit(0)` (không trả về). Lỗi: `Err` (đã dịch, `locales/update.yml`), bản đã báo vẫn giữ để thử lại, ghi `util::log_line`. |
- Plugin tự kiểm chữ ký trước khi chạy bộ cài; bộ cài bị tráo / hỏng bị từ chối.

### Frontend
- `src/lib/update.svelte.js` (store `updater`): `info`, `avail`, `checking`, `error`, `lastCheck`, `phase`, `got`, `total`, `hidden`; `init()` (gọi trong `App.svelte`), `check({ manual })`, `later()`, `skip()`, `install()`.
- `src/lib/UpdateBar.svelte`: dải `.banner.update-bar` + thanh `.cp-track`/`.cp-fill` (transform, không hoạt ảnh liên tục). Glyph `download`.
- Chữ: `src/lib/i18n/{vi,en}/update.js`. Tuỳ chọn: `prefs.updateAuto` (`ui:updateAuto`). Mock (`mock-v2.js`) giả lập bản 0.3.1 + tiến trình tải.

### Version: một nguồn duy nhất
- `package.json` là nguồn. `tauri.conf.json` có `"version": "../package.json"`, giao diện đọc qua `src/lib/version.js` (import từ package.json).
- `npm run bump <x.y.z|patch|minor|major>` (`scripts/bump.mjs`) đổi package.json + package-lock.json + version của 2 crate Rust + Cargo.lock chung của workspace (giữ CRLF), và từ chối version không lớn hơn bản hiện tại (người dùng chỉ nhận bản lớn hơn).

### Cấu hình phát hành: `src-tauri/tauri.release.conf.json`
Build trên máy giữ nguyên (exe portable, `bundle.active: false`). CI thêm `--config src-tauri/tauri.release.conf.json`: NSIS, `createUpdaterArtifacts: true`, `installMode: perMachine`.
- **Vì sao perMachine (Program Files)**: app chạy bằng quyền Admin; exe trong `%LOCALAPPDATA%` thì chương trình thường nào cũng ghi đè được rồi chờ bạn mở bằng Admin (leo thang quyền). Program Files chỉ Admin sửa được.
- `productName` của bộ cài dùng dấu nháy cong `’` ("Vietnamese’s Ultimate W App"): script NSIS của Tauri đặt đường dẫn shortcut trong dấu nháy đơn, dấu `'` thường làm makensis lỗi ("macro NSISCOMCALL requires 4 parameter(s), passed 9"). Tên cửa sổ vẫn là tên gốc (`app.windows[].title`).
- `mainBinaryName: fomeg-windows-app` giữ tên exe sau khi cài (named pipe của MCP kiểm tra tên tiến trình này).
- Bộ cài NSIS bằng tiếng Anh (trong app vẫn theo ngôn ngữ người dùng chọn).
- Tauri CLI ký bộ cài rồi **tự kiểm chữ ký bằng `plugins.updater.pubkey` của cấu hình**; `tauri.conf.json` để `""` nên `release.yml` ghi variable `TAURI_UPDATER_PUBKEY` vào `tauri.release.conf.json` ngay trước khi build (chỉ trên máy CI). Thiếu bước này: bộ cài build xong nhưng lệnh lỗi "failed to decode pubkey: Missing comment in public key".
- Thử bộ cài trên máy: đặt `TAURI_SIGNING_PRIVATE_KEY` (+ `_PASSWORD`), tạm điền nội dung `.key.pub` vào `plugins.updater.pubkey` của `tauri.release.conf.json` (đừng commit), rồi `npm run build:setup` (bản này không có repo nhúng nên không tự cập nhật).
- Cache CI: chỉ lần chạy trên `main` lưu cache Rust (PR đọc được cache của main, cache tạo trong PR thì PR khác không dùng được). Lần đầu sau khi đổi đường dẫn cache (vd. chuyển sang workspace) build từ đầu ~10 phút, các lần sau chỉ biên dịch lại crate của app.

## 5. Việc bạn tự làm một lần

1. **Tạo khoá ký cập nhật** trên máy bạn (PowerShell):
   ```powershell
   npx tauri signer generate -w "$env:USERPROFILE\.tauri\fomeg-updater.key"
   ```
   Đặt mật khẩu khi được hỏi. Ra hai tệp: `fomeg-updater.key` (**bí mật**) và `fomeg-updater.key.pub` (công khai).
2. **GitHub → repo → Settings → Secrets and variables → Actions**:
   - tab **Secrets**: `TAURI_SIGNING_PRIVATE_KEY` = toàn bộ nội dung tệp `.key`; `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` = mật khẩu.
   - tab **Variables**: `TAURI_UPDATER_PUBKEY` = toàn bộ nội dung tệp `.key.pub`.
   Thiếu thứ nào thì job Release dừng sớm và báo rõ thiếu gì.
3. Nếu job Release báo lỗi 403 khi tạo Release: **Settings → Actions → General → Workflow permissions** → *Read and write permissions*.
4. **Sao lưu tệp `.key` và mật khẩu** ở chỗ an toàn (password manager). Mất khoá = các bản đã cài không nhận cập nhật được nữa, người dùng phải tự tải lại bộ cài. Không bao giờ commit tệp `.key` (`.gitignore` đã chặn `*.key`).
5. (Nên) bật branch protection cho `main` như mục 2.
6. Repo nên **public**: Actions miễn phí và người dùng tải được bộ cài / `latest.json` không cần đăng nhập. Repo private thì app không đọc được Release.

## 6. Mỗi lần ra bản mới

```powershell
git switch dev
npm run bump patch            # 0.3.0 -> 0.3.1 (hoặc: npm run bump 0.4.0)
git commit -am "chore: phát hành v0.3.1"
git push
# mở PR dev -> main, chờ CI xanh, merge
```
CI build khoảng 10–20 phút rồi tạo Release `v0.3.1` (ghi chú = các commit từ bản trước, bỏ `chore:`/`ci:`/`docs:`). Người dùng thấy thông báo ở lần kiểm tra tiếp theo (lúc mở app, hoặc trong vòng 6 giờ).

Viết commit theo kiểu `feat: …`, `fix: …` (tiếng Việt cũng được) để ghi chú phát hành dễ đọc.

## 7. Hạn chế đã biết

- **SmartScreen**: lần tải `setup.exe` đầu tiên bằng trình duyệt bị cảnh báo "Unknown publisher" vì chưa có chữ ký Authenticode (chữ ký minisign chỉ dùng cho updater). Các lần cập nhật trong app tải trực tiếp nên thường không bị. Muốn hết hẳn: Azure Trusted Signing hoặc SignPath (miễn phí cho mã nguồn mở), cấu hình `bundle.windows.signCommand` trong `tauri.release.conf.json`.
- **UAC khi mở lại** sau cập nhật là bình thường (app đòi quyền Admin).
- **Bản portable / tự build** không tự cập nhật; người dùng bản cũ (`quanlykhoidong.exe`) cài `setup.exe` một lần, dữ liệu được `core/migrate.rs` chuyển sang tên mới.
- **Chống hạ version**: `requireSignedVersion` của plugin chưa bật; chỉ bật sau khi kiểm chứng CLI trong CI ghi version vào chữ ký, nếu không mọi bản cập nhật sẽ bị từ chối.

## 8. Cách kiểm tra lần đầu

1. Làm mục 5, rồi phát hành `0.3.1` theo mục 6. Kiểm tra Release có `…_x64-setup.exe`, `.sig`, `latest.json`.
2. Cài `0.3.1` bằng setup.exe. Mở Cài đặt → Thông tin: phải thấy công tắc "Tự kiểm tra bản mới" (không bị khoá) và "Đã là bản mới nhất" sau khi bấm Kiểm tra ngay.
3. Phát hành `0.3.2`. Mở `0.3.1`: dải thông báo hiện → thử **Để sau** (mở lại app thì báo lại), **Bỏ qua bản này**, **Kiểm tra ngay** (vẫn báo), **Cập nhật** (tải, cài, mở lại, đúng `0.3.2` ở thanh trạng thái).
4. Tắt mạng rồi Kiểm tra ngay: phải có thông báo lỗi dễ hiểu, app không treo.
