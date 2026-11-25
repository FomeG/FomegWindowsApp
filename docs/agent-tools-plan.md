# Bộ công cụ cho Agent – plan & thiết kế

Trạng thái: **đã làm đủ 5 giai đoạn** (Toolbox 15 tool chỉ đọc, vòng lặp tool-use cho Anthropic + OpenAI/custom, các bước
"Đang xem / Đã xem" trong khung chat, công tắc "Cho trợ lý tự tra cứu máy", MCP server `fomeg-windows-app-mcp.exe` + hướng dẫn ở trang
Cài đặt, khối đề xuất `tat_khoi_dong` / `dich_vu` / `ket_thuc`, named pipe `core/agent_pipe.rs` để MCP dùng dữ liệu sống
của app đang mở). MCP nhúng sẵn trong exe của app và tự giải ra `%LOCALAPPDATA%\fomeg-windows-app\fomeg-windows-app-mcp.exe`: chỉ phát
hành một exe; agent tự khởi chạy MCP, người dùng không phải mở riêng. Đã chốt: MCP là binary riêng (không UAC); agent được tự quét dọn dẹp (≤ ~10 s) nhưng chỉ đọc kết quả
quét ổ đĩa có sẵn; mô tả tool tiếng Anh, kết quả tiếng Việt. Tham khảo: RustWTM `crates/wtm-ai/src/tools.rs` (Toolbox),
`crates/wtm-ai/src/agent.rs` (vòng lặp tool-use), `crates/wtm-cli/src/mcp.rs` (`wtm mcp`).

## 1. Mục tiêu

- Trợ lý AI trong app **tự tra cứu** những gì nó cần (tiến trình, cây tiến trình, khởi động, dịch vụ, cổng, ổ đĩa, dọn
  dẹp, hiệu năng, phần cứng…) thay vì bảo người dùng "gửi mình ảnh cây tiến trình". Ví dụ ở đoạn chat: hỏi "app này tốn
  bao nhiêu RAM tính cả WebView2" → agent gọi `process_tree(pid)` → tự cộng RAM cả nhánh → trả lời một con số.
- Cùng bộ tool đó mở ra ngoài như **MCP server**, để Claude Code / Claude Desktop / coding agent khác xem máy.
- **Giới hạn cứng: chỉ đọc + gợi ý.** Tool không bao giờ xoá, tắt, dừng, kết thúc hay ghi gì. Muốn thao tác thì agent viết
  khối đề xuất (`xoa`, `dong_cong`, `don_dep`, sau này `tat_khoi_dong`, `dich_vu`…), người dùng tự xem lại và bấm trong
  popup `ActionReview` như hiện nay. MCP server không có tool thao tác (RustWTM có `--allow-control`; mình không làm).

## 2. Làm bây giờ hay đợi app đủ tính năng?

**Làm khung ngay, thêm tool dần theo tính năng.** Lý do:

- Tool chỉ là lớp mỏng đọc dữ liệu core đã có (monitor, scan, du, ports, cleanup). Phần khó và dùng chung là khung:
  định nghĩa tool, vòng lặp tool-use với 3 nhà cung cấp, giao diện "đang tra cứu…", giới hạn, MCP. Khung làm một lần.
- Đợi app xong mới làm thì phải viết rất nhiều tool một lúc, dễ lệch với code đã đổi. Làm kèm theo từng tính năng thì mỗi
  tính năng mới chỉ thêm 1–2 tool, cùng lúc với lúc viết nó (đã nhớ rõ dữ liệu).
- Các khối ngữ cảnh hiện có (`<disk_scan>`, `<ports>`, `<cleanup>`, `<system_snapshot>`) về sau thành "tool đã gọi sẵn";
  không phải bỏ đi.

Đề xuất thêm vào CLAUDE.md khi bắt đầu: *"Mỗi tính năng có dữ liệu mới phải có tool đọc tương ứng trong
`core/agent_tools.rs` (mô tả + schema + test), cùng lần với tính năng."*

## 3. Kiến trúc

```
core/agent_tools.rs   Toolbox: danh sách tool (name, description, input JSON schema) + run(name, args) -> text
        │   đọc từ AppState (mẫu monitor mới nhất, kết quả scan, du::Scan, ports, cleanup plan…) – không đo thêm
        ├── core/ai.rs        vòng lặp tool-use cho trợ lý trong app (Anthropic tool_use / OpenAI tools)
        └── bin fomeg-windows-app-mcp      MCP server (JSON-RPC 2.0 qua stdio) dùng cùng Toolbox
```

