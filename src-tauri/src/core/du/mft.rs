// Quét nhanh cả một ổ NTFS bằng cách đọc thẳng bảng tệp MFT (cách của WizTree) thay vì hỏi từng thư mục:
//
// - Mở ổ (\\.\C:, cần quyền Admin – bản phát hành luôn chạy Admin), FSCTL_GET_NTFS_VOLUME_DATA cho vị trí MFT,
//   đọc bản ghi 0 ($MFT) để biết MFT nằm ở những đoạn nào trên đĩa.
// - Nhiều luồng đọc tuần tự các khối 2 MB, không qua bộ nhớ đệm của Windows (không đẩy 2–3 GB vào RAM standby).
//   Mỗi bản ghi (1 KB): sửa "fixup", lấy $STANDARD_INFORMATION (giờ sửa, thuộc tính), $FILE_NAME (tên, thư mục cha),
//   $DATA (kích thước; dung lượng trên đĩa cộng cả luồng phụ như WofCompressedData của tệp nén CompactOS).
// - Tệp chỉ được cộng dồn vào thư mục cha (không giữ tên tệp); chỉ thư mục được giữ lại để dựng cây – cùng
//   loại cây với cách quét thường (`Tree`), nên mọi chức năng xem / xoá / đồng bộ lại dùng chung.
// - Hard link chỉ tính MỘT lần (WinSxS ↔ System32), như WizTree; liệt kê thư mục thì đếm mỗi tên một lần.
// - Thư mục reparse (junction, symlink, thư mục OneDrive…) là nút "liên kết", không tính nội dung – giống cách quét thường.
// - Tệp hệ thống của NTFS ($MFT, $LogFile, $Extend\…) không tính, giống Explorer.
// - Không mở được ổ (không Admin, không phải NTFS, ổ mạng…) -> Err, `Scan::run` quay về cách liệt kê thư mục.
//
// Đọc thẳng đĩa nên thay đổi trong vài giây gần nhất (NTFS chưa ghi xuống) có thể chưa thấy. Mọi chỗ đọc bản ghi
// đều kiểm tra biên (bản phát hành dùng panic = abort): bản ghi hỏng thì bỏ qua, không bao giờ làm app thoát.

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{ft_ms, secs, Scan, Tree, ATTR_REPARSE, F_LISTED, F_LINK, NONE, OPEN_EXISTING};

#[link(name = "kernel32")]
unsafe extern "system" {
    fn DeviceIoControl(h: *mut c_void, code: u32, inb: *const c_void, insz: u32, outb: *mut c_void, outsz: u32, ret: *mut u32, ov: *mut c_void) -> i32;
    fn ReadFile(h: *mut c_void, buf: *mut c_void, n: u32, read: *mut u32, ov: *mut Overlapped) -> i32;
}

const GENERIC_READ: u32 = 0x8000_0000;
const SHARE_RW: u32 = 0x1 | 0x2;
const FILE_FLAG_NO_BUFFERING: u32 = 0x2000_0000;
const FSCTL_GET_NTFS_VOLUME_DATA: u32 = 0x0009_0064;
const ERROR_ACCESS_DENIED: u32 = 5;
/// mỗi lần đọc; 6 luồng × 2 MB = 12 MB bộ đệm khi quét
const READ_BYTES: usize = 2 << 20;
/// 48 bit thấp của tham chiếu bản ghi = số bản ghi
const REF_MASK: u64 = 0xFFFF_FFFF_FFFF;
/// bản ghi của thư mục gốc
const ROOT: u32 = 5;
/// 0..16 là tệp hệ thống của NTFS (trừ gốc)
const FIRST_USER: u64 = 16;

const AT_STANDARD_INFORMATION: u32 = 0x10;
const AT_ATTRIBUTE_LIST: u32 = 0x20;
const AT_FILE_NAME: u32 = 0x30;
const AT_DATA: u32 = 0x80;
const AT_END: u32 = 0xFFFF_FFFF;
const NS_DOS: u8 = 2;

/// "C:\" -> 'C' nếu là ổ cứng / ổ rời (không phải ổ mạng, CD…). Thư mục con dùng cách liệt kê thường.
pub(super) fn drive_letter(root: &str) -> Option<char> {
    let b = root.as_bytes();
    if b.len() != 3 || b[1] != b':' || b[2] != b'\\' || !b[0].is_ascii_alphabetic() {
        return None;
    }
    let w: Vec<u16> = root.encode_utf16().chain([0]).collect();
    let kind = unsafe { super::GetDriveTypeW(w.as_ptr()) };
    matches!(kind, 2 | 3).then(|| char::from(b[0].to_ascii_uppercase()))
}

// ---------------- đọc ổ ----------------

#[repr(C)]
struct Overlapped {
    internal: usize,
    internal_high: usize,
    offset: u32,
    offset_high: u32,
    event: *mut c_void,
}

