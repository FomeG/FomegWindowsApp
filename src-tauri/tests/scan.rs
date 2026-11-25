// cargo test --release --test scan -- --nocapture
use fomeg_windows_app_lib::core::{mem, scan, util};

#[test]
fn exe_path_parsing() {
    assert_eq!(util::exe_path(r#""C:\Program Files\A b\x.exe" --min"#), r"C:\Program Files\A b\x.exe");
    assert_eq!(util::exe_path(r"C:\Tools\app.exe -silent"), r"C:\Tools\app.exe");
    assert!(util::exe_path(r"%windir%\system32\svchost.exe -k netsvcs").to_lowercase().ends_with(r"\system32\svchost.exe"));
    assert!(util::exe_path(r"\SystemRoot\System32\smss.exe").to_lowercase().ends_with(r"\system32\smss.exe"));
    assert!(util::exe_path("rundll32.exe foo.dll,Bar").to_lowercase().ends_with(r"\system32\rundll32.exe"));
}

#[test]
fn scan_runs_fast() {
    for round in 1..=2 {
        let r = scan::run();
        println!("--- lần {round}: {} ms, {} mục khởi động, {} dịch vụ", r.took_ms, r.startup.len(), r.services.len());
        for (k, ms) in &r.timings {
            println!("    {k}: {ms} ms");
        }
        for w in &r.warnings {
            println!("    CẢNH BÁO: {w}");
        }
        let mut kinds = std::collections::BTreeMap::new();
        for it in &r.startup {
            *kinds.entry(it.kind.clone()).or_insert(0) += 1;
        }
        println!("    theo loại: {kinds:?}");
        assert!(!r.services.is_empty());
        let ids: std::collections::HashSet<_> = r.startup.iter().map(|i| &i.id).collect();
        assert_eq!(ids.len(), r.startup.len(), "id phải duy nhất");
    }
    let r = scan::run();
    for it in r.startup.iter().filter(|i| !i.is_ms).take(12) {
        println!("  [{}] {} | {} | on={} | {} | {}", it.kind, it.name, it.publisher, it.on, it.file_path, it.source);
    }
}

#[test]
fn memory_sampler() {
    let mut s = mem::Sampler::default();
    let t = std::time::Instant::now();
    let a = s.sample();
    let first = t.elapsed();
    let t = std::time::Instant::now();
    let b = s.sample();
    let second = t.elapsed();
    let with_mem = b.iter().filter(|p| p.bytes > 0).count();
    let svchost: u64 = b.iter().filter(|p| p.name.eq_ignore_ascii_case("svchost.exe")).map(|p| p.bytes).sum();
    println!("có RAM: {with_mem}/{}, tổng svchost: {} MB", b.len(), svchost >> 20);
    assert!(svchost > 0, "phải đọc được RAM của svchost (tiến trình SYSTEM)");
    let (used, total) = mem::system_memory();
    let with_exe = b.iter().filter(|p| !p.exe.is_empty()).count();
    println!("tiến trình: {} (có đường dẫn: {}), lần đầu {:?}, lần sau {:?}, RAM {}/{} MB", a.len(), with_exe, first, second, used >> 20, total >> 20);
    assert!(total > 0 && !b.is_empty());
}
