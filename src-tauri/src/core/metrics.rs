use std::ffi::c_void;
use crate::tr;
use std::mem::{offset_of, size_of, size_of_val, swap, take};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, NO_ERROR};
use windows::Win32::NetworkManagement::IpHelper::{
    FreeMibTable, GetIfEntry2, GetIfEntry2Ex, GetIfTable2, MibIfEntryNormalWithoutStatistics,
    IF_TYPE_ETHERNET_CSMACD, IF_TYPE_IEEE80211, IF_TYPE_SOFTWARE_LOOPBACK, MIB_IF_ROW2,
    MIB_IF_TABLE2,
};
use windows::Win32::NetworkManagement::Ndis::{IfOperStatusUp, NET_LUID_LH};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    OPEN_EXISTING,
};
use windows::Win32::System::Ioctl::{
    PropertyStandardQuery, StorageDeviceProperty, StorageDeviceSeekPenaltyProperty,
    DEVICE_SEEK_PENALTY_DESCRIPTOR, DISK_GEOMETRY_EX, GET_LENGTH_INFORMATION,
    IOCTL_DISK_GET_DRIVE_GEOMETRY_EX, IOCTL_DISK_GET_LENGTH_INFO, IOCTL_STORAGE_QUERY_PROPERTY,
    STORAGE_DESCRIPTOR_HEADER, STORAGE_DEVICE_DESCRIPTOR, STORAGE_PROPERTY_ID,
    STORAGE_PROPERTY_QUERY,
};
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
    PdhGetRawCounterArrayW, PdhOpenQueryW, PDH_CSTATUS_NEW_DATA, PDH_CSTATUS_VALID_DATA, PDH_FMT,
    PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PDH_INVALID_ARGUMENT, PDH_MORE_DATA,
};
use windows::Win32::System::Power::{
    CallNtPowerInformation, GetSystemPowerStatus, ProcessorInformation, SYSTEM_POWER_STATUS,
};
use windows::Win32::System::ProcessStatus::{GetPerformanceInfo, PERFORMANCE_INFORMATION};
use windows::Win32::System::SystemInformation::{
    ComputerNamePhysicalDnsHostname, GetComputerNameExW, GetLogicalProcessorInformationEx,
    GetTickCount64, GlobalMemoryStatusEx, RelationProcessorCore, MEMORYSTATUSEX,
    SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
};
use windows::Win32::System::Threading::{GetActiveProcessorCount, ALL_PROCESSOR_GROUPS};
use windows::Win32::System::IO::DeviceIoControl;
use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY};
use winreg::RegKey;

#[derive(Serialize, Clone)]
pub struct Cpu {
    pub total: f32,
    pub cores: Vec<f32>,
    pub mhz: u32,
}

#[derive(Serialize, Clone)]
pub struct Mem {
    pub used: u64,
    pub total: u64,
    pub avail: u64,
    pub committed: u64,
    pub commit_limit: u64,
    pub cached: u64,
    pub paged_pool: u64,
    pub nonpaged_pool: u64,
}

#[derive(Serialize, Clone)]
pub struct Disk {
    pub id: String,
    pub name: String,
    pub active: f32,
    pub read: f64,
    pub write: f64,
}

#[derive(Serialize, Clone)]
pub struct Net {
    pub id: String,
    pub name: String,
    pub desc: String,
    pub kind: String,
    pub rx: f64,
    pub tx: f64,
    pub link_bps: u64,
}

#[derive(Serialize, Clone)]
pub struct Counts {
    pub processes: u32,
    pub threads: u32,
    pub handles: u32,
}

#[derive(Serialize, Clone)]
pub struct Battery {
    pub percent: u8,
    pub charging: bool,
    pub plugged: bool,
}

#[derive(Serialize, Clone)]
pub struct Metrics {
    pub t: i64,
    pub cpu: Cpu,
    pub mem: Mem,
    pub disks: Vec<Disk>,
    pub nets: Vec<Net>,
    pub counts: Counts,
    pub uptime_s: u64,
    pub battery: Option<Battery>,
}

#[derive(Serialize, Clone)]
pub struct DiskInfo {
    pub id: String,
    pub name: String,
    pub model: String,
    pub size: u64,
    pub ssd: bool,
}

#[derive(Serialize, Clone)]
pub struct SystemInfo {
    pub cpu_name: String,
    pub cores: u32,
    pub logical: u32,
    pub base_mhz: u32,
    pub mem_total: u64,
    pub disks: Vec<DiskInfo>,
    pub os: String,
    pub hostname: String,
    pub boot_ms: i64,
    /// ngày cài Windows (ms), 0 nếu không rõ
    pub os_installed_ms: i64,
    /// tài khoản Windows đang chạy app
    pub user: String,
    /// thanh RAM, số khe, bo mạch chủ (SMBIOS) – trải phẳng: mem_modules, mem_slots, board
    #[serde(flatten)]
    pub smbios: super::smbios::Smbios,
}

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtQuerySystemInformation(class: u32, info: *mut c_void, len: u32, ret_len: *mut u32) -> i32;
}

const SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION: u32 = 8;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct ProcessorTimes {
    idle: i64,
    kernel: i64,
    user: i64,
    dpc: i64,
    interrupt: i64,
    interrupt_count: u32,
}

const _: () = assert!(size_of::<ProcessorTimes>() == 48);

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct ProcessorPower {
    number: u32,
    max_mhz: u32,
    current_mhz: u32,
    mhz_limit: u32,
    max_idle_state: u32,
    current_idle_state: u32,
}

#[derive(Default)]
struct CpuSampler {
    current: Vec<ProcessorTimes>,
    previous: Vec<ProcessorTimes>,
    power: Vec<ProcessorPower>,
}

impl CpuSampler {
    fn sample(&mut self) -> Cpu {
        let logical = logical_count() as usize;
        let mut cores = vec![0.0; logical];
        self.current.resize(logical, ProcessorTimes::default());
        let mut written = 0u32;
        // Bộ đệm có một cấu trúc 48 byte cho mỗi CPU logic.
        let status = unsafe {
            NtQuerySystemInformation(
                SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION,
                self.current.as_mut_ptr().cast(),
                size_of_val(self.current.as_slice()) as u32,
                &mut written,
            )
        };
        if status >= 0 {
            self.current
                .truncate(written as usize / size_of::<ProcessorTimes>());
            if self.current.len() == self.previous.len() {
                for ((core, now), prev) in cores.iter_mut().zip(&self.current).zip(&self.previous) {
                    let kernel = now.kernel.saturating_sub(prev.kernel);
                    let user = now.user.saturating_sub(prev.user);
                    let idle = now.idle.saturating_sub(prev.idle);
                    let total = kernel.saturating_add(user);
                    if kernel >= 0 && user >= 0 && idle >= 0 && total > 0 {
                        *core =
                            (100.0 * (total - idle) as f64 / total as f64).clamp(0.0, 100.0) as f32;
                    }
                }
            }
            swap(&mut self.current, &mut self.previous);
        }

        let total = cores.iter().sum::<f32>() / logical.max(1) as f32;
        self.power.resize(logical, ProcessorPower::default());
        self.power.fill(ProcessorPower::default());
        // ProcessorInformation ghi một cấu trúc tần số vào mỗi phần tử.
        let status = unsafe {
            CallNtPowerInformation(
                ProcessorInformation,
                None,
                0,
                Some(self.power.as_mut_ptr().cast()),
                size_of_val(self.power.as_slice()) as u32,
            )
        };
        let mhz = if status.0 >= 0 {
            let sum: u64 = self.power.iter().map(|p| u64::from(p.current_mhz)).sum();
            (sum / logical.max(1) as u64) as u32
        } else {
            0
        };
        Cpu { total, cores, mhz }
    }
}

fn logical_count() -> u32 {
    // API không giữ tài nguyên và đếm các CPU đang hoạt động trên mọi nhóm.
    unsafe { GetActiveProcessorCount(ALL_PROCESSOR_GROUPS) }
}