### 3.1 Toolbox (`core/agent_tools.rs`)
- Mỗi tool: `name` (snake_case, tiếng Anh như RustWTM để model hiểu tốt), `description` (tiếng Anh, nói rõ dùng khi nào),
  `inputSchema`, kết quả là **văn bản gọn** (mỗi dòng một mục, đơn vị ngắn như `du::sz`), có trần ký tự (≈ 4 000) và dòng
  "… và N mục nữa".
- Chỉ đọc dữ liệu đã có trong `AppState`. Tool nào cần số liệu "đang chạy" (CPU %) mà monitor đang tạm dừng/ẩn thì lấy một
  mẫu mới như RustWTM (`FRESH` 2 s, `WINDOW` 0,5 s) – không mở vòng lấy mẫu riêng.
- Bảo mật dữ liệu: lọc bí mật trong dòng lệnh (port `redact.rs` của RustWTM: `--password=`, token `sk-…`, `user:pass@`),
  tuỳ chọn "không gửi dòng lệnh". Không đọc NỘI DUNG tệp, chỉ siêu dữ liệu (tên, cỡ, ngày).

### 3.2 Vòng lặp tool-use trong app (`core/ai.rs`)
- Anthropic: gửi `tools`, nhận `tool_use` → chạy Toolbox → gửi `tool_result` (gộp mọi kết quả của một lượt), lặp.
  OpenAI/custom: `tools` + `tool_calls` → `role: tool`. Nhà cung cấp không hỗ trợ tool → chạy như hiện nay (khối ngữ cảnh).
- Giới hạn như RustWTM: ≤ 6 vòng, trần độ dài lịch sử gửi lại, hạn giờ; vòng cuối không cho gọi tool, bắt trả lời.
  Huỷ (nút Dừng) dừng cả vòng lặp. Lịch sử tool giữ trong `Chats` (append-only, replay đúng nhà cung cấp).
- Sự kiện mới cho UI: `ai` `{ kind: 'tool', name, args, summary }` → khung chat hiện dòng nhỏ có thể mở xem
  ("Đã xem: cây tiến trình PID 12860 · 14 tiến trình"). Không làm chậm luồng trả lời.
- Cài đặt AI: "Cho trợ lý tự tra cứu máy" (bật mặc định), "Gửi dòng lệnh tiến trình" (bật, có lọc bí mật).

### 3.3 MCP server
- Vấn đề: exe release yêu cầu Admin (manifest) → coding agent khởi chạy sẽ bật UAC mỗi lần. Đề xuất **binary riêng
  `fomeg-windows-app-mcp.exe`** (cùng crate, `[[bin]]`, manifest `asInvoker`, console): tự lấy mẫu tại chỗ, không cần app đang mở.
  Dữ liệu cần Admin (Windows\Temp, tiến trình hệ thống…) ghi rõ "cần Admin".
- Giai đoạn sau (tuỳ chọn): nếu app đang mở thì `fomeg-windows-app-mcp` hỏi app qua named pipe để lấy mẫu monitor, lịch sử 1 giờ, kết
  quả quét ổ đĩa / dọn dẹp đang có. Pipe chỉ cho người dùng hiện tại, chỉ đọc.
- Hỗ trợ: `initialize`, `ping`, `tools/list`, `tools/call`, `prompts/list|get` (vd. "Vì sao máy chậm?"). Không nghe mạng.
- Cấu hình mẫu cho Claude Code: `claude mcp add fomeg-windows-app -- "C:\...\fomeg-windows-app-mcp.exe"`.

## 4. Danh mục tool (theo tính năng hiện có)

