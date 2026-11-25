// Lấy thông tin GPU từ DXGI và đo tải từng engine, VRAM qua PDH.
use crate::core::util::from_wide_buf;
use serde::Serialize;
use std::collections::HashMap;
use std::mem::size_of;
use std::slice;

use windows::core::{w, PCWSTR};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE,
};
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
    PdhOpenQueryW, PDH_CSTATUS_NEW_DATA, PDH_CSTATUS_VALID_DATA, PDH_FMT,
    PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_FMT_LARGE, PDH_HCOUNTER, PDH_HQUERY,
    PDH_MORE_DATA,
};

const ENGINE_NAMES: [&str; 4] = ["3D", "Copy", "Video Decode", "Video Encode"];
const ENGINE_TYPES: [&[u8]; 4] = [b"3D", b"Copy", b"VideoDecode", b"VideoEncode"];
// Cờ định dạng trong pdh.h; windows 0.62 chưa có hai hằng này.
const PDH_FMT_NOSCALE: u32 = 0x1000;
const PDH_FMT_NOCAP100: u32 = 0x8000;
const UTIL_FORMAT: PDH_FMT = PDH_FMT(PDH_FMT_DOUBLE.0 | PDH_FMT_NOCAP100);
const BYTE_FORMAT: PDH_FMT = PDH_FMT(PDH_FMT_LARGE.0 | PDH_FMT_NOSCALE | PDH_FMT_NOCAP100);

#[derive(Serialize, Clone)]
pub struct Engine {
    pub name: String,
    pub util: f32,
}

#[derive(Serialize, Clone)]
pub struct GpuMetrics {
    pub id: String,
    pub name: String,
    pub util: f32,
    pub engines: Vec<Engine>,
    pub ded_used: u64,
    pub ded_total: u64,
    pub shared_used: u64,
    pub shared_total: u64,
}

#[derive(Serialize, Clone)]
pub struct GpuInfo {
    pub id: String,
    pub name: String,
    pub ded_total: u64,
    pub shared_total: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Luid {
    high: u32,
    low: u32,
}

struct EngineTotal {
    group: usize,
    util: f64,
}

struct Adapter {
    luid: Luid,
    info: GpuInfo,
    groups: Vec<Engine>,
    engines: HashMap<u32, EngineTotal>,
    ded_used: u64,
    shared_used: u64,
}

impl Adapter {
    fn new(luid: Luid, info: GpuInfo) -> Self {
        Self {
            luid,
            info,
            groups: ENGINE_NAMES
                .iter()
                .map(|name| Engine {
                    name: (*name).into(),
                    util: 0.0,
                })
                .collect(),
            engines: HashMap::new(),
            ded_used: 0,
            shared_used: 0,
        }
    }

    fn reset(&mut self) {
        for engine in self.engines.values_mut() {
            engine.util = 0.0;
        }
        for group in &mut self.groups {
            group.util = 0.0;
        }
        self.ded_used = 0;
        self.shared_used = 0;
    }

    fn add_engine(&mut self, index: u32, raw_type: &[u16], util: f64) {
        if util <= 0.0 {
            return;
        }
        let engine = self.engines.entry(index).or_insert_with(|| {
            let group = ENGINE_TYPES
                .iter()
                .position(|name| eq_ascii(raw_type, name))
                .or_else(|| {
                    self.groups[ENGINE_NAMES.len()..]
                        .iter()
                        .position(|kind| kind.name.encode_utf16().eq(raw_type.iter().copied()))
                        .map(|index| index + ENGINE_NAMES.len())
                })
                .unwrap_or_else(|| {
                    let index = self.groups.len();
                    self.groups.push(Engine {
                        name: String::from_utf16_lossy(raw_type),
                        util: 0.0,
                    });
                    index
                });
            EngineTotal { group, util: 0.0 }
        });
        engine.util += util;
    }