fn physical_memory() -> (u64, u64) {
    let mut status = MEMORYSTATUSEX {
        dwLength: size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // Windows ghi dữ liệu RAM vào cấu trúc có dwLength hợp lệ.
    if unsafe { GlobalMemoryStatusEx(&mut status) }.is_ok() {
        (status.ullTotalPhys, status.ullAvailPhys)
    } else {
        (0, 0)
    }
}

fn memory_and_counts() -> (Mem, Counts) {
    let (total, avail) = physical_memory();
    let cb = size_of::<PERFORMANCE_INFORMATION>() as u32;
    let mut info = PERFORMANCE_INFORMATION {
        cb,
        ..Default::default()
    };
    // Mỗi lần gọi lấy chung số trang bộ nhớ và số tiến trình, luồng, handle.
    if unsafe { GetPerformanceInfo(&mut info, cb) }.is_err() {
        info = PERFORMANCE_INFORMATION::default();
    }
    let to_bytes = |pages: usize| (pages as u64).saturating_mul(info.PageSize as u64);
    (
        Mem {
            used: total.saturating_sub(avail),
            total,
            avail,
            committed: to_bytes(info.CommitTotal),
            commit_limit: to_bytes(info.CommitLimit),
            cached: to_bytes(info.SystemCache),
            paged_pool: to_bytes(info.KernelPaged),
            nonpaged_pool: to_bytes(info.KernelNonpaged),
        },
        Counts {
            processes: info.ProcessCount,
            threads: info.ThreadCount,
            handles: info.HandleCount,
        },
    )
}

const DISK_PATHS: [PCWSTR; 3] = [
    w!(r"\PhysicalDisk(*)\% Idle Time"),
    w!(r"\PhysicalDisk(*)\Disk Read Bytes/sec"),
    w!(r"\PhysicalDisk(*)\Disk Write Bytes/sec"),
];
// Giá trị từ pdh.h, chưa được windows 0.62 xuất ra.
const PDH_FMT_NOCAP100: u32 = 0x8000;

struct DiskQuery {
    query: PDH_HQUERY,
    counters: [PDH_HCOUNTER; DISK_PATHS.len()],
    buffer: CounterBuffer,
}

// Handle PDH là mã định danh của hệ điều hành. Mỗi truy vấn có một chủ sở hữu,
// chỉ được dùng tuần tự qua &mut self và có thể chuyển sang luồng lấy mẫu.
unsafe impl Send for DiskQuery {}

impl DiskQuery {
    fn new() -> Self {
        let mut this = Self {
            query: PDH_HQUERY::default(),
            counters: [PDH_HCOUNTER::default(); DISK_PATHS.len()],
            buffer: CounterBuffer::default(),
        };
        // Các đường dẫn là chuỗi UTF-16 kết thúc bằng NUL và tồn tại suốt chương trình.
        if unsafe { PdhOpenQueryW(PCWSTR::null(), 0, &mut this.query) } == 0 {
            for (path, counter) in DISK_PATHS.into_iter().zip(&mut this.counters) {
                // Handle bộ đếm được truy vấn sở hữu và sẽ được đóng cùng truy vấn.
                if unsafe { PdhAddEnglishCounterW(this.query, path, 0, counter) } != 0 {
                    *counter = PDH_HCOUNTER::default();
                }
            }
            // Lấy mốc đầu cho bộ đếm tốc độ.
            let _ = unsafe { PdhCollectQueryData(this.query) };
        } else {
            this.query = PDH_HQUERY::default();
        }
        this
    }

    fn sample(&mut self) -> Vec<Disk> {
        // Truy vấn và các bộ đếm chỉ được cập nhật tại đây trong luồng lấy mẫu.
        if self.query.is_invalid() || unsafe { PdhCollectQueryData(self.query) } != 0 {
            return Vec::new();
        }
        let mut disks: Vec<Disk> = Vec::new();
        let format = PDH_FMT(PDH_FMT_DOUBLE.0 | PDH_FMT_NOCAP100);
        for (index, counter) in self.counters.into_iter().enumerate() {
            if counter.is_invalid() {
                continue;
            }
            // Hàm PDH ghi đúng kiểu phần tử và đặt tên vào cùng bộ đệm.
            let Some((items, text)) = (unsafe {
                self.buffer.read(|size, count, buffer| {
                    PdhGetFormattedCounterArrayW(counter, format, size, count, buffer)
                })
            }) else {
                continue;
            };
            for item in items {
                let Some((number, volumes)) = disk_instance(text, item.szName) else {
                    continue;
                };
                let disk = disk_entry(&mut disks, number, volumes);
                if matches!(
                    item.FmtValue.CStatus,
                    PDH_CSTATUS_VALID_DATA | PDH_CSTATUS_NEW_DATA
                ) {
                    // PDH_FMT_DOUBLE chỉ định thành viên doubleValue của union.
                    let value = unsafe { item.FmtValue.Anonymous.doubleValue };
                    if value.is_finite() {
                        match index {
                            0 => disk.active = (100.0 - value).clamp(0.0, 100.0) as f32,
                            1 => disk.read = value.max(0.0),
                            _ => disk.write = value.max(0.0),
                        }
                    }
                }
            }
        }
        disks.sort_unstable_by(|a, b| a.id.cmp(&b.id));
        disks
    }

    fn instances(&mut self) -> Vec<Disk> {
        // Mẫu thô có tên đĩa ngay sau lần thu đầu, chưa cần chờ tính tốc độ.
        for counter in self
            .counters
            .into_iter()
            .filter(|counter| !counter.is_invalid())
        {
            // Hàm PDH ghi đúng kiểu phần tử và đặt tên vào cùng bộ đệm.
            let Some((items, text)) = (unsafe {
                self.buffer.read(|size, count, buffer| {
                    PdhGetRawCounterArrayW(counter, size, count, buffer)
                })
            }) else {
                continue;
            };
            let mut disks = Vec::new();
            for item in items {
                if let Some((number, volumes)) = disk_instance(text, item.szName) {
                    disk_entry(&mut disks, number, volumes);
                }
            }
            if !disks.is_empty() {
                disks.sort_unstable_by(|a, b| a.id.cmp(&b.id));
                return disks;
            }
        }
        Vec::new()
    }
}

#[derive(Default)]
struct CounterBuffer {
    words: Vec<u64>,
}

impl CounterBuffer {
    // Hàm đọc phải ghi các phần tử T hợp lệ vào đầu bộ đệm và trả kích thước đã dùng.
    unsafe fn read<T>(
        &mut self,
        mut read: impl FnMut(&mut u32, &mut u32, Option<*mut T>) -> u32,
    ) -> Option<(&[T], &[u16])> {
        let mut len = size_of_val(self.words.as_slice()) as u32;
        let mut count = 0;
        if len != 0 {
            let status = read(&mut len, &mut count, Some(self.words.as_mut_ptr().cast()));
            if status == 0 {
                // PDH báo thành công, các phần tử đã được ghi.
                return unsafe { self.items(len, count) };
            }
            if !matches!(status, PDH_MORE_DATA | PDH_INVALID_ARGUMENT) {
                return None;
            }
        }
        // Khi bộ đệm nhỏ, chỉ tin kích thước trả về từ lần hỏi với con trỏ NULL.
        len = 0;
        if read(&mut len, &mut count, None) != PDH_MORE_DATA || len == 0 {
            return None;
        }
        self.words
            .resize((len as usize).div_ceil(size_of::<u64>()), 0);
        len = size_of_val(self.words.as_slice()) as u32;
        if read(&mut len, &mut count, Some(self.words.as_mut_ptr().cast())) != 0 {
            return None;
        }
        // PDH báo thành công, các phần tử đã được ghi.
        unsafe { self.items(len, count) }
    }

    // Vùng đầu bộ đệm phải chứa đúng count phần tử T do PDH ghi.
    unsafe fn items<T>(&self, len: u32, count: u32) -> Option<(&[T], &[u16])> {
        let len = len as usize;
        let count = count as usize;
        let items_len = count.checked_mul(size_of::<T>())?;
        if len > size_of_val(self.words.as_slice())
            || items_len > len
            || std::mem::align_of::<T>() > std::mem::align_of::<u64>()
        {
            return None;
        }
        let base = self.words.as_ptr();
        // Các lời gọi PDH thành công đã ghi mảng T hợp lệ, sau đó là các tên UTF-16.
        Some(unsafe {
            (
                std::slice::from_raw_parts(base.cast(), count),
                std::slice::from_raw_parts(
                    base.cast::<u8>().add(items_len).cast(),
                    (len - items_len) / size_of::<u16>(),
                ),
            )
        })
    }
}

fn disk_instance(text: &[u16], name: PWSTR) -> Option<(u32, &[u16])> {
    let offset = (name.0 as usize).checked_sub(text.as_ptr() as usize)?;
    if offset % size_of::<u16>() != 0 {
        return None;
    }
    let rest = text.get(offset / size_of::<u16>()..)?;
    let end = rest.iter().position(|&unit| unit == 0)?;
    let instance = &rest[..end];
    let split = instance
        .iter()
        .position(|&unit| !(48..=57).contains(&unit))
        .unwrap_or(end);
    if split == 0 || (split < end && !matches!(instance[split], 9 | 32)) {
        return None;
    }
    let number = instance[..split].iter().try_fold(0u32, |number, &digit| {
        number.checked_mul(10)?.checked_add(u32::from(digit - 48))
    })?;
    Some((number, &instance[split..]))
}

fn disk_entry<'a>(disks: &'a mut Vec<Disk>, number: u32, volumes: &[u16]) -> &'a mut Disk {
    if let Some(index) = disks.iter().position(|disk| disk.id.parse() == Ok(number)) {
        return &mut disks[index];
    }
    let mut name = tr!("metrics.disk_name", number = number);
    let volumes = String::from_utf16_lossy(volumes);
    let volumes = volumes.trim();
    if !volumes.is_empty() {
        name.push_str(" (");
        name.push_str(volumes);
        name.push(')');
    }
    disks.push(Disk {
        id: number.to_string(),
        name,
        active: 0.0,
        read: 0.0,
        write: 0.0,
    });
    let index = disks.len() - 1;
    &mut disks[index]
}

