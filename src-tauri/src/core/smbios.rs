// Đọc bảng SMBIOS của firmware (GetSystemFirmwareTable 'RSMB') – không cần WMI / PowerShell, ~1 ms.
// Lấy: hãng / model máy (type 1), BIOS (type 0), từng thanh RAM (type 17), số khe RAM, bo mạch chủ (type 2).
// Tham chiếu: DMTF SMBIOS Reference Specification 3.x.

use serde::Serialize;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetSystemFirmwareTable(provider: u32, id: u32, buf: *mut u8, size: u32) -> u32;
}

const RSMB: u32 = u32::from_be_bytes(*b"RSMB");

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct MemModule {
    /// Tên khe do firmware đặt, vd. "DIMM 1", "ChannelA-DIMM0".
    pub slot: String,
    pub maker: String,
    pub part: String,
    /// byte
    pub size: u64,
    /// "DDR4", "LPDDR5"… (rỗng nếu không rõ)
    pub kind: String,
    /// "DIMM", "SODIMM"… (rỗng nếu không rõ)
    pub form: String,
    /// tốc độ đang chạy (MT/s), 0 nếu không rõ
    pub speed: u32,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct Smbios {
    pub mem_modules: Vec<MemModule>,
    /// tổng số khe RAM (kể cả khe trống); 0 nếu firmware không khai báo
    pub mem_slots: u32,
    /// "HP 8A4E", "ASUSTeK COMPUTER INC. PRIME B660M-A"…
    pub board: String,
    /// hãng + model máy, vd. "ASUSTeK COMPUTER INC. ASUS TUF Gaming F15" (type 1)
    pub system: String,
    /// "American Megatrends · FX506HF.316 · 03/12/2024" (type 0)
    pub bios: String,
}

/// Đọc SMBIOS của máy này; lỗi -> giá trị rỗng.
pub fn read() -> Smbios {
    unsafe {
        let need = GetSystemFirmwareTable(RSMB, 0, std::ptr::null_mut(), 0);
        if need == 0 || need > 4 * 1024 * 1024 {
            return Smbios::default();
        }
        let mut buf = vec![0u8; need as usize];
        let got = GetSystemFirmwareTable(RSMB, 0, buf.as_mut_ptr(), need);
        if got == 0 || got > need {
            return Smbios::default();
        }
        buf.truncate(got as usize);
        parse_raw(&buf)
    }
}

/// `RawSMBIOSData`: 4 byte phiên bản + u32 độ dài + bảng.
pub fn parse_raw(raw: &[u8]) -> Smbios {
    if raw.len() < 8 {
        return Smbios::default();
    }
    let len = u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]) as usize;
    let table = &raw[8..raw.len().min(8 + len)];
    parse_table(table)
}

struct Entry<'a> {
    kind: u8,
    f: &'a [u8],          // vùng định dạng (gồm header)
    strings: Vec<&'a [u8]>, // chuỗi đánh số từ 1
}

impl Entry<'_> {
    fn u8(&self, off: usize) -> Option<u8> {
        self.f.get(off).copied()
    }
    fn u16(&self, off: usize) -> Option<u16> {
        Some(u16::from_le_bytes([*self.f.get(off)?, *self.f.get(off + 1)?]))
    }
    fn u32(&self, off: usize) -> Option<u32> {
        Some(u32::from_le_bytes([*self.f.get(off)?, *self.f.get(off + 1)?, *self.f.get(off + 2)?, *self.f.get(off + 3)?]))
    }
    fn s(&self, off: usize) -> String {
        let i = self.u8(off).unwrap_or(0) as usize;
        if i == 0 || i > self.strings.len() {
            return String::new();
        }
        clean(&String::from_utf8_lossy(self.strings[i - 1]))
    }
}

/// Bỏ khoảng trắng và các giá trị "giữ chỗ" vô nghĩa mà firmware hay để.
fn clean(s: &str) -> String {
    let t = s.trim().trim_matches('\0').trim();
    let l = t.to_ascii_lowercase();
    const JUNK: [&str; 10] = [
        "unknown", "not specified", "to be filled by o.e.m.", "default string", "none", "n/a",
        "undefined", "manufacturer", "partnum", "module partnumber",
    ];
    if t.is_empty() || JUNK.iter().any(|j| l == *j) || t.chars().all(|c| c == '0' || c == 'F' || c == ' ') {
        return String::new();
    }
    t.to_owned()
}

