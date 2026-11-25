// Cổng đang dùng: chương trình nào nghe (TCP) / gắn (UDP) cổng nào, ai truy cập được, bao nhiêu kết nối.
// Đọc thẳng bảng của IP Helper (GetExtendedTcpTable / GetExtendedUdpTable) – vài ms, không qua netstat.
// Danh mục cổng quen thuộc (Vite 5173, PostgreSQL 5432…) nằm ở frontend (src/lib/ports.js).
use std::collections::BTreeMap;
use crate::tr;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde::Serialize;
use windows::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, NO_ERROR};
use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, GetExtendedUdpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCP6TABLE_OWNER_PID, MIB_TCPROW_OWNER_PID,
    MIB_TCPTABLE_OWNER_PID, MIB_UDP6ROW_OWNER_PID, MIB_UDP6TABLE_OWNER_PID, MIB_UDPROW_OWNER_PID,
    MIB_UDPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_CONNECTIONS, TCP_TABLE_OWNER_PID_LISTENER, UDP_TABLE_OWNER_PID,
};

const AF_INET: u32 = 2;
const AF_INET6: u32 = 23;
const TCP_ESTABLISHED: u32 = 5;
/// Bảng lớn nhất chấp nhận (máy rất nhiều kết nối vẫn chỉ vài MB).
const MAX_TABLE: usize = 16 << 20;

/// Một chương trình nghe (TCP) hoặc gắn (UDP) một cổng cục bộ.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct PortUse {
    /// "tcp" | "udp"
    pub proto: &'static str,
    pub port: u16,
    /// Địa chỉ cục bộ (IPv4 trước, IPv6 sau), không trùng.
    pub addrs: Vec<String>,
    pub pid: u32,
    /// Kết nối TCP đang mở tới cổng này của tiến trình này.
    pub conns: u32,
    /// Ai truy cập được: "local" (chỉ máy này) | "network" (một mạng) | "all" (mọi mạng, 0.0.0.0 / ::)
    pub reach: &'static str,
    /// Điền ở commands.rs từ lần đo gần nhất của monitor (rỗng / 0 nếu chưa có).
    pub name: String,
    pub exe: String,
    pub cpu: f32,
    pub mem: u64,
}

#[derive(Serialize)]
pub struct PortsOut {
    pub uses: Vec<PortUse>,
    pub took_ms: f64,
    /// Còn PID chưa có tên (monitor chưa đo tới): giao diện nên hỏi lại sớm.
    pub pending: bool,
}

/// Kích thước bảng lần trước của từng loại: gọi một lần là đủ thay vì hai.
static HINTS: [AtomicUsize; 6] = [const { AtomicUsize::new(16 * 1024) }; 6];

/// Gọi hàm GetExtended*Table tới khi bộ đệm đủ lớn. Trả về bộ đệm căn lề 8 byte.
fn fetch(hint: &AtomicUsize, call: impl Fn(*mut core::ffi::c_void, &mut u32) -> u32) -> Result<Vec<u64>, String> {
    let mut buf: Vec<u64> = vec![0; hint.load(Ordering::Relaxed).div_ceil(8).max(64)];
    for _ in 0..4 {
        let mut size = (buf.len() * 8) as u32;
        let rc = call(buf.as_mut_ptr().cast(), &mut size);
        if rc == NO_ERROR.0 {
            hint.store(buf.len() * 8, Ordering::Relaxed);
            return Ok(buf);
        }
        if rc != ERROR_INSUFFICIENT_BUFFER.0 {
            return Err(tr!("ports.err.read_table", rc = rc));
        }
        // bảng có thể lớn thêm giữa hai lần gọi -> chừa thêm chỗ
        let want = (size as usize).saturating_add(4096);
        if want > MAX_TABLE {
            return Err(tr!("ports.err.table_too_big"));
        }
        buf = vec![0; want.div_ceil(8)];
    }
    Err(tr!("ports.err.table_changing"))
}

/// # Safety
/// `buf` phải chứa một bảng: `u32` số dòng, rồi các dòng `R` từ `offset`.
unsafe fn rows<R: Copy>(buf: &[u64], offset: usize) -> Vec<R> {
    let base = buf.as_ptr().cast::<u8>();
    let count = *base.cast::<u32>() as usize;
    // không đọc quá bộ đệm dù số dòng có sai
    let fit = (buf.len() * 8).saturating_sub(offset) / std::mem::size_of::<R>().max(1);
    let first = base.add(offset).cast::<R>();
    (0..count.min(fit)).map(|i| std::ptr::read_unaligned(first.add(i))).collect()
}