struct IfTable(*mut MIB_IF_TABLE2);

impl Drop for IfTable {
    fn drop(&mut self) {
        // Bảng do GetIfTable2 cấp phát và chỉ được giải phóng ở đây.
        unsafe { FreeMibTable(self.0.cast()) };
    }
}

struct NetCounters {
    received: u64,
    sent: u64,
    sampled_at: Instant,
}

struct NetNames {
    alias: Box<[u16]>,
    description: Box<[u16]>,
    display: Option<(String, String)>,
}

impl NetNames {
    fn new(alias: &[u16], description: &[u16]) -> Self {
        let display = (!virtual_name(description)).then(|| {
            (
                String::from_utf16_lossy(alias),
                String::from_utf16_lossy(description),
            )
        });
        Self {
            alias: alias.into(),
            description: description.into(),
            display,
        }
    }
}

struct NetAdapter {
    luid: u64,
    index: u32,
    id: String,
    names: Option<NetNames>,
    previous: Option<NetCounters>,
}

impl NetAdapter {
    fn sample(&mut self, row: &MIB_IF_ROW2, now: Instant) -> Option<Net> {
        let previous = self.previous.take();
        if row.OperStatus != IfOperStatusUp || !physical_interface(row) {
            return None;
        }

        let alias = wide_text(&row.Alias);
        let description = wide_text(&row.Description);
        if self.names.as_ref().is_none_or(|names| {
            names.alias.as_ref() != alias || names.description.as_ref() != description
        }) {
            self.names = Some(NetNames::new(alias, description));
        }
        let (name, desc) = self.names.as_ref()?.display.as_ref()?;

        self.previous = Some(NetCounters {
            received: row.InOctets,
            sent: row.OutOctets,
            sampled_at: now,
        });
        let (rx, tx) = previous.map_or((0.0, 0.0), |prev| {
            let seconds = (now - prev.sampled_at).as_secs_f64();
            if seconds == 0.0 {
                return (0.0, 0.0);
            }
            (
                row.InOctets.saturating_sub(prev.received) as f64 / seconds,
                row.OutOctets.saturating_sub(prev.sent) as f64 / seconds,
            )
        });
        let kind = match row.Type {
            IF_TYPE_IEEE80211 => "wifi",
            IF_TYPE_ETHERNET_CSMACD => "ethernet",
            _ => "other",
        };
        let link_bps = [row.ReceiveLinkSpeed, row.TransmitLinkSpeed]
            .into_iter()
            .filter(|&speed| speed != u64::MAX)
            .max()
            .unwrap_or(0);

        // Giá trị trả về sở hữu các chuỗi; tên UTF-16 chỉ được giải mã khi thay đổi.
        Some(Net {
            id: self.id.clone(),
            name: name.clone(),
            desc: desc.clone(),
            kind: kind.into(),
            rx,
            tx,
            link_bps,
        })
    }
}