struct Vol(*mut c_void);

impl Drop for Vol {
    fn drop(&mut self) {
        unsafe {
            super::CloseHandle(self.0);
        }
    }
}

/// Lỗi "bình thường" (không Admin / không phải NTFS) trả chuỗi rỗng: quay về cách thường mà không ghi log.
fn open(letter: char) -> Result<Vol, String> {
    let path: Vec<u16> = format!(r"\\.\{letter}:").encode_utf16().chain([0]).collect();
    let h = unsafe { super::CreateFileW(path.as_ptr(), GENERIC_READ, SHARE_RW, std::ptr::null(), OPEN_EXISTING, FILE_FLAG_NO_BUFFERING, std::ptr::null_mut()) };
    if h as isize == -1 {
        let e = unsafe { super::GetLastError() };
        return Err(if e == ERROR_ACCESS_DENIED { String::new() } else { format!("mở ổ: lỗi {e}") });
    }
    Ok(Vol(h))
}

/// Bộ đệm căn 4096 byte (đọc không qua bộ nhớ đệm cần địa chỉ căn theo sector).
struct ABuf {
    v: Vec<u8>,
    off: usize,
}

impl ABuf {
    fn new(len: usize) -> Self {
        let v = vec![0u8; len + 4096];
        let off = v.as_ptr().align_offset(4096);
        ABuf { v, off }
    }
    fn get(&mut self, len: usize) -> &mut [u8] {
        &mut self.v[self.off..self.off + len]
    }
}

fn read_at(v: &Vol, off: u64, buf: &mut [u8]) -> Result<(), String> {
    let mut ov = Overlapped { internal: 0, internal_high: 0, offset: off as u32, offset_high: (off >> 32) as u32, event: std::ptr::null_mut() };
    let mut got = 0u32;
    let ok = unsafe { ReadFile(v.0, buf.as_mut_ptr().cast(), buf.len() as u32, &mut got, &mut ov) };
    if ok == 0 {
        return Err(format!("đọc ổ: lỗi {}", unsafe { super::GetLastError() }));
    }
    if got as usize != buf.len() {
        return Err("đọc ổ: thiếu dữ liệu".into());
    }
    Ok(())
}

struct Geo {
    cluster: u64,
    rec: usize,
    mft_lcn: u64,
    valid: u64,
}

fn geometry(v: &Vol) -> Result<Geo, String> {
    // NTFS_VOLUME_DATA_BUFFER: 40 BytesPerSector, 44 BytesPerCluster, 48 BytesPerFileRecordSegment,
    // 56 MftValidDataLength, 64 MftStartLcn
    let mut out = [0u64; 16];
    let mut ret = 0u32;
    let ok = unsafe { DeviceIoControl(v.0, FSCTL_GET_NTFS_VOLUME_DATA, std::ptr::null(), 0, out.as_mut_ptr().cast(), 128, &mut ret, std::ptr::null_mut()) };
    if ok == 0 || ret < 72 {
        return Err(String::new()); // không phải NTFS
    }
    let g = Geo { cluster: out[5] >> 32, rec: (out[6] & 0xFFFF_FFFF) as usize, valid: out[7], mft_lcn: out[8] };
    if !matches!(g.rec, 1024 | 2048 | 4096) || g.cluster < 512 || !g.cluster.is_multiple_of(g.rec as u64) || g.cluster > 2 << 20 {
        return Err(format!("bố cục NTFS không hỗ trợ (cluster {}, bản ghi {})", g.cluster, g.rec));
    }
    Ok(g)
}

// ---------------- đọc bản ghi ----------------

fn rd<const N: usize>(b: &[u8], o: usize) -> Option<[u8; N]> {
    b.get(o..o.checked_add(N)?)?.try_into().ok()
}
fn u16_at(b: &[u8], o: usize) -> Option<u16> {
    rd::<2>(b, o).map(u16::from_le_bytes)
}
fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    rd::<4>(b, o).map(u32::from_le_bytes)
}
fn u64_at(b: &[u8], o: usize) -> Option<u64> {
    rd::<8>(b, o).map(u64::from_le_bytes)
}

/// Kiểm tra "FILE" và trả 2 byte cuối mỗi khối 512 byte về giá trị thật (update sequence array).
fn fixup(r: &mut [u8]) -> Option<()> {
    if r.get(0..4)? != b"FILE" {
        return None;
    }
    let ofs = usize::from(u16_at(r, 4)?);
    let cnt = usize::from(u16_at(r, 6)?);
    if cnt < 2 || (cnt - 1) * 512 > r.len() {
        return None;
    }
    let usn = rd::<2>(r, ofs)?;
    for i in 1..cnt {
        let fix = rd::<2>(r, ofs + 2 * i)?;
        let tail = r.get_mut(i * 512 - 2..i * 512)?;
        if tail != usn {
            return None; // ghi dở / hỏng
        }
        tail.copy_from_slice(&fix);
    }
    Some(())
}

