// Lõi native: quét + thao tác bằng Windows API trực tiếp (không qua PowerShell).
pub mod actions;
pub mod ai;
pub mod apps;
pub mod leftovers;
pub mod agent_pipe;
pub mod agent_tools;
pub mod cleanup;
pub mod gpu;
pub mod i18n;
pub mod icons;
pub mod metrics;
pub mod ports;
pub mod proc_actions;
pub mod procs;
pub mod folders;
pub mod mcp;
pub mod mem;
pub mod migrate;
pub mod registry;
pub mod scan;
pub mod du;
pub mod services;
pub mod shell;
pub mod smbios;
pub mod tasks;
pub mod usage;
pub mod util;

use serde::{Deserialize, Serialize};

/// Một mục khởi động hoặc một dịch vụ. `kind` quyết định cách bật/tắt và cách khớp RAM:
/// - `Run` / `Folder`: cờ StartupApproved (giống Task Manager). key1 = khoá StartupApproved, key2 = tên giá trị/file, key3 = khoá Run gốc / thư mục.
/// - `RegMove`: RunOnce / Policy Run, tắt bằng cách cất giá trị sang khoá backup. key1 = khoá gốc, key2 = tên, key3 = tên trong khoá backup.
/// - `Task`: Scheduled Task. key1 = thư mục task (có `\` cuối), key2 = tên task.
/// - `Service` (tab Khởi động) / `Svc` (tab Dịch vụ): key1 = tên dịch vụ.
/// - `Packaged`: App Store (MSIX) StartupTask. key1 = khoá registry chứa giá trị `State`.
#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default)]
pub struct Item {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub source: String,
    pub command: String,
    pub file_path: String,
    pub key1: String,
    pub key2: String,
    pub key3: String,
    pub on: bool,
    pub publisher: String,
    pub is_ms: bool,
    pub pid: u32,
    /// Dịch vụ: auto | delayed | demand | disabled | boot | system
    pub start_type: String,
    /// Dịch vụ: running | stopped | starting | stopping | paused | pending
    pub state: String,
    pub description: String,
    /// Dịch vụ: tài khoản chạy (lpServiceStartName), rỗng với mục khởi động
    pub account: String,
    pub critical: bool,
    /// Số dịch vụ dùng chung tiến trình (svchost)
    pub shared: u32,
}

impl Item {
    pub fn make_id(&mut self) {
        self.id = format!("{}:{}|{}", self.kind, self.key1.to_lowercase(), self.key2.to_lowercase());
    }
}

#[derive(Serialize, Default)]
pub struct ScanResult {
    pub startup: Vec<Item>,
    pub services: Vec<Item>,
    pub took_ms: u64,
    pub timings: Vec<(String, u64)>,
    pub warnings: Vec<String>,
}

// Dịch vụ hệ thống quan trọng: không cho chuyển sang Thủ công / Tắt / Dừng.
pub const CRITICAL: &[&str] = &[
    "wuauserv", "UsoSvc", "BITS", "WinDefend", "WdNisSvc", "SecurityHealthService", "mpssvc", "BFE", "RpcSs",
    "RpcEptMapper", "DcomLaunch", "LSM", "EventLog", "EventSystem", "Dhcp", "Dnscache", "nsi", "NlaSvc", "netprofm",
    "Wcmsvc", "WlanSvc", "LanmanWorkstation", "AudioSrv", "AudioEndpointBuilder", "Schedule", "ProfSvc", "Winmgmt",
    "CryptSvc", "SamSs", "TextInputManagementService", "Power", "PlugPlay", "SystemEventsBroker", "TimeBrokerSvc",
    "TokenBroker", "AppXSvc", "StateRepository", "CoreMessagingRegistrar", "gpsvc", "UserManager", "Themes", "SENS",
    "sppsvc", "TrustedInstaller", "msiserver", "DeviceInstall", "DispBrokerDesktopSvc", "BrokerInfrastructure",
    "ShellHWDetection", "KeyIso", "VaultSvc", "Appinfo", "camsvc", "WpnService", "FontCache", "DPS", "DusmSvc",
    "CDPSvc", "InstallService", "ClipSVC", "LicenseManager",
];

pub fn is_critical(name: &str) -> bool {
    CRITICAL.iter().any(|c| c.eq_ignore_ascii_case(name))
}