fn physical_interface(row: &MIB_IF_ROW2) -> bool {
    // Các bit 0 và 1 lần lượt là HardwareInterface và FilterInterface.
    row.InterfaceAndOperStatusFlags._bitfield & 0b11 == 1 && row.Type != IF_TYPE_SOFTWARE_LOOPBACK
}

const NET_REFRESH_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Default)]
struct NetSampler {
    adapters: Vec<NetAdapter>,
    refreshed_at: Option<Instant>,
}

impl NetSampler {
    fn sample(&mut self) -> Vec<Net> {
        if self
            .refreshed_at
            .is_none_or(|at| at.elapsed() >= NET_REFRESH_INTERVAL)
        {
            if let Some(nets) = self.refresh() {
                return nets;
            }
            if self.refreshed_at.is_some() {
                self.refreshed_at = Some(Instant::now());
            }
        }

        let mut nets = Vec::new();
        for adapter in &mut self.adapters {
            let mut row = MIB_IF_ROW2 {
                InterfaceLuid: NET_LUID_LH {
                    Value: adapter.luid,
                },
                InterfaceIndex: adapter.index,
                ..Default::default()
            };
            // Với giao diện chưa có mốc, hỏi trạng thái trước để tránh đọc bộ đếm khi đang ngắt.
            // Cả hai API ghi đồng bộ vào cấu trúc đã điền LUID và chỉ số.
            if adapter.previous.is_none()
                && unsafe { GetIfEntry2Ex(MibIfEntryNormalWithoutStatistics, &mut row) } == NO_ERROR
                && (row.OperStatus != IfOperStatusUp || !physical_interface(&row))
            {
                continue;
            }
            // Dùng lời gọi đầy đủ khi giao diện đang hoạt động hoặc không đọc được trạng thái.
            if unsafe { GetIfEntry2(&mut row) } != NO_ERROR {
                adapter.previous = None;
                continue;
            }
            nets.extend(adapter.sample(&row, Instant::now()));
        }
        nets
    }

    fn refresh(&mut self) -> Option<Vec<Net>> {
        let mut table = std::ptr::null_mut();
        // Khi thành công, Windows trả về bảng gồm NumEntries hàng.
        if unsafe { GetIfTable2(&mut table) } != NO_ERROR || table.is_null() {
            return None;
        }
        let table = IfTable(table);
        let now = Instant::now();
        self.refreshed_at = Some(now);
        // Table là mảng đuôi do Windows cấp phát, kể cả phần đệm trước hàng đầu tiên.
        let rows = unsafe {
            let first = table.0.cast::<u8>().add(offset_of!(MIB_IF_TABLE2, Table));
            std::slice::from_raw_parts(first.cast::<MIB_IF_ROW2>(), (*table.0).NumEntries as usize)
        };
        let mut previous = take(&mut self.adapters);
        let mut nets = Vec::new();
        // Giữ cả giao diện vật lý đang ngắt kết nối để nhận ra khi nó hoạt động trở lại.
        for row in rows.iter().filter(|row| physical_interface(row)) {
            // Value là biểu diễn nguyên 64 bit của LUID hiệu lực trong lần khởi động này.
            let luid = unsafe { row.InterfaceLuid.Value };
            let mut adapter = previous
                .iter()
                .position(|adapter| adapter.luid == luid)
                .map(|index| previous.swap_remove(index))
                .unwrap_or_else(|| NetAdapter {
                    luid,
                    index: row.InterfaceIndex,
                    id: luid.to_string(),
                    names: None,
                    previous: None,
                });
            adapter.index = row.InterfaceIndex;
            nets.extend(adapter.sample(row, now));
            self.adapters.push(adapter);
        }
        self.adapters.sort_unstable_by(|a, b| a.id.cmp(&b.id));
        nets.sort_unstable_by(|a, b| a.id.cmp(&b.id));
        Some(nets)
    }
}

