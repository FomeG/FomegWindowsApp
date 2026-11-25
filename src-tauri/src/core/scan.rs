// Quét song song mọi nguồn, sau đó đọc nhà phát hành (có cache) song song.
use std::collections::{HashMap, HashSet};
use std::time::Instant;
use crate::tr;

use super::services::{self, SvcInfo};
use super::util::{cached_publisher, exe_path, is_ms_publisher, publisher};
use super::{folders, is_critical, registry, tasks, Item, ScanResult};

fn timed<T>(f: impl FnOnce() -> T) -> (T, u64) {
    let t = Instant::now();
    let r = f();
    (r, t.elapsed().as_millis() as u64)
}

pub fn run() -> ScanResult {
    let t0 = Instant::now();
    let mut res = ScanResult::default();

    let (reg, fold, task, svc) = std::thread::scope(|s| {
        let reg = s.spawn(|| timed(|| {
            let mut v = Vec::new();
            registry::scan(&mut v);
            v
        }));
        let fold = s.spawn(|| timed(|| {
            let mut v = Vec::new();
            folders::scan(&mut v);
            v
        }));
        let task = s.spawn(|| timed(|| {
            let mut v = Vec::new();
            let r = tasks::scan(&mut v);
            (v, r)
        }));
        let svc = s.spawn(|| timed(services::enumerate));
        (reg.join(), fold.join(), task.join(), svc.join())
    });

    let mut startup: Vec<Item> = Vec::with_capacity(512);
    match reg {
        Ok((v, ms)) => {
            startup.extend(v);
            res.timings.push(("Registry + MSIX".into(), ms));
        }
        Err(_) => res.warnings.push(tr!("startup.scan.err_registry")),
    }
    match fold {
        Ok((v, ms)) => {
            startup.extend(v);
            res.timings.push((tr!("startup.scan.t_folders"), ms));
        }
        Err(_) => res.warnings.push(tr!("startup.scan.err_folders")),
    }
    match task {
        Ok(((v, r), ms)) => {
            startup.extend(v);
            if let Err(e) = r {
                res.warnings.push(format!("Scheduled Task: {e}"));
            }
            res.timings.push(("Scheduled Task".into(), ms));
        }
        Err(_) => res.warnings.push(tr!("startup.scan.err_tasks")),
    }
    let svcs: Vec<SvcInfo> = match svc {
        Ok((Ok(v), ms)) => {
            res.timings.push((tr!("startup.scan.t_services"), ms));
            v
        }
        Ok((Err(e), _)) => {
            res.warnings.push(tr!("startup.scan.warn_services", e = e));
            Vec::new()
        }
        Err(_) => {
            res.warnings.push(tr!("startup.scan.err_services"));
            Vec::new()
        }
    };

    let mut services: Vec<Item> = svcs.into_iter().map(svc_item).collect();

    // Nhà phát hành: gom đường dẫn chưa có trong cache rồi đọc song song.
    let (_, pub_ms) = timed(|| {
        let mut todo: Vec<String> = startup
            .iter()
            .filter(|i| i.kind != "Packaged")
            .chain(services.iter())
            .map(|i| i.file_path.clone())
            .filter(|p| !p.is_empty() && cached_publisher(p).is_none())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        todo.sort();
        if !todo.is_empty() {
            let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(2, 8);
            let chunk = todo.len().div_ceil(threads).max(1);
            std::thread::scope(|s| {
                for part in todo.chunks(chunk) {
                    s.spawn(move || {
                        for p in part {
                            publisher(p);
                        }
                    });
                }
            });
        }
    });
    res.timings.push((tr!("startup.scan.t_publisher"), pub_ms));

    for it in startup.iter_mut().chain(services.iter_mut()) {
        if it.kind != "Packaged" {
            it.publisher = publisher(&it.file_path);
            // Task trong \Microsoft\ đã được đánh dấu is_ms từ trước.
            it.is_ms = it.is_ms || is_ms_publisher(&it.publisher);
        }
    }
    super::util::save_publisher_cache();

    // Số dịch vụ dùng chung một tiến trình.
    let mut shared: HashMap<u32, u32> = HashMap::new();
    for s in services.iter().filter(|s| s.pid > 0) {
        *shared.entry(s.pid).or_insert(0) += 1;
    }
    for s in services.iter_mut() {
        s.shared = if s.pid > 0 { shared[&s.pid] } else { 0 };
    }

    // Dịch vụ cho tab Khởi động: Tự động, hoặc Thủ công của bên thứ ba.
    for s in &services {
        let auto = s.start_type == "auto" || s.start_type == "delayed";
        if auto || (s.start_type == "demand" && !s.is_ms) {
            let mut it = s.clone();
            it.kind = "Service".into();
            it.make_id();
            startup.push(it);
        }
    }

    for it in startup.iter_mut() {
        if it.id.is_empty() {
            it.make_id();
        }
    }
    dedupe_ids(&mut startup);

    res.startup = startup;
    res.services = services;
    res.took_ms = t0.elapsed().as_millis() as u64;
    res
}

fn dedupe_ids(items: &mut [Item]) {
    let mut seen: HashSet<String> = HashSet::new();
    for it in items.iter_mut() {
        let mut n = 1;
        let base = it.id.clone();
        while !seen.insert(it.id.clone()) {
            n += 1;
            it.id = format!("{base}#{n}");
        }
    }
}

pub fn svc_item(s: SvcInfo) -> Item {
    let file_path = exe_path(&s.path);
    let mut it = Item {
        kind: "Svc".into(),
        on: s.start_type == "auto" || s.start_type == "delayed",
        source: tr!("startup.src.service"),
        critical: is_critical(&s.name),
        key1: s.name,
        name: s.display,
        command: s.path,
        file_path,
        start_type: s.start_type,
        state: s.state,
        pid: s.pid,
        description: s.description,
        account: s.account,
        ..Default::default()
    };
    it.make_id();
    it
}

/// Dựng lại item dịch vụ (cả bản tab Dịch vụ) sau khi thao tác.
pub fn refresh_service(name: &str) -> Result<Item, String> {
    let info = services::query_one(name)?;
    let mut it = svc_item(info);
    it.publisher = publisher(&it.file_path);
    it.is_ms = is_ms_publisher(&it.publisher);
    Ok(it)
}