/// Giá trị của thuộc tính lưu ngay trong bản ghi (resident).
fn value(a: &[u8]) -> Option<&[u8]> {
    let len = u32_at(a, 0x10)? as usize;
    let off = usize::from(u16_at(a, 0x14)?);
    a.get(off..off.checked_add(len)?)
}

#[derive(Default)]
struct Rec<'a> {
    /// bản ghi chính (bản ghi phụ chứa thuộc tính tràn của một tệp lớn / nhiều hard link); NONE = chính nó
    base: u32,
    dir: bool,
    /// có $ATTRIBUTE_LIST: một phần thuộc tính nằm ở bản ghi phụ
    list: bool,
    /// (giờ sửa – giây Unix, thuộc tính)
    si: Option<(u32, u32)>,
    /// (bản ghi thư mục cha, tên UTF-16LE) của tên đầu tiên không phải tên ngắn DOS
    name: Option<(u32, &'a [u8])>,
    /// kích thước luồng dữ liệu chính
    size: Option<u64>,
    /// dung lượng trên đĩa của mọi luồng dữ liệu
    alloc: u64,
}

fn parse(r: &[u8]) -> Option<Rec<'_>> {
    let flags = u16_at(r, 0x16)?;
    if flags & 1 == 0 {
        return None; // bản ghi trống
    }
    let base = u64_at(r, 0x20)? & REF_MASK;
    let mut x = Rec { base: if base == 0 { NONE } else { u32::try_from(base).ok()? }, dir: flags & 2 != 0, ..Default::default() };
    let end = (u32_at(r, 0x18)? as usize).min(r.len());
    let mut p = usize::from(u16_at(r, 0x14)?);
    while p + 16 <= end {
        let ty = u32_at(r, p)?;
        if ty == AT_END {
            break;
        }
        let len = u32_at(r, p + 4)? as usize;
        if len < 16 || p + len > end {
            break;
        }
        let a = &r[p..p + len];
        let resident = a[8] == 0;
        let unnamed = a[9] == 0;
        match ty {
            AT_STANDARD_INFORMATION if resident => {
                let v = value(a)?;
                x.si = Some((secs(ft_ms(u64_at(v, 0x08)? as i64)), u32_at(v, 0x20)?));
            }
            AT_ATTRIBUTE_LIST => x.list = true,
            AT_FILE_NAME if resident && x.name.is_none() => {
                let v = value(a)?;
                if *v.get(0x41)? != NS_DOS {
                    let n = usize::from(*v.get(0x40)?);
                    let parent = u32::try_from(u64_at(v, 0)? & REF_MASK).ok()?;
                    x.name = Some((parent, v.get(0x42..0x42 + 2 * n)?));
                }
            }
            AT_DATA if resident => {
                if unnamed {
                    x.size = Some(u64::from(u32_at(a, 0x10)?));
                }
            }
            // luồng nằm ngoài bản ghi: số liệu chỉ có ở đoạn đầu (lowest VCN = 0)
            AT_DATA if u64_at(a, 0x10)? == 0 => {
                let mut alloc = u64_at(a, 0x28)?;
                // nén / sparse: dung lượng thật nằm ở "compressed size" (header dài 0x48)
                if u16_at(a, 0x0C)? & 0x8001 != 0 && u16_at(a, 0x20)? >= 0x48 {
                    alloc = u64_at(a, 0x40)?;
                }
                x.alloc += alloc;
                if unnamed {
                    x.size = Some(u64_at(a, 0x30)?);
                }
            }
            _ => {}
        }
        p += len;
    }
    Some(x)
}

/// Mapping pairs -> [(LCN, số cluster)].
fn decode_runs(mut b: &[u8]) -> Option<Vec<(u64, u64)>> {
    let mut v = Vec::new();
    let mut lcn: i64 = 0;
    loop {
        let h = *b.first()?;
        if h == 0 {
            break;
        }
        let (ln, lo) = (usize::from(h & 0xF), usize::from(h >> 4));
        if ln == 0 || ln > 8 || lo == 0 || lo > 8 {
            return None; // MFT không bao giờ sparse
        }
        let body = b.get(1..1 + ln + lo)?;
        let len = body[..ln].iter().rev().fold(0u64, |acc, &x| (acc << 8) | u64::from(x));
        let mut off = body[ln..].iter().rev().fold(0i64, |acc, &x| (acc << 8) | i64::from(x));
        if lo < 8 && body[ln + lo - 1] & 0x80 != 0 {
            off |= -1i64 << (8 * lo); // số âm
        }
        lcn = lcn.checked_add(off)?;
        if lcn < 0 {
            return None;
        }
        v.push((lcn as u64, len));
        b = &b[1 + ln + lo..];
    }
    Some(v)
}