fn wide_text(buf: &[u16]) -> &[u16] {
    &buf[..buf.iter().position(|&unit| unit == 0).unwrap_or(buf.len())]
}

fn virtual_name(name: &[u16]) -> bool {
    const FILTERS: &[&[u8]] = &[
        b"-WFP",
        b"Npcap",
        b"QoS Packet Scheduler",
        b"Native WiFi Filter",
        b"Virtual",
        b"Hyper-V",
        b"VMware",
    ];
    FILTERS.iter().any(|filter| {
        name.windows(filter.len()).any(|window| {
            window.iter().zip(*filter).all(|(&unit, letter)| {
                u8::try_from(unit).is_ok_and(|unit| unit.eq_ignore_ascii_case(letter))
            })
        })
    })
}

fn battery() -> Option<Battery> {
    let mut state = SYSTEM_POWER_STATUS::default();
    // Cấu trúc trạng thái nguồn có kích thước cố định.
    unsafe { GetSystemPowerStatus(&mut state) }.ok()?;
    if state.BatteryFlag & 128 != 0 || state.BatteryLifePercent > 100 {
        return None;
    }
    Some(Battery {
        percent: state.BatteryLifePercent,
        charging: state.BatteryFlag & 8 != 0,
        plugged: state.ACLineStatus == 1,
    })
}

fn uptime_ms() -> u64 {
    // Đồng hồ Windows đếm mili giây từ lúc khởi động và không cấp phát.
    unsafe { GetTickCount64() }
}

fn now_ms() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_millis() as i64,
        Err(error) => -(error.duration().as_millis() as i64),
    }
}

pub struct MetricsSampler {
    cpu: CpuSampler,
    disk_query: DiskQuery,
    net: NetSampler,
}

impl MetricsSampler {
    /// Mở truy vấn đĩa và lấy mốc đầu tiên cho các bộ đếm PDH.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            cpu: CpuSampler::default(),
            disk_query: DiskQuery::new(),
            net: NetSampler::default(),
        }
    }

    /// Lấy số liệu hiện tại. CPU và mạng cần mẫu trước để tính chênh lệch.
    pub fn sample(&mut self) -> Metrics {
        let t = now_ms();
        let cpu = self.cpu.sample();
        let (mem, counts) = memory_and_counts();
        Metrics {
            t,
            cpu,
            mem,
            disks: self.disk_query.sample(),
            nets: self.net.sample(),
            counts,
            uptime_s: uptime_ms() / 1000,
            battery: battery(),
        }
    }
}

impl Drop for MetricsSampler {
    fn drop(&mut self) {
        if !self.disk_query.query.is_invalid() {
            // Đóng truy vấn cũng giải phóng toàn bộ bộ đếm thuộc truy vấn.
            let _ = unsafe { PdhCloseQuery(self.disk_query.query) };
        }
    }
}

fn read_u32(buf: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_ne_bytes(
        buf.get(offset..)?.first_chunk().copied()?,
    ))
}

fn physical_cores() -> u32 {
    let mut len = 0u32;
    // Hỏi kích thước trước, sau đó dùng bộ đệm căn chỉnh đủ cho cấu trúc Windows.
    let _ = unsafe { GetLogicalProcessorInformationEx(RelationProcessorCore, None, &mut len) };
    if len == 0 {
        return 0;
    }
    let mut words = vec![0u64; (len as usize).div_ceil(size_of::<u64>())];
    // Windows chỉ ghi tối đa len byte vào vùng nhớ có thể ghi đã cấp phát.
    let result = unsafe {
        GetLogicalProcessorInformationEx(
            RelationProcessorCore,
            Some(words.as_mut_ptr().cast()),
            &mut len,
        )
    };
    if result.is_err() || len as usize > size_of_val(words.as_slice()) {
        return 0;
    }
    // Mỗi bản ghi có header 8 byte và chiều dài riêng.
    let mut records = unsafe { std::slice::from_raw_parts(words.as_ptr().cast(), len as usize) };
    let mut count = 0;
    while let Some(len) = read_u32(
        records,
        offset_of!(SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX, Size),
    ) {
        let len = len as usize;
        if len < 8 || len > records.len() {
            break;
        }
        count += 1;
        records = &records[len..];
    }
    count
}

