//! Trích xuất biểu tượng Windows và mã hóa PNG 32 × 32.

use std::collections::HashMap;
use std::mem::{size_of, size_of_val};
use std::path::Path;
use std::sync::{LazyLock, Mutex};

use windows::core::PCWSTR;
use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::Graphics::Gdi::{
    DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ,
};
use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, GetIconInfo, PrivateExtractIconsW, HICON, ICONINFO,
};

const SIZE: usize = 32;
const ROW_BYTES: usize = SIZE * 4 + 1;
const RAW_BYTES: usize = ROW_BYTES * SIZE;
const PREFIX: &str = "data:image/png;base64,";

static CACHE: LazyLock<Mutex<HashMap<String, Option<String>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Lấy PNG 32 × 32. Ghi nhớ kết quả theo đường dẫn viết thường, kể cả khi thiếu icon.
pub fn icon_data_url(path: &str) -> Option<String> {
    let key = path.to_lowercase();
    if let Some(cached) = CACHE.lock().ok()?.get(&key).cloned() {
        return cached;
    }
    // Logo app Store là PNG sẵn: gửi nguyên tệp (nhỏ), không qua Shell.
    if key.ends_with(".png") {
        let result = std::fs::read(path).ok().filter(|b| !b.is_empty() && b.len() <= 256 * 1024).map(|png| {
            let mut url = String::with_capacity(PREFIX.len() + png.len().div_ceil(3) * 4);
            url.push_str(PREFIX);
            append_base64(&png, &mut url);
            url
        });
        return CACHE.lock().ok()?.entry(key).or_insert(result).clone();
    }
    // Shell và GDI chạy ngoài mutex để không chặn tra cứu đã có trong cache.
    let result = extract(path).map(|pixels| {
        let png = encode_png(&pixels);
        let mut url = String::with_capacity(PREFIX.len() + png.len().div_ceil(3) * 4);
        url.push_str(PREFIX);
        append_base64(&png, &mut url);
        url
    });
    CACHE.lock().ok()?.entry(key).or_insert(result).clone()
}

struct Com(bool);

impl Com {
    fn initialize() -> Option<Self> {
        // Cần COM trên đúng luồng gọi Shell; giữ apartment hiện tại nếu khác STA.
        let result = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if result.is_ok() || result == RPC_E_CHANGED_MODE {
            Some(Self(result.is_ok()))
        } else {
            None
        }
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            // Cân bằng cả S_OK và S_FALSE trên luồng đã gọi CoInitializeEx.
            unsafe { CoUninitialize() };
        }
    }
}

struct Icon(HICON);

impl Drop for Icon {
    fn drop(&mut self) {
        // SHGetFileInfoW chuyển quyền sở hữu HICON cho bên gọi.
        let _ = unsafe { DestroyIcon(self.0) };
    }
}

struct Bitmap(HBITMAP);

impl Drop for Bitmap {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            // GetIconInfo tạo bitmap mới cho bên gọi.
            let _ = unsafe { DeleteObject(HGDIOBJ(self.0 .0)) };
        }
    }
}

struct ScreenDc(HDC);

impl Drop for ScreenDc {
    fn drop(&mut self) {
        // Giải phóng DC màn hình bằng cùng HWND đã dùng khi lấy DC.
        unsafe { ReleaseDC(None, self.0) };
    }
}

fn exe_icon(path: &[u16]) -> Option<Icon> {
    let mut short_path = [0; 260];
    short_path.get_mut(..path.len())?.copy_from_slice(path);
    let mut icons = [HICON::default(); 1];
    // Đọc trực tiếp icon 32 × 32 của tệp thực thi, tránh khởi động Shell.
    let count = unsafe {
        PrivateExtractIconsW(
            &short_path,
            0,
            SIZE as i32,
            SIZE as i32,
            Some(&mut icons),
            None,
            0,
        )
    };
    let icon = (!icons[0].is_invalid()).then(|| Icon(icons[0]))?;
    (count == 1).then_some(icon)
}

fn shell_icon(path: &[u16]) -> Option<Icon> {
    let _com = Com::initialize()?;
    let mut file = SHFILEINFOW::default();
    // Shell đọc chuỗi kết thúc bằng NUL và ghi vào cấu trúc đúng kích thước.
    let result = unsafe {
        SHGetFileInfoW(
            PCWSTR(path.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut file),
            size_of_val(&file) as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        )
    };
    let icon = (!file.hIcon.is_invalid()).then(|| Icon(file.hIcon))?;
    (result != 0).then_some(icon)
}

fn extract(path: &str) -> Option<[u8; RAW_BYTES]> {
    if path.is_empty() || path.contains('\0') {
        return None;
    }
    let file = Path::new(path);
    file.metadata().ok()?;
    let is_exe = file
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"));
    let path: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let icon = if is_exe { exe_icon(&path) } else { None }.or_else(|| shell_icon(&path))?;

    let mut info = ICONINFO::default();
    // Windows tạo hai bitmap riêng từ handle biểu tượng còn hiệu lực.
    unsafe { GetIconInfo(icon.0, &mut info) }.ok()?;
    let color = Bitmap(info.hbmColor);
    let mask = Bitmap(info.hbmMask);
    // DC màn hình dùng cho chuyển đổi DIB; bitmap không được chọn vào DC.
    let dc = unsafe { GetDC(None) };
    if dc.is_invalid() {
        return None;
    }
    let dc = ScreenDc(dc);
    if color.0.is_invalid() {
        let mask = read_bitmap(&dc, &mask)?;
        if mask.height % 2 != 0 {
            return None;
        }
        return Some(rgba_rows(None, Some(&mask), mask.width, mask.height / 2));
    }
    let color = read_bitmap(&dc, &color)?;
    let has_alpha = color
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .any(|pixel| pixel[3] != 0);
    let mask = if has_alpha {
        None
    } else {
        let bitmap = read_bitmap(&dc, &mask)?;
        if bitmap.width != color.width || bitmap.height != color.height {
            return None;
        }
        Some(bitmap)
    };
    Some(rgba_rows(
        Some(&color),
        mask.as_ref(),
        color.width,
        color.height,
    ))
}

