// vi – khu vực shell. Khoá dạng 'shell.xxx'.
export default {
  'shell.ui_error': "Có lỗi trong giao diện",

  // chung
  'shell.cancel': 'Huỷ',
  'shell.close': 'Đóng',
  'shell.refresh': 'Làm mới',
  'shell.filter': 'Lọc',
  'shell.chart': 'Biểu đồ',

  // App
  'shell.banner.no_admin': 'Đang chạy không có quyền Admin – các thay đổi sẽ bị từ chối',
  'shell.banner.mock': 'Chế độ xem thử trong trình duyệt – dữ liệu giả lập, không thay đổi gì trên máy.',

  // Sidebar
  'shell.nav.label': 'Điều hướng',
  'shell.brand.sub': 'GIÁM SÁT HỆ THỐNG',
  'shell.nav.expand': 'Mở rộng thanh bên',
  'shell.nav.collapse': 'Thu gọn thanh bên',
  'shell.nav.group.monitor': 'GIÁM SÁT',
  'shell.nav.group.system': 'HỆ THỐNG',
  'shell.nav.dashboard': 'Tổng quan',
  'shell.nav.processes': 'Tiến trình',
  'shell.nav.performance': 'Hiệu năng',
  'shell.nav.apps': 'Ứng dụng',
  'shell.nav.startup': 'Khởi động',
  'shell.nav.services': 'Dịch vụ',
  'shell.nav.ports': 'Cổng',
  'shell.nav.disk': 'Ổ cứng',
  'shell.nav.cleanup': 'Dọn dẹp',
  'shell.nav.badge.startup': '{n} mục đang bật',
  'shell.nav.badge.services': '{n} dịch vụ đang chạy',
  'shell.nav.ai': 'Trợ lý AI',
  'shell.nav.ai_tip': 'Trợ lý AI (Ctrl+J)',
  'shell.nav.settings': 'Cài đặt',
  'shell.nav.log': 'Mở log',
  'shell.nav.log_tip': 'Mở tệp log',

  // StatusBar
  'shell.status.label': 'Thanh trạng thái',
  'shell.status.mem': 'Bộ nhớ',
  'shell.status.procs': '{n} tiến trình',
  'shell.status.paused': 'Đã tạm dừng cập nhật',
  'shell.status.waiting': 'Đang chờ dữ liệu…',
  'shell.status.just_now': 'Vừa cập nhật',
  'shell.status.ago': 'Cập nhật {s} giây trước',

  // IntervalSelect
  'shell.interval.label': 'Chu kỳ cập nhật',
  'shell.interval.paused': 'Tạm dừng',
  'shell.interval.secs': '{s} giây',
  'shell.interval.every': 'Mỗi {s} giây',

  // OptionRow
  'shell.opt.soon': 'Sắp có',
  'shell.opt.on': 'Bật',
  'shell.opt.off': 'Tắt',
  'shell.opt.locked': 'Không đổi được',

  // Toggle
  'shell.toggle.pending': 'Đang xử lý…',
  'shell.toggle.on': 'Đang bật – bấm để tắt',
  'shell.toggle.off': 'Đã tắt – bấm để bật',

  // ConfirmModal
  'shell.confirm.title': 'Xác nhận',
  'shell.confirm.ok': 'Đồng ý',

  // BulkBar + summarize()
  'shell.bulk.bar': 'Thao tác với mục đã chọn',
  'shell.bulk.selected': 'Đã chọn',
  'shell.bulk.clear': 'Bỏ chọn',
  'shell.bulk.title': '{verb}: {parts}',
  'shell.bulk.ok': '{n} {unit} thành công',
  'shell.bulk.errors': '{count} lỗi',
  'shell.bulk.none': 'không có thay đổi',

  // Toasts
  'shell.toast.details': 'Chi tiết ({n})',
  'shell.toast.close': 'Đóng thông báo',

  // DataTable
  'shell.table.label': 'Danh sách',
  'shell.table.empty': 'Không có mục nào khớp bộ lọc.',
  'shell.table.check_all': 'Chọn tất cả mục đang hiện',
  'shell.table.sort_by': 'Sắp xếp theo {col}',
  'shell.table.resize_col': 'Kéo để đổi độ rộng cột {col}',
  'shell.table.resize_tip': 'Kéo để đổi độ rộng (bấm đúp để đặt lại)',
  'shell.table.select_row': 'Chọn {name}',

  // SearchBox
  'shell.search.placeholder': 'Tìm…',
  'shell.search.clear': 'Xoá tìm kiếm',
  'shell.search.clear_tip': 'Xoá (Esc)',

  // SplitView / InspectorShell
  'shell.split.resize': 'Kéo để đổi độ rộng ngăn chi tiết',
  'shell.split.resize_tip': 'Kéo để đổi độ rộng',
  'shell.insp.hide': 'Ẩn ngăn chi tiết',
  'shell.insp.close_tip': 'Đóng (Esc)',

  // RamCell
  'shell.ram.shared_tip': 'Tiến trình dùng chung cho {n} dịch vụ – RAM là của cả tiến trình',
  'shell.ram.shared': 'chung {n} DV',
};