fn hostname() -> String {
    let mut buf = [0u16; 256];
    let mut len = buf.len() as u32;
    // Bộ đệm dư chỗ cho tên máy DNS vật lý và dấu NUL.
    let result = unsafe {
        GetComputerNameExW(
            ComputerNamePhysicalDnsHostname,
            Some(PWSTR(buf.as_mut_ptr())),
            &mut len,
        )
    };
    if result.is_ok() {
        String::from_utf16_lossy(wide_text(&buf))
    } else {
        String::new()
    }
}

fn os_name(hklm: &RegKey) -> String {
    let Ok(key) = hklm.open_subkey_with_flags(
        r"SOFTWARE\Microsoft\Windows NT\CurrentVersion",
        KEY_READ | KEY_WOW64_64KEY,
    ) else {
        return String::new();
    };
    let mut product: String = key.get_value("ProductName").unwrap_or_default();
    let version: String = key.get_value("DisplayVersion").unwrap_or_default();
    let mut build: String = key.get_value("CurrentBuild").unwrap_or_default();
    let ubr: u32 = key.get_value("UBR").unwrap_or(0);
    if build.parse::<u32>().is_ok_and(|build| build >= 22000) {
        if let Some(edition) = product.strip_prefix("Windows 10") {
            product = format!("Windows 11{edition}");
        }
    }
    let number: u32 = build.parse().unwrap_or(0);
    if ubr > 0 && number > 0 {
        build = format!("{number}.{ubr}");
    }
    [product, version, build]
        .into_iter()
        .map(|part| part.trim().to_owned())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

struct PhysicalDrive(HANDLE);

impl PhysicalDrive {
    fn open(id: &str) -> Option<Self> {
        let path: Vec<_> = format!(r"\\.\PhysicalDrive{id}")
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        // Mở handle chỉ để hỏi thuộc tính, không xin quyền đọc hoặc ghi dữ liệu.
        unsafe {
            CreateFileW(
                PCWSTR(path.as_ptr()),
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                FILE_FLAGS_AND_ATTRIBUTES(0),
                None,
            )
        }
        .ok()
        .map(Self)
    }

    fn ioctl<'a>(
        &self,
        code: u32,
        query: Option<&STORAGE_PROPERTY_QUERY>,
        output: &'a mut [u8],
    ) -> windows::core::Result<&'a [u8]> {
        let mut returned = 0;
        // Driver chỉ đọc truy vấn và ghi tối đa số byte đã cấp trong lời gọi đồng bộ.
        unsafe {
            DeviceIoControl(
                self.0,
                code,
                query.map(|query| std::ptr::from_ref(query).cast()),
                query.map_or(0, |_| size_of::<STORAGE_PROPERTY_QUERY>() as u32),
                Some(output.as_mut_ptr().cast()),
                output.len() as u32,
                Some(&mut returned),
                None,
            )
        }?;
        Ok(&output[..(returned as usize).min(output.len())])
    }

    fn property<'a>(
        &self,
        property: STORAGE_PROPERTY_ID,
        output: &'a mut [u8],
    ) -> Option<&'a [u8]> {
        let query = STORAGE_PROPERTY_QUERY {
            PropertyId: property,
            QueryType: PropertyStandardQuery,
            ..Default::default()
        };
        self.ioctl(IOCTL_STORAGE_QUERY_PROPERTY, Some(&query), output)
            .ok()
    }

    fn model(&self) -> Option<String> {
        let mut header = [0; size_of::<STORAGE_DESCRIPTOR_HEADER>()];
        let header = self.property(StorageDeviceProperty, &mut header)?;
        let size = read_u32(header, offset_of!(STORAGE_DESCRIPTOR_HEADER, Size))? as usize;
        let vendor_offset = offset_of!(STORAGE_DEVICE_DESCRIPTOR, VendorIdOffset);
        let product_offset = offset_of!(STORAGE_DEVICE_DESCRIPTOR, ProductIdOffset);
        if !(product_offset + size_of::<u32>()..=1024 * 1024).contains(&size) {
            return None;
        }
        let mut buf = vec![0; size];
        let descriptor = self.property(StorageDeviceProperty, &mut buf)?;
        let vendor = descriptor_text(descriptor, vendor_offset).unwrap_or_default();
        let product = descriptor_text(descriptor, product_offset).unwrap_or_default();
        if vendor.is_empty()
            || product
                .to_ascii_lowercase()
                .starts_with(&vendor.to_ascii_lowercase())
        {
            Some(product)
        } else if product.is_empty() {
            Some(vendor)
        } else {
            Some(format!("{vendor} {product}"))
        }
    }

    fn size(&self) -> u64 {
        let mut length = [0; size_of::<GET_LENGTH_INFORMATION>()];
        let length = self.ioctl(IOCTL_DISK_GET_LENGTH_INFO, None, &mut length);
        if let Some(size) = length.ok().and_then(|bytes| disk_length(bytes, 0)) {
            return size;
        }
        // Một số driver yêu cầu quyền đọc cho GET_LENGTH_INFO; GEOMETRY_EX cũng trả kích thước.
        let mut geometry = [0; size_of::<DISK_GEOMETRY_EX>()];
        self.ioctl(IOCTL_DISK_GET_DRIVE_GEOMETRY_EX, None, &mut geometry)
            .ok()
            .and_then(|bytes| disk_length(bytes, offset_of!(DISK_GEOMETRY_EX, DiskSize)))
            .unwrap_or(0)
    }

    fn is_ssd(&self) -> bool {
        let mut buf = [0; size_of::<DEVICE_SEEK_PENALTY_DESCRIPTOR>()];
        let offset = offset_of!(DEVICE_SEEK_PENALTY_DESCRIPTOR, IncursSeekPenalty);
        self.property(StorageDeviceSeekPenaltyProperty, &mut buf)
            .and_then(|bytes| bytes.get(offset))
            == Some(&0)
    }
}