/// Các đoạn của MFT trên đĩa + kích thước MFT.
fn mft_runs(v: &Vol, g: &Geo) -> Result<(Vec<(u64, u64)>, u64), String> {
    let mut buf = ABuf::new(g.cluster as usize);
    let b = buf.get(g.cluster as usize);
    read_at(v, g.mft_lcn * g.cluster, b)?;
    let r = &mut b[..g.rec];
    fixup(r).ok_or("bản ghi $MFT hỏng")?;
    let bad = || "không đọc được danh sách đoạn của $MFT".to_string();
    let end = (u32_at(r, 0x18).ok_or_else(bad)? as usize).min(r.len());
    let mut p = usize::from(u16_at(r, 0x14).ok_or_else(bad)?);
    while p + 16 <= end {
        let ty = u32_at(r, p).ok_or_else(bad)?;
        let len = u32_at(r, p + 4).ok_or_else(bad)? as usize;
        if ty == AT_END || len < 16 || p + len > end {
            break;
        }
        let a = &r[p..p + len];
        if ty == AT_DATA && a[8] != 0 && a[9] == 0 {
            let size = u64_at(a, 0x30).ok_or_else(bad)?;
            let runs = decode_runs(a.get(usize::from(u16_at(a, 0x20).ok_or_else(bad)?)..).ok_or_else(bad)?).ok_or_else(bad)?;
            let clusters: u64 = runs.iter().map(|r| r.1).sum();
            // MFT phân mảnh tới mức cần $ATTRIBUTE_LIST: danh sách ở đây chưa đủ -> dùng cách thường
            if clusters * g.cluster < size {
                return Err("$MFT phân mảnh quá nhiều".into());
            }
            return Ok((runs, size));
        }
        p += len;
    }
    Err(bad())
}

struct Piece {
    vol_off: u64,
    len: usize,
    first_rec: u64,
}

fn pieces(runs: &[(u64, u64)], g: &Geo, records: u64) -> Vec<Piece> {
    let limit = records * g.rec as u64;
    let mut out = Vec::new();
    let mut mft_off = 0u64;
    for &(lcn, n) in runs {
        let (mut off, mut left) = (lcn * g.cluster, n * g.cluster);
        while left > 0 && mft_off < limit {
            let len = left.min(READ_BYTES as u64);
            out.push(Piece { vol_off: off, len: len as usize, first_rec: mft_off / g.rec as u64 });
            off += len;
            left -= len;
            mft_off += len;
        }
    }
    out
}

// ---------------- kết quả của từng luồng ----------------

struct DirRec {
    rec: u32,
    parent: u32,
    mtime: u32,
    link: bool,
    name_off: u32,
    name_len: u32,
}

/// Bản ghi có thuộc tính tràn sang bản ghi phụ: gom lại khi mọi luồng xong.
struct Part {
    base: u32,
    is_base: bool,
    dir: bool,
    si: Option<(u32, u32)>,
    name: Option<(u32, String)>,
    size: Option<u64>,
    alloc: u64,
}

#[derive(Default)]
struct Out {
    dirs: Vec<DirRec>,
    /// tên thư mục, UTF-8 nối liền
    names: Vec<u8>,
    /// (thư mục cha, số tệp, kích thước, dung lượng trên đĩa), đã gộp theo cha trong mỗi khối
    files: Vec<(u32, u32, u64, u64)>,
    parts: Vec<Part>,
}

fn utf16_to(dst: &mut Vec<u8>, b: &[u8]) {
    let units = b.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c));
    let mut tmp = [0u8; 4];
    for c in char::decode_utf16(units) {
        dst.extend_from_slice(c.unwrap_or('\u{FFFD}').encode_utf8(&mut tmp).as_bytes());
    }
}

impl Out {
    fn dir(&mut self, rec: u32, parent: u32, si: Option<(u32, u32)>, name: impl FnOnce(&mut Vec<u8>)) {
        let off = self.names.len();
        name(&mut self.names);
        self.dirs.push(DirRec {
            rec,
            parent,
            mtime: si.map_or(0, |s| s.0),
            link: si.is_some_and(|s| s.1 & ATTR_REPARSE != 0),
            name_off: off as u32,
            name_len: (self.names.len() - off) as u32,
        });
    }
}