    fn metrics(&mut self) -> GpuMetrics {
        // Mỗi nhóm hiển thị mức tải cao nhất của một engine trong nhóm.
        let mut util = 0.0_f32;
        for total in self.engines.values() {
            let value = percent(total.util);
            let group = &mut self.groups[total.group];
            group.util = group.util.max(value);
            util = util.max(value);
        }
        GpuMetrics {
            id: self.info.id.clone(),
            name: self.info.name.clone(),
            util,
            engines: self.groups.clone(),
            ded_used: self.ded_used,
            ded_total: self.info.ded_total,
            shared_used: self.shared_used,
            shared_total: self.info.shared_total,
        }
    }
}

struct PdhQuery {
    handle: PDH_HQUERY,
    engine: PDH_HCOUNTER,
    dedicated: PDH_HCOUNTER,
    shared: PDH_HCOUNTER,
}

// Các handle thuộc riêng một truy vấn PDH, có thể chuyển luồng. Mỗi lần lấy mẫu
// cần mượn độc quyền sampler; không có bản sao handle dùng song song.
unsafe impl Send for PdhQuery {}

impl PdhQuery {
    fn open() -> Option<Self> {
        let mut handle = PDH_HQUERY::default();
        // Nguồn NULL chọn bộ đếm của máy hiện tại; PDH điền handle vào biến cục bộ.
        if unsafe { PdhOpenQueryW(PCWSTR::null(), 0, &mut handle) } != 0 || handle.is_invalid() {
            return None;
        }
        Some(Self {
            handle,
            engine: add_counter(handle, w!(r"\GPU Engine(*)\Utilization Percentage")),
            dedicated: add_counter(handle, w!(r"\GPU Adapter Memory(*)\Dedicated Usage")),
            shared: add_counter(handle, w!(r"\GPU Adapter Memory(*)\Shared Usage")),
        })
    }

    fn collect(&self) -> bool {
        // Handle còn hiệu lực và chỉ được dùng tuần tự.
        unsafe { PdhCollectQueryData(self.handle) == 0 }
    }
}

fn add_counter(handle: PDH_HQUERY, path: PCWSTR) -> PDH_HCOUNTER {
    let mut counter = PDH_HCOUNTER::default();
    // Đường dẫn UTF-16 có ký tự kết thúc; vòng đời bộ đếm theo truy vấn.
    if unsafe { PdhAddEnglishCounterW(handle, path, 0, &mut counter) } == 0 {
        counter
    } else {
        PDH_HCOUNTER::default()
    }
}

type EngineInstances = HashMap<(u32, Luid, u32), f64>;

pub struct GpuSampler {
    query: Option<PdhQuery>,
    adapters: Vec<Adapter>,
    buffer: Vec<u64>,
    process_engines: EngineInstances,
}

impl GpuSampler {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let adapters: Vec<_> = enumerate_adapters()
            .into_iter()
            .map(|(luid, info)| Adapter::new(luid, info))
            .collect();
        let query = if adapters.is_empty() {
            None
        } else {
            PdhQuery::open()
        };
        if let Some(query) = &query {
            query.collect();
        }
        Self {
            query,
            adapters,
            buffer: Vec::new(),
            process_engines: HashMap::new(),
        }
    }

    pub fn info(&self) -> Vec<GpuInfo> {
        self.adapters
            .iter()
            .map(|adapter| adapter.info.clone())
            .collect()
    }

    /// Trả về chỉ số GPU theo thứ tự ID và mức tải engine cao nhất theo PID.
    pub fn sample(&mut self) -> (Vec<GpuMetrics>, HashMap<u32, f32>) {
        let mut processes = HashMap::new();
        self.process_engines.clear();
        for adapter in &mut self.adapters {
            adapter.reset();
        }
        if let Some(query) = &self.query {
            if query.collect() {
                collect_engines(
                    counter_array(&mut self.buffer, query.engine, UTIL_FORMAT),
                    &mut self.adapters,
                    &mut self.process_engines,
                );
                collect_memory(
                    counter_array(&mut self.buffer, query.dedicated, BYTE_FORMAT),
                    &mut self.adapters,
                    |adapter| &mut adapter.ded_used,
                );
                collect_memory(
                    counter_array(&mut self.buffer, query.shared, BYTE_FORMAT),
                    &mut self.adapters,
                    |adapter| &mut adapter.shared_used,
                );
                for (&(pid, _, _), &util) in &self.process_engines {
                    let process = processes.entry(pid).or_insert(0.0_f32);
                    *process = process.max(percent(util));
                }
            }
        }
        (
            self.adapters.iter_mut().map(Adapter::metrics).collect(),
            processes,
        )
    }
}

impl Drop for GpuSampler {
    fn drop(&mut self) {
        if let Some(query) = self.query.take() {
            // Đóng truy vấn do sampler sở hữu cũng giải phóng mọi bộ đếm.
            unsafe { PdhCloseQuery(query.handle) };
        }
    }
}

/// Liệt kê thông tin GPU từ DXGI mà không khởi tạo truy vấn PDH.
pub fn adapter_info() -> Vec<GpuInfo> {
    enumerate_adapters()
        .into_iter()
        .map(|(_, info)| info)
        .collect()
}

