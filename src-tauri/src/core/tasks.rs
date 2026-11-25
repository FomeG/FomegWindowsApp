// Scheduled Task qua COM (ITaskService) – nhanh hơn nhiều so với Get-ScheduledTask.
use std::mem::ManuallyDrop;
use crate::tr;

use windows::core::{Interface, BSTR};
use windows::Win32::Foundation::{VARIANT_BOOL, VARIANT_FALSE, VARIANT_TRUE};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
use windows::Win32::System::TaskScheduler::{
    IExecAction, IRegisteredTask, ITaskFolder, ITaskService, TaskScheduler, TASK_ACTION_EXEC, TASK_ACTION_TYPE,
    TASK_ENUM_HIDDEN, TASK_TRIGGER_BOOT, TASK_TRIGGER_LOGON, TASK_TRIGGER_TYPE2,
};
use windows::Win32::System::Variant::{VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VT_I4};

use super::util::{exe_path, win_err};
use super::Item;

fn vi4(i: i32) -> VARIANT {
    VARIANT {
        Anonymous: VARIANT_0 {
            Anonymous: ManuallyDrop::new(VARIANT_0_0 {
                vt: VT_I4,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: VARIANT_0_0_0 { lVal: i },
            }),
        },
    }
}

fn connect() -> Result<ITaskService, String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let svc: ITaskService = CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER).map_err(|e| win_err(&e))?;
        let e = VARIANT::default();
        svc.Connect(&e, &e, &e, &e).map_err(|e| win_err(&e))?;
        Ok(svc)
    }
}

/// Liệt kê toàn bộ thư mục task rồi chia cho nhiều luồng đọc song song
/// (đọc Definition của từng task là phần tốn thời gian nhất).
pub fn scan(out: &mut Vec<Item>) -> Result<(), String> {
    let svc = connect()?;
    let root = unsafe { svc.GetFolder(&BSTR::from("\\")) }.map_err(|e| win_err(&e))?;
    let mut folders = Vec::with_capacity(160);
    collect_folders(&root, &mut folders);
    drop(root);
    drop(svc);

    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(2, 8);
    // Chia xen kẽ để các luồng có khối lượng tương đương.
    let parts: Vec<Vec<String>> =
        (0..threads).map(|t| folders.iter().skip(t).step_by(threads).cloned().collect()).collect();
    std::thread::scope(|s| {
        let hs: Vec<_> = parts
            .iter()
            .filter(|p| !p.is_empty())
            .map(|part| {
                s.spawn(move || {
                    let mut v = Vec::new();
                    let Ok(svc) = connect() else { return v };
                    for path in part {
                        unsafe {
                            if let Ok(f) = svc.GetFolder(&BSTR::from(path.as_str())) {
                                read_tasks(&f, &mut v);
                            }
                        }
                    }
                    v
                })
            })
            .collect();
        for h in hs {
            if let Ok(v) = h.join() {
                out.extend(v);
            }
        }
    });
    Ok(())
}

fn collect_folders(folder: &ITaskFolder, out: &mut Vec<String>) {
    unsafe {
        if let Ok(p) = folder.Path() {
            out.push(p.to_string());
        }
        if let Ok(subs) = folder.GetFolders(0) {
            let n = subs.Count().unwrap_or(0);
            for i in 1..=n {
                if let Ok(f) = subs.get_Item(&vi4(i)) {
                    collect_folders(&f, out);
                }
            }
        }
    }
}

fn read_tasks(folder: &ITaskFolder, out: &mut Vec<Item>) {
    unsafe {
        if let Ok(tasks) = folder.GetTasks(TASK_ENUM_HIDDEN.0) {
            let n = tasks.Count().unwrap_or(0);
            for i in 1..=n {
                if let Ok(t) = tasks.get_Item(&vi4(i)) {
                    if let Some(it) = task_item(&t) {
                        out.push(it);
                    }
                }
            }
        }
    }
}

unsafe fn task_item(t: &IRegisteredTask) -> Option<Item> {
    let path = t.Path().ok()?.to_string();
    let name = t.Name().ok()?.to_string();
    let folder = path.strip_suffix(name.as_str()).unwrap_or("\\").to_string();
    let is_ms_path = folder.to_ascii_lowercase().starts_with("\\microsoft\\");
    let def = t.Definition().ok()?;

    let mut at_logon = false;
    if let Ok(trs) = def.Triggers() {
        let mut n = 0i32;
        let _ = trs.Count(&mut n);
        for j in 1..=n {
            if let Ok(tr) = trs.get_Item(j) {
                let mut ty = TASK_TRIGGER_TYPE2(0);
                if tr.Type(&mut ty).is_ok() && (ty == TASK_TRIGGER_LOGON || ty == TASK_TRIGGER_BOOT) {
                    at_logon = true;
                    break;
                }
            }
        }
    }
    if is_ms_path && !at_logon {
        return None;
    }

    let mut cmds: Vec<String> = Vec::new();
    let mut first_exe = String::new();
    if let Ok(acts) = def.Actions() {
        let mut n = 0i32;
        let _ = acts.Count(&mut n);
        for j in 1..=n {
            let Ok(a) = acts.get_Item(j) else { continue };
            let mut ty = TASK_ACTION_TYPE(-1);
            if a.Type(&mut ty).is_err() || ty != TASK_ACTION_EXEC {
                continue;
            }
            let Ok(ea) = a.cast::<IExecAction>() else { continue };
            let mut p = BSTR::new();
            let mut args = BSTR::new();
            let _ = ea.Path(&mut p);
            let _ = ea.Arguments(&mut args);
            let p = p.to_string();
            if p.is_empty() {
                continue;
            }
            if first_exe.is_empty() {
                first_exe = exe_path(&p);
            }
            cmds.push(format!("{} {}", p, args).trim().to_string());
        }
    }
    let enabled = t.Enabled().map(|v| v.as_bool()).unwrap_or(true);
    Some(Item {
        kind: "Task".into(),
        name: name.clone(),
        source: if at_logon { tr!("startup.src.task_logon") } else { tr!("startup.src.task_schedule") },
        command: cmds.join(" ; "),
        file_path: first_exe,
        key1: folder,
        key2: name,
        on: enabled,
        is_ms: is_ms_path,
        ..Default::default()
    })
}

pub fn set_enabled(folder: &str, name: &str, on: bool) -> Result<(), String> {
    let svc = connect()?;
    unsafe {
        let root = svc.GetFolder(&BSTR::from("\\")).map_err(|e| win_err(&e))?;
        let t = root.GetTask(&BSTR::from(format!("{folder}{name}"))).map_err(|e| win_err(&e))?;
        let v: VARIANT_BOOL = if on { VARIANT_TRUE } else { VARIANT_FALSE };
        t.SetEnabled(v).map_err(|e| win_err(&e))
    }
}
