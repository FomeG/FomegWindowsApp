# Nhật ký thay đổi

Mọi thay đổi đáng chú ý của dự án được ghi trong tệp này.

Định dạng theo [Keep a Changelog](https://keepachangelog.com/vi/1.1.0/),
và dự án tuân theo [Semantic Versioning](https://semver.org/lang/vi/spec/v2.0.0.html).
Bản tiếng Anh: [CHANGELOG.md](CHANGELOG.md).

## [Unreleased]

## [0.3.1] - 2026-11-27

Thêm trang Biến môi trường và Có gì mới, dọn dẹp theo từng ứng dụng và quét cả ổ nhanh hơn khoảng 10 lần.

### Thêm mới

- **Trang "Có gì mới".** Nút ở cuối thanh bên (thay cho "Mở log") mở nhật ký thay đổi của từng phiên bản theo ngôn ngữ đang chọn; khi có bản mới chưa cài, ghi chú của bản đó hiện trên cùng kèm nút Cập nhật.
- **Thông báo sau khi cập nhật.** Lần mở đầu tiên sau khi lên bản mới, app hiện vài thay đổi chính và chỉ chỗ xem đầy đủ.
- **Trang "Biến môi trường".** Ở Hệ thống → Biến môi trường: xem biến người dùng và hệ thống trong một bảng, Path được tách từng mục và đánh dấu mục không còn tồn tại hoặc bị trùng. Sửa, thêm, xoá ngay trong ngăn chi tiết (chọn thư mục, lên / xuống, "Bỏ các mục này"), mọi thay đổi hoàn tác được ở "Thay đổi gần đây". Giá trị của biến bí mật (KEY, TOKEN…) được che.
- **Trợ lý AI cho biến môi trường.** Nút "Hỏi AI" và "Nhờ AI dọn Path": trợ lý đọc biến (giá trị bí mật luôn bị che), chỉ ra mục Path hỏng / trùng và đề xuất sửa; bạn xem lại, tích rồi mới áp dụng.
- **Chọn đúng phần cần dọn.** Ở Dọn dẹp, ngăn chi tiết của mỗi ứng dụng có cây Cache / Log / Báo cáo lỗi → từng thư mục: tích cả loại hoặc từng thư mục, bấm để xem tệp lớn nhất bên trong, chuột phải để mở thư mục hoặc hỏi AI, rồi bấm "Dọn phần đã chọn" ở đáy.
- **Tab "Nhật ký" trong Cài đặt.** Mở tệp log hoặc thư mục chứa log, và chọn thư mục lưu log khác (log cũ được chuyển sang); không cho chọn thư mục hệ thống.

### Thay đổi

- **Mở log chuyển vào Cài đặt → Nhật ký.**
- **Ghi chú trên thanh cập nhật gọn hơn.** "Có gì mới" chỉ hiện phần đúng ngôn ngữ đang dùng.
- **Dọn dẹp theo từng ứng dụng, kèm icon thật.** Trang Dọn dẹp tìm thêm log, báo cáo lỗi (Crashpad…) và cache Service Worker của Discord, Brave, Chrome, Edge, VS Code…, gộp theo ứng dụng với tên và icon của app đã cài; cột "Nhóm" cho biết gồm Cache · Log · Báo cáo lỗi. Thư mục log chỉ được dọn khi bên trong toàn là tệp log. Lần quét đầu xong trong khoảng 1 giây.
- **Dọn dẹp chia tab rõ hơn.** Tệp tạm, Hệ thống, Ứng dụng (chỉ ứng dụng nhận ra được), Lập trình (npm, pip, .bun…) và Khác (thư mục không rõ của app nào); tab không có mục thì ẩn.
- **Quét cả ổ nhanh hơn khoảng 10 lần.** Trang Ổ cứng đọc thẳng bảng tệp của ổ NTFS: ổ C 1,8 triệu tệp xong trong 3–5 giây thay vì khoảng 40 giây. Quét một thư mục, hoặc ổ không phải NTFS, vẫn dùng cách cũ.
- **Dung lượng ổ cứng chính xác hơn.** Tệp có nhiều tên (hard link, ví dụ giữa WinSxS và System32) chỉ được tính một lần, và System Volume Information giờ đã đọc được, nên tổng khớp với dung lượng đã dùng của ổ.

### Sửa lỗi

- **Nút Cập nhật hỏi xác nhận trước.** Nút Cập nhật ở Cài đặt → Thông tin hỏi xác nhận trước khi cài, giống thanh cập nhật.
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

[unreleased]: https://github.com/FomeG/FomegWindowsApp/compare/v0.3.1...HEAD
[0.3.1]: https://github.com/FomeG/FomegWindowsApp/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/FomeG/FomegWindowsApp/releases/tag/v0.3.0