fn entries(table: &[u8]) -> Vec<Entry<'_>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 4 <= table.len() {
        let kind = table[i];
        let flen = table[i + 1] as usize;
        if flen < 4 || i + flen > table.len() {
            break;
        }
        let f = &table[i..i + flen];
        // vùng chuỗi: các chuỗi kết thúc bằng 0, cả vùng kết thúc bằng 00 00
        let mut j = i + flen;
        let mut strings = Vec::new();
        if j + 1 < table.len() && table[j] == 0 && table[j + 1] == 0 {
            j += 2;
        } else {
            loop {
                let s0 = j;
                while j < table.len() && table[j] != 0 {
                    j += 1;
                }
                if j >= table.len() {
                    break;
                }
                strings.push(&table[s0..j]);
                j += 1;
                if j >= table.len() || table[j] == 0 {
                    j += 1;
                    break;
                }
            }
        }
        out.push(Entry { kind, f, strings });
        if kind == 127 {
            break; // End-of-table
        }
        i = j;
    }
    out
}

fn mem_kind(code: u8) -> &'static str {
    match code {
        0x0F => "SDRAM",
        0x12 => "DDR",
        0x13 => "DDR2",
        0x14 => "DDR2 FB-DIMM",
        0x18 => "DDR3",
        0x1A => "DDR4",
        0x1B => "LPDDR",
        0x1C => "LPDDR2",
        0x1D => "LPDDR3",
        0x1E => "LPDDR4",
        0x20 => "HBM",
        0x21 => "HBM2",
        0x22 => "DDR5",
        0x23 => "LPDDR5",
        0x24 => "HBM3",
        _ => "",
    }
}

fn form_factor(code: u8) -> &'static str {
    match code {
        0x03 => "SIMM",
        0x05 | 0x0B => "On-board",
        0x09 => "DIMM",
        0x0C => "RIMM",
        0x0D => "SODIMM",
        0x0F => "FB-DIMM",
        0x10 => "Die",
        0x11 => "CAMM",
        _ => "",
    }
}

/// Firmware nhiều máy ghi mã JEDEC (vd. "802C", "80CE") thay cho tên hãng.
fn maker_name(raw: &str) -> String {
    let k = raw.trim().to_ascii_uppercase();
    let k = k.trim_start_matches("0X");
    let name = match k {
        "802C" | "2C00" | "2C80" | "002C" => "Micron Technology",
        "80CE" | "CE00" | "CE80" | "00CE" => "Samsung",
        "80AD" | "AD00" | "AD80" | "00AD" | "HYNIX" => "SK hynix",
        "859B" | "9B05" | "059B" => "Crucial",
        "0198" | "9801" | "8001" => "Kingston",
        "04CB" | "CB04" => "ADATA",
        "04CD" | "CD04" => "G.Skill",
        "029E" | "9E02" => "Corsair",
        "8551" | "5185" => "Qimonda",
        "80C1" | "C180" => "Infineon",
        "8A76" | "768A" => "Ramaxel",
        "0443" | "4304" => "Ramaxel",
        "8C26" | "268C" => "Netac",
        "8632" | "3286" => "CXMT",
        _ => return raw.trim().to_owned(),
    };
    name.to_owned()
}

pub fn parse_table(table: &[u8]) -> Smbios {
    let mut out = Smbios::default();
    let mut board_maker = String::new();
    let mut board_product = String::new();
    for e in entries(table) {
        match e.kind {
            0 if out.bios.is_empty() => {
                out.bios = [e.s(0x04), e.s(0x05), e.s(0x08)].into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" · ");
            }
            1 if out.system.is_empty() => {
                out.system = join_maker(e.s(0x04), e.s(0x05));
            }
            2 if board_product.is_empty() => {
                board_maker = e.s(0x04);
                board_product = e.s(0x05);
            }
            17 => {
                out.mem_slots += 1;
                let Some(raw) = e.u16(0x0C) else { continue };
                let size: u64 = match raw {
                    0 | 0xFFFF => continue, // khe trống / không rõ
                    0x7FFF => e.u32(0x1C).map_or(0, |mb| u64::from(mb & 0x7FFF_FFFF) << 20),
                    v if v & 0x8000 != 0 => u64::from(v & 0x7FFF) << 10, // đơn vị KB
                    v => u64::from(v) << 20,                            // đơn vị MB
                };
                if size == 0 {
                    continue;
                }
                let speed = |off: usize, ext: usize| match e.u16(off) {
                    Some(0xFFFF) => e.u32(ext).unwrap_or(0),
                    Some(v) => u32::from(v),
                    None => 0,
                };
                // tốc độ đang chạy (configured) nếu có, không thì tốc độ tối đa của thanh
                let configured = speed(0x20, 0x58);
                let max = speed(0x15, 0x54);
                let mut slot = e.s(0x10);
                if slot.is_empty() {
                    slot = e.s(0x11);
                }
                out.mem_modules.push(MemModule {
                    slot,
                    maker: maker_name(&e.s(0x17)),
                    part: e.s(0x1A),
                    size,
                    kind: mem_kind(e.u8(0x12).unwrap_or(0)).to_owned(),
                    form: form_factor(e.u8(0x0E).unwrap_or(0)).to_owned(),
                    speed: if configured > 0 { configured } else { max },
                });
            }
            _ => {}
        }
    }
    out.board = join_maker(board_maker, board_product);
    out
}

