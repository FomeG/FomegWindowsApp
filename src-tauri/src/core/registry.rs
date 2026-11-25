use winreg::enums::{RegType, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_SET_VALUE, KEY_WRITE};
use winreg::types::FromRegValue;
use winreg::{RegKey, RegValue};

use super::util::exe_path;
use crate::tr;
use super::Item;

pub const AB: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved";
pub const BACKUP_KEY: &str = r"HKCU\Software\fomeg-windows-app\Disabled";
const WINLOGON: &str = r"Software\Microsoft\Windows NT\CurrentVersion\Winlogon";
const PKG_BASE: &str =
    r"Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppModel\SystemAppData";

const RUN_DEFS: [(&str, &str, &str, &str); 3] = [
    ("HKCU", r"Software\Microsoft\Windows\CurrentVersion\Run", "Run", "startup.src.run_user"),
    ("HKLM", r"Software\Microsoft\Windows\CurrentVersion\Run", "Run", "startup.src.run_machine"),
    ("HKLM", r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run", "Run32", "startup.src.run_machine32"),
];

const MOVE_DEFS: [(&str, &str, &str); 4] = [
    ("HKCU", r"Software\Microsoft\Windows\CurrentVersion\RunOnce", "startup.src.runonce_user"),
    ("HKLM", r"Software\Microsoft\Windows\CurrentVersion\RunOnce", "startup.src.runonce_machine"),
    ("HKCU", r"Software\Microsoft\Windows\CurrentVersion\Policies\Explorer\Run", "startup.src.policy_user"),
    ("HKLM", r"Software\Microsoft\Windows\CurrentVersion\Policies\Explorer\Run", "startup.src.policy_machine"),
];

fn root(h: &str) -> RegKey {
    RegKey::predef(if h == "HKCU" { HKEY_CURRENT_USER } else { HKEY_LOCAL_MACHINE })
}

pub fn hive(path: &str) -> Result<(RegKey, &str), String> {
    if let Some(r) = path.strip_prefix(r"HKCU\") {
        Ok((RegKey::predef(HKEY_CURRENT_USER), r))
    } else if let Some(r) = path.strip_prefix(r"HKLM\") {
        Ok((RegKey::predef(HKEY_LOCAL_MACHINE), r))
    } else {
        Err(tr!("startup.err.bad_reg_key", path = path))
    }
}

fn string_values(k: &RegKey) -> Vec<(String, String)> {
    k.enum_values()
        .filter_map(|r| r.ok())
        .filter(|(n, _)| !n.is_empty())
        .map(|(n, v)| {
            let s = String::from_reg_value(&v).unwrap_or_default();
            (n, s)
        })
        .collect()
}

pub fn approved_on(h: &str, sub: &str, name: &str) -> bool {
    root(h)
        .open_subkey(sub)
        .and_then(|k| k.get_raw_value(name))
        .map(|v| v.bytes.first().is_none_or(|b| b & 1 == 0))
        .unwrap_or(true)
}

/// Registry Run + RunOnce/Policy + khoá backup + App Store (MSIX).
pub fn scan(out: &mut Vec<Item>) {
    for (h, sub, ab, label) in RUN_DEFS {
        let Ok(k) = root(h).open_subkey(sub) else { continue };
        let ab_sub = format!(r"{AB}\{ab}");
        for (n, cmd) in string_values(&k) {
            out.push(Item {
                kind: "Run".into(),
                on: approved_on(h, &ab_sub, &n),
                source: tr!(label),
                file_path: exe_path(&cmd),
                command: cmd,
                key1: format!(r"{h}\{ab_sub}"),
                key3: format!(r"{h}\{sub}"),
                name: n.clone(),
                key2: n,
                ..Default::default()
            });
        }
    }

    for (h, sub, label) in MOVE_DEFS {
        let Ok(k) = root(h).open_subkey(sub) else { continue };
        for (n, cmd) in string_values(&k) {
            out.push(Item {
                kind: "RegMove".into(),
                on: true,
                source: tr!(label),
                file_path: exe_path(&cmd),
                command: cmd,
                key1: format!(r"{h}\{sub}"),
                name: n.clone(),
                key2: n,
                ..Default::default()
            });
        }
    }

    if let Ok((r, s)) = hive(BACKUP_KEY) {
        if let Ok(bk) = r.open_subkey(s) {
            for (bn, cmd) in string_values(&bk) {
                let Some(idx) = bn.find('|') else { continue };
                if idx < 1 {
                    continue;
                }
                let orig = bn[..idx].replacen(r"HKCU:\", r"HKCU\", 1).replacen(r"HKLM:\", r"HKLM\", 1);
                let name = bn[idx + 1..].to_string();
                let label = MOVE_DEFS
                    .iter()
                    .find(|(h, sub, _)| format!(r"{h}\{sub}") == orig)
                    .map(|d| tr!(d.2))
                    .unwrap_or_else(|| orig.clone());
                out.push(Item {
                    kind: "RegMove".into(),
                    on: false,
                    source: label,
                    file_path: exe_path(&cmd),
                    command: cmd,
                    key1: orig,
                    name: name.clone(),
                    key2: name,
                    key3: bn,
                    ..Default::default()
                });
            }
        }
    }

    scan_packaged(out);
}

fn scan_packaged(out: &mut Vec<Item>) {
    let Ok(pk) = RegKey::predef(HKEY_CURRENT_USER).open_subkey(PKG_BASE) else { return };
    for pkg in pk.enum_keys().filter_map(|r| r.ok()) {
        let Ok(pkk) = pk.open_subkey(&pkg) else { continue };
        for task in pkk.enum_keys().filter_map(|r| r.ok()) {
            let Ok(tk) = pkk.open_subkey(&task) else { continue };
            let Ok(st) = tk.get_value::<u32, _>("State") else { continue };
            let short = pkg.split('_').next().unwrap_or(&pkg).to_string();
            let is_ms = short.to_ascii_lowercase().starts_with("microsoft");
            let publisher = if is_ms {
                "Microsoft Corporation".to_string()
            } else {
                short.split('.').next().unwrap_or("").to_string()
            };
            out.push(Item {
                kind: "Packaged".into(),
                name: format!("{short} ({task})"),
                source: "App Store (MSIX)".into(),
                command: pkg.clone(),
                key1: format!(r"HKCU\{PKG_BASE}\{pkg}\{task}"),
                on: st == 2 || st == 4,
                publisher,
                is_ms,
                ..Default::default()
            });
        }
    }
}

pub fn set_approved(key: &str, name: &str, on: bool) -> Result<(), String> {
    let (root, sub) = hive(key)?;
    let (k, _) = root.create_subkey(sub).map_err(reg_err)?;
    let mut bytes = vec![0u8; 12];
    bytes[0] = if on { 2 } else { 3 };
    // .into(): winreg 0.55 dùng Vec<u8>, 0.56+ dùng Cow<[u8]> – chạy được với cả hai
    #[allow(clippy::useless_conversion)]
    let bytes = bytes.into();
    k.set_raw_value(name, &RegValue { bytes, vtype: RegType::REG_BINARY }).map_err(reg_err)
}

pub fn move_value(from_key: &str, from_name: &str, to_key: &str, to_name: &str) -> Result<(), String> {
    let (r1, s1) = hive(from_key)?;
    let src = r1.open_subkey_with_flags(s1, KEY_READ | KEY_WRITE).map_err(reg_err)?;
    let val = src.get_raw_value(from_name).map_err(reg_err)?;
    let (r2, s2) = hive(to_key)?;
    let (dst, _) = r2.create_subkey(s2).map_err(reg_err)?;
    dst.set_raw_value(to_name, &val).map_err(reg_err)?;
    src.delete_value(from_name).map_err(reg_err)
}

pub fn set_packaged(key: &str, on: bool) -> Result<(), String> {
    let (root, sub) = hive(key)?;
    let k = root.open_subkey_with_flags(sub, KEY_SET_VALUE).map_err(reg_err)?;
    k.set_value("State", &(if on { 2u32 } else { 1u32 })).map_err(reg_err)
}

pub fn read_restart_apps() -> bool {
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(WINLOGON)
        .and_then(|k| k.get_value::<u32, _>("RestartApps"))
        .map(|v| v == 1)
        .unwrap_or(false)
}

pub fn set_restart_apps(on: bool) -> Result<(), String> {
    let (k, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(WINLOGON).map_err(reg_err)?;
    k.set_value("RestartApps", &(if on { 1u32 } else { 0u32 })).map_err(reg_err)
}

pub fn set_regedit_last_key(key: &str) {
    let full = key.replacen(r"HKCU\", r"HKEY_CURRENT_USER\", 1).replacen(r"HKLM\", r"HKEY_LOCAL_MACHINE\", 1);
    if let Ok((k, _)) =
        RegKey::predef(HKEY_CURRENT_USER).create_subkey(r"Software\Microsoft\Windows\CurrentVersion\Applets\Regedit")
    {
        let _ = k.set_value("LastKey", &format!(r"Computer\{full}"));
    }
}

fn reg_err(e: std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::PermissionDenied {
        tr!("errors.need_admin")
    } else {
        e.to_string()
    }
}