fn port_of(raw: u32) -> u16 {
    u16::from_be(raw as u16)
}
fn v4(raw: u32) -> IpAddr {
    IpAddr::V4(Ipv4Addr::from(raw.to_ne_bytes()))
}
fn v6(raw: [u8; 16]) -> IpAddr {
    IpAddr::V6(Ipv6Addr::from(raw))
}

fn reach_of(a: &IpAddr) -> u8 {
    if a.is_unspecified() {
        2
    } else if a.is_loopback() {
        0
    } else {
        1
    }
}
const REACH: [&str; 3] = ["local", "network", "all"];

struct Acc {
    addrs: Vec<IpAddr>,
    conns: u32,
}

/// Mọi cổng TCP đang nghe và (nếu `udp`) cổng UDP đang gắn; mỗi (giao thức, cổng, PID) một dòng, sắp theo cổng.
pub fn list(udp: bool) -> Result<Vec<PortUse>, String> {
    // khoá: (cổng, 0 = tcp / 1 = udp, pid)
    let mut map: BTreeMap<(u16, u8, u32), Acc> = BTreeMap::new();
    let mut add = |port: u16, proto: u8, pid: u32, ip: IpAddr| {
        let e = map.entry((port, proto, pid)).or_insert_with(|| Acc { addrs: Vec::new(), conns: 0 });
        if !e.addrs.contains(&ip) {
            e.addrs.push(ip);
        }
    };

    let t4 = fetch(&HINTS[0], |p, s| unsafe { GetExtendedTcpTable(Some(p), s, false, AF_INET, TCP_TABLE_OWNER_PID_LISTENER, 0) })?;
    for r in unsafe { rows::<MIB_TCPROW_OWNER_PID>(&t4, std::mem::offset_of!(MIB_TCPTABLE_OWNER_PID, table)) } {
        add(port_of(r.dwLocalPort), 0, r.dwOwningPid, v4(r.dwLocalAddr));
    }
    let t6 = fetch(&HINTS[1], |p, s| unsafe { GetExtendedTcpTable(Some(p), s, false, AF_INET6, TCP_TABLE_OWNER_PID_LISTENER, 0) })?;
    for r in unsafe { rows::<MIB_TCP6ROW_OWNER_PID>(&t6, std::mem::offset_of!(MIB_TCP6TABLE_OWNER_PID, table)) } {
        add(port_of(r.dwLocalPort), 0, r.dwOwningPid, v6(r.ucLocalAddr));
    }
    if udp {
        let u4 = fetch(&HINTS[2], |p, s| unsafe { GetExtendedUdpTable(Some(p), s, false, AF_INET, UDP_TABLE_OWNER_PID, 0) })?;
        for r in unsafe { rows::<MIB_UDPROW_OWNER_PID>(&u4, std::mem::offset_of!(MIB_UDPTABLE_OWNER_PID, table)) } {
            add(port_of(r.dwLocalPort), 1, r.dwOwningPid, v4(r.dwLocalAddr));
        }
        let u6 = fetch(&HINTS[3], |p, s| unsafe { GetExtendedUdpTable(Some(p), s, false, AF_INET6, UDP_TABLE_OWNER_PID, 0) })?;
        for r in unsafe { rows::<MIB_UDP6ROW_OWNER_PID>(&u6, std::mem::offset_of!(MIB_UDP6TABLE_OWNER_PID, table)) } {
            add(port_of(r.dwLocalPort), 1, r.dwOwningPid, v6(r.ucLocalAddr));
        }
    }

    // kết nối đang mở tới các cổng đang nghe (lỗi ở đây chỉ làm mất số kết nối, không làm hỏng danh sách)
    let mut est = |port: u16, pid: u32| {
        if let Some(a) = map.get_mut(&(port, 0, pid)) {
            a.conns += 1;
        }
    };
    if let Ok(c4) = fetch(&HINTS[4], |p, s| unsafe { GetExtendedTcpTable(Some(p), s, false, AF_INET, TCP_TABLE_OWNER_PID_CONNECTIONS, 0) }) {
        for r in unsafe { rows::<MIB_TCPROW_OWNER_PID>(&c4, std::mem::offset_of!(MIB_TCPTABLE_OWNER_PID, table)) } {
            if r.dwState == TCP_ESTABLISHED {
                est(port_of(r.dwLocalPort), r.dwOwningPid);
            }
        }
    }
    if let Ok(c6) = fetch(&HINTS[5], |p, s| unsafe { GetExtendedTcpTable(Some(p), s, false, AF_INET6, TCP_TABLE_OWNER_PID_CONNECTIONS, 0) }) {
        for r in unsafe { rows::<MIB_TCP6ROW_OWNER_PID>(&c6, std::mem::offset_of!(MIB_TCP6TABLE_OWNER_PID, table)) } {
            if r.dwState == TCP_ESTABLISHED {
                est(port_of(r.dwLocalPort), r.dwOwningPid);
            }
        }
    }

    Ok(map
        .into_iter()
        .map(|((port, proto, pid), a)| {
            let reach = a.addrs.iter().map(reach_of).max().unwrap_or(0);
            let mut addrs = a.addrs;
            addrs.sort_by_key(|ip| ip.is_ipv6()); // IPv4 trước
            PortUse {
                proto: if proto == 0 { "tcp" } else { "udp" },
                port,
                addrs: addrs.iter().map(IpAddr::to_string).collect(),
                pid,
                conns: a.conns,
                reach: REACH[reach as usize],
                name: String::new(),
                exe: String::new(),
                cpu: 0.0,
                mem: 0,
            }
        })
        .collect())
}

