# Nhật ký thay đổi

Mọi thay đổi đáng chú ý của dự án được ghi trong tệp này.

Định dạng theo [Keep a Changelog](https://keepachangelog.com/vi/1.1.0/),
và dự án tuân theo [Semantic Versioning](https://semver.org/lang/vi/spec/v2.0.0.html).
Bản tiếng Anh: [CHANGELOG.md](CHANGELOG.md).

## [Unreleased]

### Thêm mới

- **Trang "Có gì mới".** Nút ở cuối thanh bên (thay cho "Mở log") mở nhật ký thay đổi của từng phiên bản theo ngôn ngữ đang chọn; khi có bản mới chưa cài, ghi chú của bản đó hiện trên cùng kèm nút Cập nhật.
- **Thông báo sau khi cập nhật.** Lần mở đầu tiên sau khi lên bản mới, app hiện vài thay đổi chính và chỉ chỗ xem đầy đủ.
- **Tab "Nhật ký" trong Cài đặt.** Mở tệp log hoặc thư mục chứa log, và chọn thư mục lưu log khác (log cũ được chuyển sang); không cho chọn thư mục hệ thống.

### Thay đổi

- **Mở log chuyển vào Cài đặt → Nhật ký.**
- **Ghi chú trên thanh cập nhật gọn hơn.** "Có gì mới" chỉ hiện phần đúng ngôn ngữ đang dùng.

### Sửa lỗi

- **Nút Cập nhật hỏi xác nhận trước.** Nút Cập nhật ở "Có gì mới" và ở Cài đặt → Thông tin hỏi xác nhận, giống thanh cập nhật.
- **Bớt chạy ngầm.** Trang Cổng ngừng làm mới khi rời trang hoặc khi tạm dừng cập nhật, và biểu đồ RAM của mục khởi động và dịch vụ không ghi thêm khi tạm dừng.
- **Kết thúc tiến trình kiểm tra lại ngay trước khi làm.** Ở trang Tiến trình, Cổng và đề xuất của trợ lý AI, tiến trình đã thoát hoặc PID đã được chương trình khác nhận sau khi bạn xác nhận sẽ được bỏ qua và báo lại, thay vì tác động nhầm.
- **Gỡ cài đặt không chờ mãi.** Nếu trình gỡ cài đặt của ứng dụng không chạy được, app báo lỗi thay vì treo.
- **Dọn dẹp đo đúng và cập nhật ổ C: ngay.** Dung lượng mục, khung chi tiết và tổng "đã chọn" khớp nhau; phần sẽ được giữ lại (tệp đổi trong 24 giờ qua, tệp đang được dùng lúc dọn) hiện rõ thay vì 0 B. Sau khi dọn, dung lượng trống của ổ C: đổi ngay, không cần quét lại.
- **Ổ cứng cập nhật ngay sau khi xoá.** Dung lượng và dung lượng trống đổi liền mà không quét lại ổ; thư mục chỉ bị xoá một phần được đo lại riêng. Xoá từ menu chuột phải của Windows cũng cập nhật trang Ổ cứng, và dọn dẹp giữ đúng số liệu của các thư mục đã dọn.
- **Khởi động lại tiến trình hoạt động lại.** Nút Khởi động lại dùng được với app thường và Explorer khi app chạy với quyền quản trị; tiến trình cũ chỉ bị kết thúc khi tiến trình mới đã sẵn sàng. Tiến trình chạy trong sandbox bị từ chối thay vì được chạy lại với quyền cao hơn.

## [0.3.0] - 2026-11-25

Bản đầu tiên có bộ cài và tự cập nhật, giao diện hai thứ tiếng.

### Thêm mới

- **Bộ cài và tự cập nhật.** Cài vào Program Files; app tự kiểm tra bản mới (lúc mở và mỗi 6 giờ) và cập nhật trong một cú bấm, không bao giờ tự cài khi bạn chưa đồng ý.
- **Tiếng Việt và tiếng Anh.** Đổi ngay trong Cài đặt → Giao diện → Ngôn ngữ, không cần mở lại app; trợ lý AI cũng trả lời theo ngôn ngữ đã chọn.
- **Trang Cài đặt mới.** Chia theo nhóm, kèm hướng dẫn kết nối MCP cho từng ứng dụng AI (Claude, Codex, Cursor, VS Code…) và thông tin chi tiết của máy.

### Sửa lỗi

- **Dọn dẹp bỏ sót thư mục Temp.** Không còn báo nhầm "đi qua liên kết" khi đường dẫn Temp có dạng tên ngắn (`RUNNER~1`).

[unreleased]: https://github.com/FomeG/FomegWindowsApp/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/FomeG/FomegWindowsApp/releases/tag/v0.3.0
