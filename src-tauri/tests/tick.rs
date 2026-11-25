// cargo test --release --test tick -- --nocapture
// Đo chi phí một nhịp đo hoàn chỉnh như luồng monitor: tiến trình + chỉ số hệ thống + GPU.
use fomeg_windows_app_lib::core::{gpu::GpuSampler, metrics::MetricsSampler, procs::ProcSampler};
use std::time::{Duration, Instant};

fn thread_cpu_ms() -> f64 {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Threading::{GetCurrentThread, GetThreadTimes};
    let (mut a, mut b, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    unsafe {
        let _ = GetThreadTimes(GetCurrentThread(), &mut a, &mut b, &mut k, &mut u);
    }
    let f = |t: FILETIME| ((t.dwHighDateTime as u64) << 32 | t.dwLowDateTime as u64) as f64 / 10_000.0;
    f(k) + f(u)
}

#[test]
fn full_tick_cost() {
    let t = Instant::now();
    let mut ps = ProcSampler::new();
    let mut ms = MetricsSampler::new();
    let mut gs = GpuSampler::new();
    println!("khởi tạo: {:?}", t.elapsed());
    let _ = (ps.sample(), ms.sample(), gs.sample());
    let mut walls = Vec::new();
    let mut cpus = Vec::new();
    for _ in 0..15 {
        std::thread::sleep(Duration::from_millis(1000));
        let c0 = thread_cpu_ms();
        let t0 = Instant::now();
        let p = ps.sample();
        let m = ms.sample();
        let (g, gp) = gs.sample();
        let json = serde_json::to_string(&m).unwrap().len() + serde_json::to_string(&g).unwrap().len();
        walls.push(t0.elapsed().as_secs_f64() * 1000.0);
        cpus.push(thread_cpu_ms() - c0);
        if walls.len() == 15 {
            println!("tiến trình {} | CPU {:.1}% | RAM {} MB | GPU {:?} | pid có GPU {} | JSON metrics {} B",
                p.rows.len(), m.cpu.total, m.mem.used >> 20, g.iter().map(|x| x.util).collect::<Vec<_>>(), gp.len(), json);
        }
    }
    walls.sort_by(f64::total_cmp);
    cpus.sort_by(f64::total_cmp);
    println!("nhịp đo (wall ms): median {:.2}, max {:.2} | CPU luồng (ms, độ phân giải ~15.6ms): tổng {:.1} trên 15 nhịp",
        walls[7], walls[14], cpus.iter().sum::<f64>());
}