/// Giới hạn bản tóm tắt cổng gửi cho trợ lý AI (~1 000 token).
pub const SUMMARY_MAX: usize = 3500;

/// Tóm tắt cổng cho trợ lý AI (khối `<ports>`): mỗi cổng một dòng gọn, cổng mở cho mọi mạng lên đầu.
/// `uses` đã có tên tiến trình (commands.rs điền). Cắt ở SUMMARY_MAX ký tự.
pub fn ai_summary(uses: &[PortUse]) -> String {
    use std::fmt::Write;
    let tcp = uses.iter().filter(|u| u.proto == "tcp").count();
    let open = uses.iter().filter(|u| u.proto == "tcp" && u.reach == "all").count();
    let mut s = format!(
        "{tcp} cổng TCP đang nghe, {open} mở cho mọi mạng, {} cổng UDP. Dòng: cổng giao_thức truy_cập(local=chỉ máy này, network=một mạng, all=mọi mạng) chương_trình(PID) kết_nối RAM\n",
        uses.len() - tcp
    );
    let rank = |u: &PortUse| (u.proto != "tcp", match u.reach { "all" => 0, "network" => 1, _ => 2 }, u.port);
    let mut v: Vec<&PortUse> = uses.iter().collect();
    v.sort_by_key(|u| rank(u));
    let mut shown = 0;
    for u in &v {
        let name = if u.name.is_empty() { "?" } else { u.name.as_str() };
        let mut line = format!("{} {} {} {}({})", u.port, u.proto, u.reach, name, u.pid);
        if u.proto == "tcp" {
            let _ = write!(line, " {}kn", u.conns);
        }
        if u.mem > 0 {
            let _ = write!(line, " {}MB", u.mem / 1_048_576);
        }
        if s.len() + line.len() + 40 > SUMMARY_MAX {
            break;
        }
        s.push_str(&line);
        s.push('\n');
        shown += 1;
    }
    if shown < v.len() {
        let _ = write!(s, "… và {} dòng nữa (đã lược bớt)", v.len() - shown);
    }
    s
}

/// Mở http://localhost:<port> bằng trình duyệt mặc định. URL dựng từ số cổng, không nhận chuỗi tuỳ ý.
pub fn open_local(port: u16) -> Result<(), String> {
    if port == 0 {
        return Err(tr!("ports.err.bad_port"));
    }
    super::shell::open_target(&format!("http://localhost:{port}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reach_ranks_all_networks_highest() {
        assert_eq!(reach_of(&"127.0.0.1".parse().unwrap()), 0);
        assert_eq!(reach_of(&"::1".parse().unwrap()), 0);
        assert_eq!(reach_of(&"192.168.1.5".parse().unwrap()), 1);
        assert_eq!(reach_of(&"0.0.0.0".parse().unwrap()), 2);
        assert_eq!(reach_of(&"::".parse().unwrap()), 2);
        assert_eq!(port_of(u32::from(8080u16.to_be())), 8080);
    }

    #[test]
    fn own_listener_shows_up_with_its_pid_and_connection() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let _c = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let (_s, _) = l.accept().unwrap();
        let t = std::time::Instant::now();
        let uses = list(true).unwrap();
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        let me = uses.iter().find(|u| u.port == port && u.proto == "tcp").expect("không thấy cổng của chính mình");
        assert_eq!(me.pid, std::process::id());
        assert_eq!(me.reach, "local");
        assert_eq!(me.addrs, ["127.0.0.1"]);
        assert_eq!(me.conns, 1);
        println!("{} cổng trong {ms:.2} ms", uses.len());
        let sum = ai_summary(&uses);
        assert!(sum.len() <= SUMMARY_MAX);
        assert!(sum.contains(&format!("{port} tcp local")));
        println!("tóm tắt AI: {} ký tự", sum.len());
    }
}