fn enumerate_adapters() -> Vec<(Luid, GpuInfo)> {
    let mut adapters = Vec::new();
    // DXGI trả về giao diện sở hữu, được giải phóng khi rời hàm.
    let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else {
        return adapters;
    };
    for index in 0..u32::MAX {
        // Chỉ số vượt danh sách khiến DXGI trả lỗi và kết thúc liệt kê.
        let Ok(adapter) = (unsafe { factory.EnumAdapters1(index) }) else {
            break;
        };
        // Giao diện adapter vẫn còn hiệu lực trong lần đọc mô tả.
        let Ok(desc) = (unsafe { adapter.GetDesc1() }) else {
            continue;
        };
        if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
            continue;
        }
        let name = from_wide_buf(&desc.Description);
        if name.eq_ignore_ascii_case("Microsoft Basic Render Driver") {
            continue;
        }
        let luid = Luid {
            high: desc.AdapterLuid.HighPart as u32,
            low: desc.AdapterLuid.LowPart,
        };
        let info = GpuInfo {
            id: adapters.len().to_string(),
            name,
            ded_total: desc.DedicatedVideoMemory as u64,
            shared_total: desc.SharedSystemMemory as u64,
        };
        adapters.push((luid, info));
    }
    adapters
}

#[derive(Default)]
struct CounterArray<'a> {
    items: &'a [PDH_FMT_COUNTERVALUE_ITEM_W],
    names: &'a [u16],
}

impl CounterArray<'_> {
    fn name(&self, item: &PDH_FMT_COUNTERVALUE_ITEM_W) -> Option<&[u16]> {
        let offset = item
            .szName
            .0
            .addr()
            .checked_sub(self.names.as_ptr().addr())?;
        if !offset.is_multiple_of(size_of::<u16>()) {
            return None;
        }
        let name = self.names.get(offset / size_of::<u16>()..)?;
        Some(&name[..name.iter().position(|&ch| ch == 0)?])
    }
}

fn counter_array(
    buffer: &mut Vec<u64>,
    counter: PDH_HCOUNTER,
    format: PDH_FMT,
) -> CounterArray<'_> {
    if counter.is_invalid() {
        return CounterArray::default();
    }
    const ITEM_SIZE: usize = size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>();
    for _ in 0..3 {
        let capacity = buffer.len() * size_of::<u64>();
        let mut bytes = capacity as u32;
        let mut count = 0;
        let ptr = buffer.as_mut_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>();
        // Vec<u64> có đủ căn chỉnh; khi chưa có vùng nhớ, PDH chỉ trả kích thước cần.
        let status = unsafe {
            PdhGetFormattedCounterArrayW(
                counter,
                format,
                &mut bytes,
                &mut count,
                (capacity != 0).then_some(ptr),
            )
        };
        if status == 0 {
            let rows = count as usize;
            let Some(item_bytes) = rows.checked_mul(ITEM_SIZE) else {
                break;
            };
            let used = bytes as usize;
            if capacity == 0 || used > capacity || item_bytes > used {
                break;
            }
            // PDH đã điền mảng item rồi các tên UTF-16 trong cùng vùng nhớ.
            // Cả hai lát cắt chỉ đọc và không sống lâu hơn vùng nhớ.
            return unsafe {
                CounterArray {
                    items: slice::from_raw_parts(ptr, rows),
                    names: slice::from_raw_parts(
                        ptr.add(rows).cast::<u16>(),
                        (used - item_bytes) / size_of::<u16>(),
                    ),
                }
            };
        }
        if status != PDH_MORE_DATA {
            break;
        }
        if capacity != 0 {
            // Khi bộ đệm chật, kích thước chỉ đáng tin sau lần hỏi với bytes = 0.
            bytes = 0;
            // Không truyền vùng nhớ, chỉ lấy kích thước cần cho dữ liệu hiện tại.
            if unsafe {
                PdhGetFormattedCounterArrayW(counter, format, &mut bytes, &mut count, None)
            } != PDH_MORE_DATA
            {
                break;
            }
        }
        let required = bytes as usize;
        if required <= capacity {
            break;
        }
        let words = (required + required / 4)
            .min(u32::MAX as usize - 7)
            .div_ceil(size_of::<u64>());
        if buffer.try_reserve(words - buffer.len()).is_err() {
            break;
        }
        buffer.resize(words, 0);
    }
    CounterArray::default()
}

fn valid(item: &PDH_FMT_COUNTERVALUE_ITEM_W) -> bool {
    matches!(
        item.FmtValue.CStatus,
        PDH_CSTATUS_VALID_DATA | PDH_CSTATUS_NEW_DATA
    )
}

