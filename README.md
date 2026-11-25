# Vietnamese's Ultimate W App

App Windows để xem / bật / tắt mọi thứ tự chạy cùng máy, quản lý dịch vụ, theo dõi RAM thời gian thực.
Phần lõi viết bằng Rust gọi thẳng Windows API (không dùng PowerShell), giao diện Svelte chạy trên WebView2 có sẵn của Windows.

## Cần cài (1 lần)

- Rust (`winget install Rustlang.Rustup`) + Visual Studio Build Tools (C++)
- Node.js LTS (`winget install OpenJS.NodeJS.LTS`)
- WebView2: Windows 11 có sẵn.

## Tải về

Vào trang **Releases** của repo, tải `…_x64-setup.exe` và cài. Từ đó app tự báo khi có bản mới và cập nhật trong một cú bấm.

## Build

```powershell
npm install
npm run tauri build
```

Bản tự build là exe portable, không tự cập nhật. Phát hành và tự cập nhật (nhánh `dev` → `main`, `npm run bump`, GitHub Actions): xem `docs/auto-update-plan.md`.

File chạy: `src-tauri\target\release\fomeg-windows-app.exe` (một file duy nhất, mở là Windows hỏi quyền Admin).

## Phát triển

```powershell
npm run tauri dev    # chạy app bản debug (không có quyền Admin – các thao tác bật/tắt sẽ bị từ chối)
npm run dev          # chỉ giao diện trong trình duyệt, dữ liệu giả lập (mock)
```

Muốn thử bật/tắt thật khi dev: mở PowerShell bằng "Run as administrator" rồi chạy `npm run tauri dev`.

Đo tốc độ quét trên máy thật:

```powershell
cd src-tauri
cargo test --release --test scan -- --nocapture
```

## Dữ liệu app ghi ra

- Log thay đổi: `%LOCALAPPDATA%\fomeg-windows-app\log.txt`
- Cache nhà phát hành (giúp mở app nhanh): `%LOCALAPPDATA%\fomeg-windows-app\pubcache.json`
- Mục RunOnce / Policy Run bị tắt được cất ở `HKCU\Software\fomeg-windows-app\Disabled` để có thể khôi phục khi bật lại.

## Tính năng (v0.3)

- **Tổng quan**: 5 thẻ CPU / Bộ nhớ / GPU / Đĩa / Mạng (vòng tải, biểu đồ 60 giây), RAM theo thời gian, tiến trình và mục khởi động tốn RAM nhất.
- **Tiến trình** (giống Task Manager): xem theo Nhóm / Danh sách / Cây, CPU · RAM · đĩa · GPU từng tiến trình, icon app, chuột phải để
  Kết thúc tác vụ (kết thúc ngay như `taskkill /F`), Kết thúc cây tiến trình, Khởi động lại, Mở vị trí tệp, Thuộc tính, Độ ưu tiên, Hỏi AI.
- **Hiệu năng**: biểu đồ chi tiết CPU (tổng / từng luồng), bộ nhớ, từng đĩa, từng card mạng, từng GPU (3D, Copy, Video), pin; xem 1–60 phút.
- **Khởi động** và **Dịch vụ** giúp bật/tắt các mục khởi động và quản lý trạng thái, kiểu khởi động của dịch vụ.
- **Dọn dẹp** quét các tệp .tmp và .temp đã được tạo và sửa lần cuối ít nhất 7 ngày trước trong Temp của người dùng và Temp của Windows. Xem kết quả trước khi xác nhận xóa vĩnh viễn. App bỏ qua tệp đang dùng, thiếu quyền, đã thay đổi hoặc là liên kết.
- **Trợ lý AI** (Ctrl+J): chat, có thể gửi kèm tình trạng máy. Chọn 1 trong 3 nhà cung cấp trong phần cài đặt của panel AI:
  **Anthropic (Claude)**, **OpenAI**, hoặc **Tùy chỉnh** – mọi dịch vụ tương thích OpenAI (LM Studio, Ollama, OpenRouter, proxy…; nhập Base URL,
  API key không bắt buộc). Bấm "Tải danh sách model" để chọn model, "Kiểm tra kết nối" để thử key/URL.
  Mỗi nhà cung cấp có key riêng, được mã hoá bằng DPAPI và chỉ lưu trên máy này (`%LOCALAPPDATA%\fomeg-windows-app\ai.json`).
- Thanh trạng thái: chọn chu kỳ cập nhật 0,5 s / 1 s / 2 s / 5 s hoặc tạm dừng. Khi thu nhỏ hoặc ẩn cửa sổ, app chỉ đo 5 giây một lần (giữ lịch sử) và ngừng cập nhật giao diện.