fn worker(s: &Scan, letter: char, g: &Geo, pieces: &[Piece], records: u64, next: &AtomicUsize) -> Result<Out, String> {
    let vol = open(letter)?;
    let mut buf = ABuf::new(READ_BYTES);
    let mut out = Out::default();
    let mut by_parent: HashMap<u32, (u32, u64, u64)> = HashMap::new();
    while !s.cancel.load(Ordering::Relaxed) {
        let Some(pc) = pieces.get(next.fetch_add(1, Ordering::Relaxed)) else { break };
        let b = buf.get(pc.len);
        read_at(&vol, pc.vol_off, b)?;
        let (mut nd, mut nf, mut bytes) = (0u64, 0u64, 0u64);
        for (k, r) in b.chunks_exact_mut(g.rec).enumerate() {
            let recno = pc.first_rec + k as u64;
            if recno >= records {
                break;
            }
            if recno < FIRST_USER && recno != u64::from(ROOT) {
                continue;
            }
            if fixup(r).is_none() {
                continue;
            }
            let Some(x) = parse(r) else { continue };
            let recno = recno as u32;
            if x.base != NONE || x.list {
                out.parts.push(Part {
                    base: if x.base == NONE { recno } else { x.base },
                    is_base: x.base == NONE,
                    dir: x.dir,
                    si: x.si,
                    name: x.name.map(|(p, n)| {
                        let mut v = Vec::new();
                        utf16_to(&mut v, n);
                        (p, String::from_utf8(v).unwrap_or_default())
                    }),
                    size: x.size,
                    alloc: x.alloc,
                });
                continue;
            }
            let Some((parent, name)) = x.name else { continue };
            if x.dir {
                out.dir(recno, parent, x.si, |v| utf16_to(v, name));
                nd += 1;
            } else {
                let e = by_parent.entry(parent).or_default();
                let size = x.size.unwrap_or(0);
                *e = (e.0 + 1, e.1 + size, e.2 + x.alloc);
                nf += 1;
                bytes += size;
            }
        }
        out.files.extend(by_parent.drain().map(|(p, (n, sz, al))| (p, n, sz, al)));
        s.dirs.fetch_add(nd, Ordering::Relaxed);
        s.files.fetch_add(nf, Ordering::Relaxed);
        s.bytes.fetch_add(bytes, Ordering::Relaxed);
    }
    Ok(out)
}

/// Gom bản ghi chính + bản ghi phụ của cùng một tệp / thư mục.
fn merge_parts(outs: &mut [Out]) -> Out {
    let mut parts: Vec<Part> = outs.iter_mut().flat_map(|o| o.parts.drain(..)).collect();
    parts.sort_unstable_by_key(|p| (p.base, !p.is_base));
    let mut m = Out::default();
    for group in parts.chunk_by(|a, b| a.base == b.base) {
        let Some(b0) = group.first().filter(|p| p.is_base) else { continue };
        let Some((parent, name)) = group.iter().find_map(|p| p.name.as_ref()) else { continue };
        if b0.dir {
            m.dir(b0.base, *parent, b0.si, |v| v.extend_from_slice(name.as_bytes()));
        } else {
            let size = group.iter().find_map(|p| p.size).unwrap_or(0);
            m.files.push((*parent, 1, size, group.iter().map(|p| p.alloc).sum()));
        }
    }
    m
}

/// Dựng `Tree` từ các thư mục (theo bản ghi cha) và cộng dồn tệp; id theo thứ tự rộng trước nên cha luôn nhỏ hơn con.
fn build(root_name: &str, records: usize, outs: Vec<Out>) -> Tree {
    let mut dirs = Vec::new();
    let mut names = Vec::new();
    let mut files = Vec::new();
    for mut o in outs {
        let base = names.len() as u32;
        names.append(&mut o.names);
        dirs.extend(o.dirs.into_iter().map(|mut d| {
            d.name_off += base;
            d
        }));
        files.push(o.files);
    }
    // bản ghi -> chỉ số trong `dirs`
    let mut idx = vec![NONE; records];
    for (i, d) in dirs.iter().enumerate() {
        if let Some(slot) = idx.get_mut(d.rec as usize) {
            *slot = i as u32;
        }
    }
    let parent_of = |d: &DirRec| if d.rec == ROOT { NONE } else { idx.get(d.parent as usize).copied().unwrap_or(NONE) };
    // con của mỗi thư mục (CSR)
    let mut start = vec![0u32; dirs.len() + 1];
    for d in &dirs {
        let p = parent_of(d);
        if p != NONE {
            start[p as usize + 1] += 1;
        }
    }
    for i in 1..start.len() {
        start[i] += start[i - 1];
    }
    let mut fill = start.clone();
    let mut kids = vec![0u32; dirs.len()];
    for (i, d) in dirs.iter().enumerate() {
        let p = parent_of(d);
        if p != NONE {
            kids[fill[p as usize] as usize] = i as u32;
            fill[p as usize] += 1;
        }
    }
    drop(fill);

    let mut tree = Tree::default();
    let root_i = idx.get(ROOT as usize).copied().unwrap_or(NONE);
    tree.add(NONE, root_name, if root_i == NONE { 0 } else { dirs[root_i as usize].mtime }, false);
    let mut tid = vec![NONE; dirs.len()];
    if root_i != NONE {
        tid[root_i as usize] = 0;
        let mut queue = vec![root_i];
        let mut qi = 0;
        while let Some(&p) = queue.get(qi) {
            qi += 1;
            let p = p as usize;
            if p != root_i as usize && dirs[p].link {
                continue; // không đi vào thư mục liên kết
            }
            for &c in &kids[start[p] as usize..start[p + 1] as usize] {
                let d = &dirs[c as usize];
                let name = std::str::from_utf8(&names[d.name_off as usize..(d.name_off + d.name_len) as usize]).unwrap_or("?");
                tid[c as usize] = tree.add(tid[p], name, d.mtime, d.link);
                queue.push(c);
            }
        }
    }
    // tệp trực tiếp
    for list in files {
        for (parent, n, sz, al) in list {
            let i = idx.get(parent as usize).copied().unwrap_or(NONE);
            if i == NONE || tid[i as usize] == NONE || (i != root_i && dirs[i as usize].link) {
                continue;
            }
            let d = tree.get_mut(tid[i as usize]);
            d.size += sz;
            d.alloc += al;
            d.files = d.files.saturating_add(n);
        }
    }
    // tổng đệ quy: từ lá lên gốc
    for id in (0..tree.len).rev() {
        let d = *tree.get(id);
        let n = tree.get_mut(id);
        n.left = 0;
        if d.flags & F_LINK == 0 {
            n.flags |= F_LISTED;
        }
        if d.parent != NONE {
            let p = tree.get_mut(d.parent);
            p.size += d.size;
            p.alloc += d.alloc;
            p.files = p.files.saturating_add(d.files);
            p.dirs = p.dirs.saturating_add(d.dirs).saturating_add(1);
        }
    }
    tree
}