fn collect_engines(
    array: CounterArray<'_>,
    adapters: &mut [Adapter],
    process_engines: &mut EngineInstances,
) {
    for item in array.items.iter().filter(|item| valid(item)) {
        let Some((pid, luid, index, kind)) = array.name(item).and_then(parse_engine) else {
            continue;
        };
        let Some(adapter) = adapters.iter_mut().find(|adapter| adapter.luid == luid) else {
            continue;
        };
        // Mảng này được đọc với UTIL_FORMAT nên union chứa doubleValue.
        let util = unsafe { item.FmtValue.Anonymous.doubleValue };
        if util.is_finite() && util >= 0.0 {
            adapter.add_engine(index, kind, util);
            *process_engines.entry((pid, luid, index)).or_default() += util;
        }
    }
}

fn collect_memory(
    array: CounterArray<'_>,
    adapters: &mut [Adapter],
    field: impl Fn(&mut Adapter) -> &mut u64,
) {
    for item in array.items.iter().filter(|item| valid(item)) {
        let Some(luid) = array.name(item).and_then(parse_memory) else {
            continue;
        };
        if let Some(adapter) = adapters.iter_mut().find(|adapter| adapter.luid == luid) {
            // Mảng này được đọc với BYTE_FORMAT nên union chứa largeValue.
            let bytes = unsafe { item.FmtValue.Anonymous.largeValue }.max(0) as u64;
            let used = field(adapter);
            *used = used.saturating_add(bytes);
        }
    }
}

fn percent(util: f64) -> f32 {
    util.clamp(0.0, 100.0) as f32
}

fn eq_ascii(wide: &[u16], ascii: &[u8]) -> bool {
    wide.len() == ascii.len() && wide.iter().zip(ascii).all(|(&w, &a)| w == u16::from(a))
}

fn number(wide: &[u16], radix: u32) -> Option<u32> {
    if wide.is_empty() {
        return None;
    }
    wide.iter().try_fold(0_u32, |number, &ch| {
        let digit = char::from_u32(u32::from(ch))?.to_digit(radix)?;
        number.checked_mul(radix)?.checked_add(digit)
    })
}

fn hex(wide: &[u16]) -> Option<u32> {
    match wide {
        [48, 120 | 88, digits @ ..] => number(digits, 16),
        _ => None,
    }
}

fn parse_adapter<'a>(tokens: &mut impl Iterator<Item = &'a [u16]>) -> Option<Luid> {
    if !eq_ascii(tokens.next()?, b"luid") {
        return None;
    }
    let high = hex(tokens.next()?)?;
    let low = hex(tokens.next()?)?;
    if !eq_ascii(tokens.next()?, b"phys") {
        return None;
    }
    number(tokens.next()?, 10)?;
    Some(Luid { high, low })
}

fn parse_engine(name: &[u16]) -> Option<(u32, Luid, u32, &[u16])> {
    // splitn giữ nguyên phần tên loại engine, kể cả dấu gạch dưới.
    let mut tokens = name.splitn(11, |&ch| ch == u16::from(b'_'));
    if !eq_ascii(tokens.next()?, b"pid") {
        return None;
    }
    let pid = number(tokens.next()?, 10)?;
    let luid = parse_adapter(&mut tokens)?;
    if !eq_ascii(tokens.next()?, b"eng") {
        return None;
    }
    let index = number(tokens.next()?, 10)?;
    if !eq_ascii(tokens.next()?, b"engtype") {
        return None;
    }
    let kind = tokens.next()?;
    (!kind.is_empty()).then_some((pid, luid, index, kind))
}

fn parse_memory(name: &[u16]) -> Option<Luid> {
    let mut tokens = name.split(|&ch| ch == u16::from(b'_'));
    let luid = parse_adapter(&mut tokens)?;
    tokens.next().is_none().then_some(luid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn adapter_info_matches_sampler() {
        let start = Instant::now();
        let info = adapter_info();
        println!("Liệt kê {} GPU qua DXGI: {:?}", info.len(), start.elapsed());

        let start = Instant::now();
        let sampler = GpuSampler::new();
        println!("Khởi tạo bộ lấy mẫu GPU: {:?}", start.elapsed());

        let sampled_info = sampler.info();
        assert_eq!(info.len(), sampled_info.len());
        for (adapter, sampled) in info.iter().zip(&sampled_info) {
            assert_eq!(adapter.id, sampled.id);
            assert_eq!(adapter.name, sampled.name);
            assert_eq!(adapter.ded_total, sampled.ded_total);
            assert_eq!(adapter.shared_total, sampled.shared_total);
        }
    }
}