struct Pixels {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
}

fn read_bitmap(dc: &ScreenDc, bitmap: &Bitmap) -> Option<Pixels> {
    if bitmap.0.is_invalid() {
        return None;
    }
    let mut object = BITMAP::default();
    // GetObjectW ghi cấu trúc BITMAP vào vùng nhớ đúng kích thước.
    let size = unsafe {
        GetObjectW(
            HGDIOBJ(bitmap.0 .0),
            size_of_val(&object) as i32,
            Some((&mut object as *mut BITMAP).cast()),
        )
    };
    if size != size_of_val(&object) as i32
        || !(1..=1024).contains(&object.bmWidth)
        || !(1..=2048).contains(&object.bmHeight)
    {
        return None;
    }
    let width = object.bmWidth as usize;
    let height = object.bmHeight as usize;
    let mut pixels = vec![0_u8; width * height * 4];
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: object.bmWidth,
            biHeight: -object.bmHeight,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    // DIB 32 bit có đúng width * 4 byte mỗi dòng, chiều cao âm để đọc từ trên xuống.
    let lines = unsafe {
        GetDIBits(
            dc.0,
            bitmap.0,
            0,
            height as u32,
            Some(pixels.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        )
    };
    (lines == height as i32).then_some(Pixels {
        width,
        height,
        pixels,
    })
}

fn rgba_rows(
    color: Option<&Pixels>,
    mask: Option<&Pixels>,
    width: usize,
    height: usize,
) -> [u8; RAW_BYTES] {
    let mut rows = [0; RAW_BYTES];
    for (y, row) in rows.as_chunks_mut::<ROW_BYTES>().0.iter_mut().enumerate() {
        let source_y = (2 * y + 1) * height / (2 * SIZE);
        for (x, pixel) in row[1..].as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let source_x = (2 * x + 1) * width / (2 * SIZE);
            let offset = (source_y * width + source_x) * 4;
            if let Some(color) = color {
                let bgra = &color.pixels[offset..offset + 4];
                pixel.copy_from_slice(&[bgra[2], bgra[1], bgra[0], bgra[3]]);
            } else if let Some(mask) = mask {
                // Icon đơn sắc lưu mặt nạ XOR ngay sau mặt nạ AND.
                let xor = mask.pixels[offset + width * height * 4];
                pixel[..3].fill(xor);
            }
            if let Some(mask) = mask {
                pixel[3] = if mask.pixels[offset] == 0 { 255 } else { 0 };
            }
        }
    }
    rows
}

const fn crc_table() -> [u32; 256] {
    let mut table = [0; 256];
    let mut i = 0;
    while i < table.len() {
        let mut crc = i as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
            bit += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}

const CRC_TABLE: [u32; 256] = crc_table();

fn png_chunk(png: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    png.extend_from_slice(&(data.len() as u32).to_be_bytes());
    png.extend_from_slice(kind);
    png.extend_from_slice(data);
    let mut crc = u32::MAX;
    for &byte in kind.iter().chain(data) {
        crc = (crc >> 8) ^ CRC_TABLE[((crc ^ u32::from(byte)) & 0xff) as usize];
    }
    png.extend_from_slice(&(!crc).to_be_bytes());
}

fn encode_png(rows: &[u8; RAW_BYTES]) -> Vec<u8> {
    let mut data = Vec::with_capacity(RAW_BYTES + 11);
    let len = RAW_BYTES as u16;
    // Đầu zlib 0x7801 và một khối DEFLATE cuối không nén (BFINAL=1, BTYPE=0).
    data.extend_from_slice(&[0x78, 0x01, 0x01]);
    data.extend_from_slice(&len.to_le_bytes());
    data.extend_from_slice(&(!len).to_le_bytes());
    data.extend_from_slice(rows);
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in rows {
        a = (a + u32::from(byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    data.extend_from_slice(&((b << 16) | a).to_be_bytes());

    let mut png = Vec::with_capacity(data.len() + 57);
    png.extend_from_slice(b"\x89PNG\r\n\x1A\n");
    let mut header = [0u8; 13];
    header[..4].copy_from_slice(&(SIZE as u32).to_be_bytes());
    header[4..8].copy_from_slice(&(SIZE as u32).to_be_bytes());
    header[8] = 8;
    header[9] = 6;
    png_chunk(&mut png, b"IHDR", &header);
    png_chunk(&mut png, b"IDAT", &data);
    png_chunk(&mut png, b"IEND", &[]);
    png
}

fn append_base64(bytes: &[u8], out: &mut String) {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    for chunk in bytes.chunks(3) {
        let a = usize::from(chunk[0]);
        let b = usize::from(*chunk.get(1).unwrap_or(&0));
        let c = usize::from(*chunk.get(2).unwrap_or(&0));
        out.push(char::from(TABLE[a >> 2]));
        out.push(char::from(TABLE[((a & 3) << 4) | (b >> 4)]));
        out.push(if chunk.len() >= 2 {
            char::from(TABLE[((b & 15) << 2) | (c >> 6)])
        } else {
            '='
        });
        out.push(if chunk.len() == 3 {
            char::from(TABLE[c & 63])
        } else {
            '='
        });
    }
}