/// Quét cả ổ `letter` từ MFT. Err("") = không dùng được (không Admin / không phải NTFS), Err(lý do) = lỗi lạ.
pub(super) fn scan(s: &Scan, letter: char, threads: usize) -> Result<Tree, String> {
    let vol = open(letter)?;
    let g = geometry(&vol)?;
    let (runs, size) = mft_runs(&vol, &g)?;
    drop(vol);
    let records = size.min(g.valid) / g.rec as u64;
    if records > u64::from(u32::MAX - 1) {
        return Err("MFT quá lớn".into());
    }
    let pieces = pieces(&runs, &g, records);
    let next = AtomicUsize::new(0);
    let results: Vec<Result<Out, String>> = std::thread::scope(|sc| {
        let hs: Vec<_> = (0..threads)
            .filter_map(|_| std::thread::Builder::new().name("du-mft".into()).spawn_scoped(sc, || worker(s, letter, &g, &pieces, records, &next)).ok())
            .collect();
        hs.into_iter().map(|h| h.join().unwrap_or_else(|_| Err("luồng đọc MFT lỗi".into()))).collect()
    });
    if s.cancel.load(Ordering::Relaxed) {
        return Err(String::new());
    }
    let mut outs = results.into_iter().collect::<Result<Vec<_>, _>>()?;
    if outs.is_empty() {
        return Err("không tạo được luồng đọc MFT".into());
    }
    let merged = merge_parts(&mut outs);
    outs.push(merged);
    Ok(build(&s.root, records as usize, outs))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bản ghi 1 KB giả: header + fixup + các thuộc tính cho trước.
    fn record(flags: u16, base: u64, attrs: &[Vec<u8>]) -> Vec<u8> {
        let mut r = vec![0u8; 1024];
        r[0..4].copy_from_slice(b"FILE");
        r[4..6].copy_from_slice(&0x30u16.to_le_bytes()); // update sequence ở 0x30
        r[6..8].copy_from_slice(&3u16.to_le_bytes()); // 1 + 2 khối 512 byte
        r[0x14..0x16].copy_from_slice(&0x38u16.to_le_bytes());
        r[0x16..0x18].copy_from_slice(&flags.to_le_bytes());
        r[0x20..0x28].copy_from_slice(&base.to_le_bytes());
        let mut p = 0x38;
        for a in attrs {
            r[p..p + a.len()].copy_from_slice(a);
            p += a.len();
        }
        r[p..p + 4].copy_from_slice(&AT_END.to_le_bytes());
        r[0x18..0x1C].copy_from_slice(&((p + 8) as u32).to_le_bytes());
        // fixup: giá trị thật của 2 byte cuối mỗi khối vào mảng, rồi ghi số thứ tự vào đó (như NTFS ghi xuống đĩa)
        let usn = [0x11u8, 0x22];
        r[0x30..0x32].copy_from_slice(&usn);
        for i in 1..3 {
            let (a, b) = (r[i * 512 - 2], r[i * 512 - 1]);
            r[0x30 + 2 * i] = a;
            r[0x31 + 2 * i] = b;
            r[i * 512 - 2..i * 512].copy_from_slice(&usn);
        }
        r
    }

    fn resident(ty: u32, name_len: u8, v: &[u8]) -> Vec<u8> {
        let len = (0x18 + v.len()).next_multiple_of(8);
        let mut a = vec![0u8; len];
        a[0..4].copy_from_slice(&ty.to_le_bytes());
        a[4..8].copy_from_slice(&(len as u32).to_le_bytes());
        a[9] = name_len;
        a[0x10..0x14].copy_from_slice(&(v.len() as u32).to_le_bytes());
        a[0x14..0x16].copy_from_slice(&0x18u16.to_le_bytes());
        a[0x18..0x18 + v.len()].copy_from_slice(v);
        a
    }

    fn file_name(parent: u64, ns: u8, name: &str) -> Vec<u8> {
        let w: Vec<u16> = name.encode_utf16().collect();
        let mut v = vec![0u8; 0x42 + 2 * w.len()];
        v[0..8].copy_from_slice(&(parent | (3 << 48)).to_le_bytes()); // số thứ tự ở 16 bit cao phải bị bỏ
        v[0x40] = w.len() as u8;
        v[0x41] = ns;
        for (i, c) in w.iter().enumerate() {
            v[0x42 + 2 * i..0x44 + 2 * i].copy_from_slice(&c.to_le_bytes());
        }
        resident(AT_FILE_NAME, 0, &v)
    }

    fn nonresident_data(name_len: u8, alloc: u64, size: u64, compressed: Option<u64>) -> Vec<u8> {
        let mut a = vec![0u8; 0x50];
        a[0..4].copy_from_slice(&AT_DATA.to_le_bytes());
        a[4..8].copy_from_slice(&0x50u32.to_le_bytes());
        a[8] = 1;
        a[9] = name_len;
        a[0x20..0x22].copy_from_slice(&(if compressed.is_some() { 0x48u16 } else { 0x40 }).to_le_bytes());
        if compressed.is_some() {
            a[0x0C..0x0E].copy_from_slice(&0x8000u16.to_le_bytes());
        }
        a[0x28..0x30].copy_from_slice(&alloc.to_le_bytes());
        a[0x30..0x38].copy_from_slice(&size.to_le_bytes());
        a[0x40..0x48].copy_from_slice(&compressed.unwrap_or(0).to_le_bytes());
        a
    }

    #[test]
    fn parses_a_file_record_like_ntfs_writes_it() {
        // 2 khối 512 byte có fixup; tên ngắn DOS đứng trước phải bị bỏ qua; luồng phụ cộng vào dung lượng trên đĩa
        let mut si = vec![0u8; 0x30];
        si[8..16].copy_from_slice(&(116_444_736_000_000_000i64 + 10_000_000 * 1_700_000_000).to_le_bytes());
        let mut r = record(
            1,
            0,
            &[
                resident(AT_STANDARD_INFORMATION, 0, &si),
                file_name(40, NS_DOS, "LONGFI~1.TXT"),
                file_name(40, 1, "long file.txt"),
                nonresident_data(0, 8192, 5000, Some(0)),
                nonresident_data(16, 4096, 9000, Some(4096)),
            ],
        );
        assert!(fixup(&mut r).is_some());
        let x = parse(&r).unwrap();
        assert_eq!(x.base, NONE);
        assert!(!x.dir && !x.list);
        assert_eq!(x.si.unwrap().0, 1_700_000_000);
        let (parent, name) = x.name.unwrap();
        assert_eq!(parent, 40);
        let mut s = Vec::new();
        utf16_to(&mut s, name);
        assert_eq!(String::from_utf8(s).unwrap(), "long file.txt");
        assert_eq!(x.size, Some(5000));
        assert_eq!(x.alloc, 4096, "sparse: dùng compressed size; luồng phụ cộng thêm");
    }

    #[test]
    fn rejects_torn_and_empty_records() {
        let mut r = record(1, 0, &[file_name(5, 1, "a")]);
        r[1022] ^= 1; // khối 2 ghi dở
        assert!(fixup(&mut r).is_none());
        let mut free = record(0, 0, &[file_name(5, 1, "a")]);
        assert!(fixup(&mut free).is_some());
        assert!(parse(&free).is_none(), "bản ghi không dùng");
        let mut ext = record(1, 77 | (2 << 48), &[nonresident_data(0, 4096, 100, None)]);
        assert!(fixup(&mut ext).is_some());
        assert_eq!(parse(&ext).unwrap().base, 77);
        assert!(parse(&[0u8; 8]).is_none(), "cắt cụt không được panic");
    }

    #[test]
    fn decodes_mapping_pairs() {
        // 0x21: 1 byte độ dài, 2 byte vị trí; đoạn sau lùi lại (vị trí âm)
        let runs = decode_runs(&[0x21, 0x10, 0x00, 0x01, 0x21, 0x08, 0x00, 0xFF, 0x00]).unwrap();
        assert_eq!(runs, vec![(0x100, 0x10), (0x100 - 0x100, 8)]);
        assert!(decode_runs(&[0x01, 0x05, 0x00]).is_none(), "sparse");
        assert!(decode_runs(&[0x21, 0x10]).is_none(), "cắt cụt");
    }

    #[test]
    fn builds_tree_with_hard_links_once_and_links_not_followed() {
        let mut o = Out::default();
        let si = |link| Some((100, if link { ATTR_REPARSE } else { 0 }));
        o.dir(ROOT, ROOT, si(false), |v| v.extend_from_slice(b"."));
        o.dir(20, ROOT, si(false), |v| v.extend_from_slice(b"Windows"));
        o.dir(21, 20, si(false), |v| v.extend_from_slice(b"System32"));
        o.dir(22, ROOT, si(true), |v| v.extend_from_slice(b"Junction"));
        o.dir(23, 22, si(false), |v| v.extend_from_slice(b"Inside"));
        o.dir(24, 11, si(false), |v| v.extend_from_slice(b"$Extend child"));
        o.files = vec![(21, 3, 300, 400), (20, 1, 10, 0), (ROOT, 2, 5, 8), (23, 9, 999, 999), (24, 1, 1, 1)];
        let t = build(r"C:\", 64, vec![o]);
        assert_eq!(t.len, 4, "gốc, Windows, System32, Junction (không có Inside, $Extend)");
        let root = *t.get(0);
        assert_eq!((root.size, root.alloc, root.files, root.dirs), (315, 408, 6, 3));
        assert_eq!(t.path(t.find(r"C:\", r"C:\Windows\System32").unwrap()), r"C:\Windows\System32");
        let j = t.find(r"C:\", r"C:\Junction").unwrap();
        assert!(t.get(j).flags & F_LINK != 0 && t.get(j).size == 0);
        assert!((0..t.len).all(|i| t.get(i).left == 0));
    }

    /// `cargo test --release --lib du::mft -- --ignored --nocapture` trong cửa sổ Admin: so với cách liệt kê thư mục.
    #[test]
    #[ignore = "đo trên ổ thật, cần Admin"]
    fn mft_matches_listing_on_system_drive() {
        let root = format!("{}\\", std::env::var("SystemDrive").unwrap_or("C:".into()));
        let wait = |s: &Scan| {
            while !s.is_done() {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        };
        let fast = Scan::start(1, &root).unwrap();
        wait(&fast);
        let slow = Scan::start_opts(2, &root, false).unwrap();
        wait(&slow);
        let (pf, ps) = (fast.progress(), slow.progress());
        eprintln!("MFT:       {} ms, {} tệp, {} thư mục, {:.2} GB, cây {:.1} MB", pf.elapsed_ms, pf.files, pf.dirs, pf.bytes as f64 / 1e9, fast.mem_estimate() as f64 / 1048576.0);
        eprintln!("liệt kê:   {} ms, {} tệp, {} thư mục, {:.2} GB, {} lỗi", ps.elapsed_ms, ps.files, ps.dirs, ps.bytes as f64 / 1e9, ps.errors);
        let (a, b) = (fast.children(0, "size", true, 30).unwrap(), slow.children(0, "size", true, 30).unwrap());
        for c in &a.children {
            let o = b.children.iter().find(|x| x.name == c.name);
            eprintln!(
                "  {:<28} MFT {:>9.2} GB {:>9} tệp | liệt kê {:>9} {:>9}",
                c.name,
                c.alloc as f64 / 1e9,
                c.files,
                o.map_or("–".into(), |o| format!("{:.2} GB", o.alloc as f64 / 1e9)),
                o.map_or("–".into(), |o| o.files.to_string())
            );
        }
        // thư mục lệch nhiều nhất (tệp trực tiếp): hard link được tính một lần, System Volume Information đọc được
        fn diff(a: &Scan, b: &Scan, ia: u32, ib: u32, path: &str, out: &mut Vec<(i64, i64, String)>) {
            let (ca, cb) = (a.children(ia, "name", false, usize::MAX).unwrap(), b.children(ib, "name", false, usize::MAX).unwrap());
            let d = (ca.node.own_alloc as i64 - cb.node.own_alloc as i64, i64::from(ca.node.own_files) - i64::from(cb.node.own_files));
            if d != (0, 0) {
                out.push((d.0, d.1, path.to_string()));
            }
            for x in &ca.children {
                if let Some(y) = cb.children.iter().find(|y| y.name == x.name) {
                    diff(a, b, x.id, y.id, &format!(r"{path}\{}", x.name), out);
                }
            }
        }
        let mut d = Vec::new();
        diff(&fast, &slow, 0, 0, root.trim_end_matches('\\'), &mut d);
        d.sort_by_key(|x| -x.0.abs());
        eprintln!("{} thư mục lệch; lệch nhiều nhất (MFT − liệt kê):", d.len());
        for (al, nf, p) in d.iter().take(15) {
            eprintln!("  {:>+9.3} GB {:>+7} tệp  {p}", *al as f64 / 1e9, nf);
        }
        assert!(pf.files > 0 && pf.dirs > 0);
    }
}