impl Drop for PhysicalDrive {
    fn drop(&mut self) {
        // Handle đĩa thuộc về duy nhất đối tượng này.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

fn disk_length(buf: &[u8], offset: usize) -> Option<u64> {
    let length = i64::from_ne_bytes(buf.get(offset..)?.first_chunk().copied()?);
    u64::try_from(length).ok().filter(|&length| length != 0)
}

fn descriptor_text(descriptor: &[u8], field: usize) -> Option<String> {
    let offset = read_u32(descriptor, field)? as usize;
    if offset < size_of::<STORAGE_DESCRIPTOR_HEADER>() {
        return None;
    }
    let bytes = descriptor.get(offset..)?;
    let end = bytes.iter().position(|&byte| byte == 0)?;
    Some(String::from_utf8_lossy(&bytes[..end]).trim().to_string())
}

/// Đọc thông tin tĩnh sau khi bộ lấy mẫu đã khởi tạo PDH để tránh chờ nạp bộ đếm lần đầu.
pub fn system_info() -> SystemInfo {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let cpu_key = hklm
        .open_subkey(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0")
        .ok();
    let cpu_name: String = cpu_key
        .as_ref()
        .and_then(|key| key.get_value("ProcessorNameString").ok())
        .unwrap_or_default();
    let base_mhz = cpu_key
        .and_then(|key| key.get_value("~MHz").ok())
        .unwrap_or_default();

    let mut sampler = MetricsSampler::new();
    let disks = sampler
        .disk_query
        .instances()
        .into_iter()
        .map(|disk| {
            let drive = PhysicalDrive::open(&disk.id);
            DiskInfo {
                id: disk.id,
                name: disk.name,
                model: drive
                    .as_ref()
                    .and_then(PhysicalDrive::model)
                    .unwrap_or_default(),
                size: drive.as_ref().map_or(0, PhysicalDrive::size),
                ssd: drive.as_ref().is_some_and(PhysicalDrive::is_ssd),
            }
        })
        .collect();

    SystemInfo {
        cpu_name: cpu_name.trim().to_owned(),
        cores: physical_cores(),
        logical: logical_count(),
        base_mhz,
        mem_total: physical_memory().0,
        smbios: super::smbios::read(),
        disks,
        os: os_name(&hklm),
        hostname: hostname(),
        boot_ms: now_ms().saturating_sub(uptime_ms() as i64),
        os_installed_ms: hklm
            .open_subkey_with_flags(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion", KEY_READ | KEY_WOW64_64KEY)
            .and_then(|k| k.get_value::<u32, _>("InstallDate"))
            .map_or(0, |s| i64::from(s) * 1000),
        user: std::env::var("USERNAME").unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set_wide(field: &mut [u16], value: &str) {
        for (unit, ch) in field.iter_mut().zip(value.encode_utf16().chain([0])) {
            *unit = ch;
        }
    }

    #[test]
    fn renamed_hardware_adapter_keeps_sampling() {
        let mut adapter = NetAdapter {
            luid: 1,
            index: 1,
            id: "1".into(),
            names: None,
            previous: None,
        };
        let mut row = MIB_IF_ROW2 {
            OperStatus: IfOperStatusUp,
            Type: IF_TYPE_ETHERNET_CSMACD,
            ..Default::default()
        };
        row.InterfaceAndOperStatusFlags._bitfield = 1;
        set_wide(&mut row.Alias, "Ethernet");
        set_wide(&mut row.Description, "Ethernet Controller");

        let now = Instant::now();
        assert!(adapter.sample(&row, now).is_some());

        set_wide(&mut row.Alias, "Virtual Lab Uplink");
        row.InOctets = 100;
        row.OutOctets = 200;
        let sample = adapter.sample(&row, now + Duration::from_secs(1)).unwrap();
        assert_eq!(sample.name, "Virtual Lab Uplink");
        assert_eq!(sample.desc, "Ethernet Controller");
        assert_eq!((sample.rx, sample.tx), (100.0, 200.0));

        set_wide(&mut row.Description, "Virtual Ethernet");
        assert!(adapter.sample(&row, now + Duration::from_secs(2)).is_none());
    }
}