| Tool | Trả về | Nguồn trong app | Chi phí |
| --- | --- | --- | --- |
| `system_overview` | CPU/RAM/GPU/đĩa/mạng/pin hiện tại, top tiến trình, uptime | mẫu monitor (`metrics`, `procs`) | ~0 |
| `metrics_history` | CPU/RAM/đĩa/mạng N phút gần nhất (tóm tắt min/avg/max, đỉnh) | `AppState.history` (1 giờ) | ~0 |
| `hardware` | CPU, số nhân, RAM, GPU, ổ đĩa vật lý, pin | metrics/gpu sampler | ~0 |
| `list_processes` | tiến trình sắp theo cpu/ram/disk/gpu, lọc theo tên, nhóm theo app | `ProcSampler` | ~0 |
| `process_report` | 1 PID: đường dẫn, dòng lệnh (đã lọc), nhà phát hành, cha/con, dịch vụ nó chạy, cổng, có khởi động cùng Windows không | procs + scan + ports | ms |
| `process_tree` | tổ tiên + toàn bộ con của 1 PID, **tổng RAM/CPU cả nhánh** | procs (parent pid) | ~0 |
| `startup_items` | mục khởi động: nguồn, bật/tắt, nhà phát hành, RAM, lệnh; lọc theo nguồn/tên | `scan::run` (đã có) | ~0 |
| `services` | dịch vụ: trạng thái, kiểu khởi động, tài khoản, PID, RAM; lọc | scan | ~0 |
| `service_details` | 1 dịch vụ: mô tả, đường dẫn, phụ thuộc, có phải CRITICAL không | scan + SCM | ms |
| `ports` | cổng đang nghe + tiến trình + phạm vi mạng | `ports::list` | ~2 ms |
| `port_owner` | ai đang giữ 1 cổng | `ports::list` | ~2 ms |
| `drives` | các ổ: loại, dung lượng, còn trống | `du::drives` | ~1 ms |
| `disk_usage` | thư mục lớn nhất / cây con của một đường dẫn **trong lần quét ổ đĩa đang có**; chưa quét thì nói rõ | `du::Scan` (`ai_summary`, `du_children`) | ms |
| `folder_files` | tệp lớn nhất trong 1 thư mục (chỉ tên/cỡ/ngày) | `du::list_files` | ms |
| `cleanup_report` | kết quả quét dọn dẹp (mục, cỡ, an toàn/cân nhắc); chưa quét thì quét (≤ 3,5 s) | `Cleaner` | 0–3,5 s |
| `cleanup_item` | 1 mục dọn dẹp: mọi đường dẫn, phần mới < 24 h | `Cleaner` plan | ~0 |
| `app_info` | phiên bản app, đang chạy Admin không, khoảng cập nhật | AppState | ~0 |

Gợi ý thao tác (không phải tool): khối đề xuất có sẵn `xoa`, `dong_cong`, `don_dep`; thêm sau `tat_khoi_dong`
(bật/tắt mục khởi động), `dich_vu` (đổi kiểu khởi động / dừng), `ket_thuc` (kết thúc tiến trình) – đều qua `ActionReview`.

## 5. Giai đoạn

1. **Khung + 6 tool cốt lõi** (`system_overview`, `list_processes`, `process_report`, `process_tree`, `startup_items`,
   `services`) + vòng lặp Anthropic + dòng "đã xem…" trong chat + test với server giả (`tests/ai_http.rs`).
   Xong khi: hỏi "app này tốn bao nhiêu RAM tính cả WebView2" ra đúng một con số.
2. **OpenAI/custom function calling** + các tool còn lại ở bảng trên + lọc bí mật dòng lệnh + cài đặt bật/tắt.
3. **`fomeg-windows-app-mcp.exe`** (stdio, chỉ đọc) + hướng dẫn thêm vào Claude Code / Claude Desktop + test JSON-RPC.
4. **Khối đề xuất mới** (`tat_khoi_dong`, `dich_vu`, `ket_thuc`) để agent gợi ý thao tác trên mọi trang.
5. (Tuỳ chọn) named pipe để `fomeg-windows-app-mcp` dùng dữ liệu sống của app đang mở.

## 6. Hiệu năng & an toàn (bắt buộc)

- Tool không bật vòng lấy mẫu nào; dùng lại mẫu monitor. Tool nặng (quét dọn dẹp) có hạn giờ và trả kết quả dở dang.
- Kết quả có trần ký tự; vòng lặp có trần số vòng / thời gian / độ dài lịch sử (tránh tốn token).
- Không tool nào ghi/xoá/khởi chạy gì. Mọi thao tác đi qua popup xác nhận của người dùng. MCP không có chế độ thao tác.
- Không đọc nội dung tệp, không gửi khoá AI / tệp cấu hình; lọc bí mật trong dòng lệnh; không nghe cổng mạng.

## 7. Câu hỏi cần chốt khi bắt đầu

- MCP: binary riêng `fomeg-windows-app-mcp.exe` (không cần app mở, không hỏi UAC) – hay chạy `fomeg-windows-app.exe --mcp`?
- Có cho agent tự quét dọn dẹp / quét ổ đĩa khi chưa quét không (tốn vài giây IO), hay chỉ đọc kết quả có sẵn?
- Mô tả tool viết tiếng Anh (model hiểu tốt hơn, như RustWTM) – kết quả tool vẫn tiếng Việt?