/// "HP" + "8A4E" -> "HP 8A4E"; tránh lặp "ASUSTeK… ASUSTeK…".
fn join_maker(maker: String, product: String) -> String {
    match (maker.is_empty(), product.is_empty()) {
        (_, true) => maker,
        (true, false) => product,
        (false, false) if product.to_ascii_lowercase().starts_with(&maker.to_ascii_lowercase()) => product,
        (false, false) => format!("{maker} {product}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dựng một cấu trúc SMBIOS: vùng định dạng `f` (đã gồm header 4 byte) + chuỗi.
    fn st(kind: u8, mut f: Vec<u8>, strings: &[&str]) -> Vec<u8> {
        f[0] = kind;
        f[1] = f.len() as u8;
        let mut v = f;
        if strings.is_empty() {
            v.extend_from_slice(&[0, 0]);
        } else {
            for s in strings {
                v.extend_from_slice(s.as_bytes());
                v.push(0);
            }
            v.push(0);
        }
        v
    }

    fn mem(size_field: u16, kind: u8, form: u8, speed: u16, conf: u16, strings: &[&str]) -> Vec<u8> {
        let mut f = vec![0u8; 0x28];
        f[0x0C..0x0E].copy_from_slice(&size_field.to_le_bytes());
        f[0x0E] = form;
        f[0x10] = 1; // locator
        f[0x12] = kind;
        f[0x15..0x17].copy_from_slice(&speed.to_le_bytes());
        f[0x17] = 2; // maker
        f[0x1A] = 3; // part
        f[0x20..0x22].copy_from_slice(&conf.to_le_bytes());
        st(17, f, strings)
    }

    #[test]
    fn parses_modules_empty_slots_and_board() {
        let mut t = Vec::new();
        let mut b = vec![0u8; 8];
        b[4] = 1;
        b[5] = 2;
        t.extend(st(2, b, &["HP", "8A4E"]));
        t.extend(mem(8192, 0x1A, 0x0D, 3200, 3200, &["DIMM 1", "802C", "4ATF1G64HZ-3G2F1   "]));
        t.extend(mem(0, 0x02, 0x02, 0, 0, &["DIMM 2", "Unknown", "Unknown"]));
        t.extend(mem(0x7FFF, 0x22, 0x09, 0xFFFF, 0, &["A1", "Samsung", "M425"]));
        // kích thước mở rộng 32 GB + tốc độ mở rộng 8000
        let last = t.len() - (b"A1\0Samsung\0M425\0\0".len());
        let base = last - 0x28;
        t[base + 0x1C..base + 0x20].copy_from_slice(&32768u32.to_le_bytes());
        // tốc độ tối đa = 0xFFFF -> đọc ở 0x54 (ngoài vùng 0x28 nên = 0)
        t.extend(st(127, vec![0u8; 4], &[]));

        let s = parse_table(&t);
        assert_eq!(s.board, "HP 8A4E");
        assert_eq!(s.mem_slots, 3);
        assert_eq!(s.mem_modules.len(), 2);
        let m = &s.mem_modules[0];
        assert_eq!((m.slot.as_str(), m.maker.as_str(), m.part.as_str()), ("DIMM 1", "Micron Technology", "4ATF1G64HZ-3G2F1"));
        assert_eq!((m.size, m.kind.as_str(), m.form.as_str(), m.speed), (8 << 30, "DDR4", "SODIMM", 3200));
        let m = &s.mem_modules[1];
        assert_eq!((m.size, m.kind.as_str(), m.form.as_str(), m.speed), (32 << 30, "DDR5", "DIMM", 0));
    }

    #[test]
    fn junk_input_is_safe() {
        assert_eq!(parse_raw(&[]), Smbios::default());
        assert_eq!(parse_table(&[17, 2, 0, 0]), Smbios::default());
        let _ = parse_table(&[17, 0x28, 0, 0, 1, 2, 3]);
    }

    #[test]
    fn reads_this_machine() {
        let t = std::time::Instant::now();
        let s = read();
        eprintln!("SMBIOS ({:?}): {s:#?}", t.elapsed());
    }
}
