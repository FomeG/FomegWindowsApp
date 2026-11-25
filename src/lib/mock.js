// Backend GIẢ LẬP – chỉ dùng khi chạy trong trình duyệt thường (không có Tauri).
// Được import động từ api.js nên không nằm trong bundle chính khi chạy Tauri.
// Phần v2 (metrics, tiến trình, AI) nằm trong mock-v2.js.
import { createV2Mock } from './mock-v2.js';
import { createDuMock } from './mock-du.js';

const MB = 1024 * 1024;
const GB = 1024 * MB;

function rng(seed) {
  let s = seed >>> 0;
  return () => {
    s = (s + 0x6d2b79f5) >>> 0;
    let t = s;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
const rand = rng(20261123);
const pick = (a) => a[Math.floor(rand() * a.length)];
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const jitter = (base) => base * (0.7 + rand() * 0.6);
const clone = (o) => JSON.parse(JSON.stringify(o));

const CRITICAL = new Set([
  'RpcSs', 'RpcEptMapper', 'DcomLaunch', 'LSM', 'SamSs', 'EventLog', 'Winmgmt', 'BFE', 'mpssvc', 'WinDefend',
  'Power', 'PlugPlay', 'CryptSvc', 'Dhcp', 'Dnscache', 'ProfSvc', 'Schedule', 'gpsvc', 'BrokerInfrastructure',
  'CoreMessagingRegistrar', 'SystemEventsBroker', 'StateRepository', 'UserManager', 'AudioSrv', 'AudioEndpointBuilder',
  'nsi', 'Wcmsvc', 'TimeBrokerSvc', 'wscsvc', 'SecurityHealthService',
]);

// [tên dịch vụ, tên hiển thị, nhóm svchost (hoặc '' nếu exe riêng), start_type, chạy?]
const MS_SERVICES = [
  ['AudioSrv', 'Windows Audio', 'LocalServiceNetworkRestricted', 'auto', 1],
  ['AudioEndpointBuilder', 'Windows Audio Endpoint Builder', 'LocalSystemNetworkRestricted', 'auto', 1],
  ['BFE', 'Base Filtering Engine', 'LocalServiceNoNetworkFirewall', 'auto', 1],
  ['BITS', 'Background Intelligent Transfer Service', 'netsvcs', 'delayed', 0],
  ['BrokerInfrastructure', 'Background Tasks Infrastructure Service', 'DcomLaunch', 'auto', 1],
  ['BthAvctpSvc', 'AVCTP service', 'LocalService', 'demand', 1],
  ['bthserv', 'Bluetooth Support Service', 'LocalService', 'demand', 1],
  ['CDPSvc', 'Connected Devices Platform Service', 'LocalService', 'delayed', 1],
  ['CoreMessagingRegistrar', 'CoreMessaging', 'LocalServiceNoNetwork', 'auto', 1],
  ['CryptSvc', 'Cryptographic Services', 'NetworkService', 'auto', 1],
  ['DcomLaunch', 'DCOM Server Process Launcher', 'DcomLaunch', 'auto', 1],
  ['DeviceAssociationService', 'Device Association Service', 'LocalSystemNetworkRestricted', 'demand', 1],
  ['Dhcp', 'DHCP Client', 'LocalServiceNetworkRestricted', 'auto', 1],
  ['DiagTrack', 'Connected User Experiences and Telemetry', 'utcsvc', 'auto', 1],
  ['DispBrokerDesktopSvc', 'Display Policy Service', 'LocalService', 'delayed', 1],
  ['Dnscache', 'DNS Client', 'NetworkService', 'auto', 1],
  ['DoSvc', 'Delivery Optimization', 'NetworkService', 'delayed', 1],
  ['DPS', 'Diagnostic Policy Service', 'LocalServiceNoNetwork', 'auto', 1],
  ['DsSvc', 'Data Sharing Service', 'LocalSystemNetworkRestricted', 'demand', 0],
  ['DusmSvc', 'Data Usage', 'LocalServiceNetworkRestricted', 'auto', 1],
  ['EventLog', 'Windows Event Log', 'LocalServiceNetworkRestricted', 'auto', 1],
  ['EventSystem', 'COM+ Event System', 'LocalService', 'auto', 1],
  ['Fax', 'Fax', '', 'demand', 0],
  ['fdPHost', 'Function Discovery Provider Host', 'LocalService', 'demand', 0],
  ['FontCache', 'Windows Font Cache Service', 'LocalService', 'auto', 1],
  ['gpsvc', 'Group Policy Client', 'netsvcs', 'auto', 1],
  ['hidserv', 'Human Interface Device Service', 'LocalSystemNetworkRestricted', 'demand', 1],
  ['iphlpsvc', 'IP Helper', 'NetSvcs', 'auto', 1],
  ['KeyIso', 'CNG Key Isolation', '', 'demand', 1],
  ['LanmanServer', 'Server', 'netsvcs', 'auto', 1],
  ['LanmanWorkstation', 'Workstation', 'NetworkService', 'auto', 1],
  ['lfsvc', 'Geolocation Service', 'netsvcs', 'demand', 0],
  ['LicenseManager', 'Windows License Manager Service', 'LocalService', 'demand', 1],
  ['lmhosts', 'TCP/IP NetBIOS Helper', 'LocalServiceNetworkRestricted', 'demand', 1],
  ['LSM', 'Local Session Manager', 'DcomLaunch', 'auto', 1],
  ['MapsBroker', 'Downloaded Maps Manager', 'NetworkService', 'delayed', 0],
  ['mpssvc', 'Windows Defender Firewall', 'LocalServiceNoNetworkFirewall', 'auto', 1],
  ['NcbService', 'Network Connection Broker', 'LocalSystemNetworkRestricted', 'demand', 1],
  ['Netman', 'Network Connections', 'LocalSystemNetworkRestricted', 'demand', 0],
  ['netprofm', 'Network List Service', 'LocalService', 'demand', 1],
  ['NlaSvc', 'Network Location Awareness', 'NetworkService', 'auto', 1],
  ['nsi', 'Network Store Interface Service', 'LocalService', 'auto', 1],
  ['PcaSvc', 'Program Compatibility Assistant Service', 'LocalSystemNetworkRestricted', 'delayed', 1],
  ['PlugPlay', 'Plug and Play', 'DcomLaunch', 'demand', 1],
  ['Power', 'Power', 'DcomLaunch', 'auto', 1],
  ['ProfSvc', 'User Profile Service', 'netsvcs', 'auto', 1],
  ['RasMan', 'Remote Access Connection Manager', 'netsvcs', 'demand', 0],
  ['RemoteRegistry', 'Remote Registry', 'localService', 'disabled', 0],
  ['RpcEptMapper', 'RPC Endpoint Mapper', 'RPCSS', 'auto', 1],
  ['RpcSs', 'Remote Procedure Call (RPC)', 'RPCSS', 'auto', 1],
  ['SamSs', 'Security Accounts Manager', '', 'auto', 1],
  ['Schedule', 'Task Scheduler', 'netsvcs', 'auto', 1],
  ['SecurityHealthService', 'Windows Security Service', '', 'demand', 1],
  ['SENS', 'System Event Notification Service', 'netsvcs', 'auto', 1],
  ['ShellHWDetection', 'Shell Hardware Detection', 'netsvcs', 'auto', 1],
  ['Spooler', 'Print Spooler', '', 'auto', 1],
  ['SSDPSRV', 'SSDP Discovery', 'LocalServiceAndNoImpersonation', 'demand', 1],
  ['StateRepository', 'State Repository Service', 'appmodel', 'demand', 1],
  ['StorSvc', 'Storage Service', 'LocalSystemNetworkRestricted', 'demand', 1],
  ['SysMain', 'SysMain', 'LocalSystemNetworkRestricted', 'auto', 1],
  ['SystemEventsBroker', 'System Events Broker', 'DcomLaunch', 'auto', 1],
  ['TabletInputService', 'Touch Keyboard and Handwriting Panel Service', 'LocalSystemNetworkRestricted', 'demand', 1],
  ['Themes', 'Themes', 'netsvcs', 'auto', 1],
  ['TimeBrokerSvc', 'Time Broker', 'LocalServiceNetworkRestricted', 'demand', 1],
  ['TokenBroker', 'Web Account Manager', 'netsvcs', 'demand', 1],
  ['TrkWks', 'Distributed Link Tracking Client', 'LocalSystemNetworkRestricted', 'auto', 1],
  ['UserManager', 'User Manager', 'netsvcs', 'auto', 1],
  ['UsoSvc', 'Update Orchestrator Service', 'netsvcs', 'delayed', 1],
  ['VaultSvc', 'Credential Manager', '', 'demand', 1],
  ['W32Time', 'Windows Time', 'LocalService', 'demand', 0],
  ['WaaSMedicSvc', 'Windows Update Medic Service', 'wusvcs', 'demand', 0],
  ['Wcmsvc', 'Windows Connection Manager', 'LocalServiceNetworkRestricted', 'auto', 1],
  ['WdiServiceHost', 'Diagnostic Service Host', 'LocalService', 'demand', 1],
  ['WinDefend', 'Microsoft Defender Antivirus Service', '', 'auto', 1],
  ['WinHttpAutoProxySvc', 'WinHTTP Web Proxy Auto-Discovery Service', 'LocalServiceNetworkRestricted', 'demand', 1],
  ['Winmgmt', 'Windows Management Instrumentation', 'netsvcs', 'auto', 1],
  ['WlanSvc', 'WLAN AutoConfig', 'LocalSystemNetworkRestricted', 'auto', 1],
  ['wscsvc', 'Security Center', 'LocalServiceNetworkRestricted', 'delayed', 1],
  ['WSearch', 'Windows Search', '', 'delayed', 1],
  ['wuauserv', 'Windows Update', 'netsvcs', 'demand', 0],
  ['WpnService', 'Windows Push Notifications System Service', 'netsvcs', 'auto', 1],
  ['XblAuthManager', 'Xbox Live Auth Manager', 'netsvcs', 'demand', 0],
  ['XblGameSave', 'Xbox Live Game Save', 'netsvcs', 'demand', 0],
  ['XboxNetApiSvc', 'Xbox Live Networking Service', 'netsvcs', 'demand', 0],
  ['WMPNetworkSvc', 'Windows Media Player Network Sharing Service', '', 'disabled', 0],
  ['seclogon', 'Secondary Logon', 'netsvcs', 'demand', 0],
  ['stisvc', 'Windows Image Acquisition (WIA)', 'imgsvc', 'demand', 0],
  ['TermService', 'Remote Desktop Services', 'NetworkService', 'demand', 0],
  ['SharedAccess', 'Internet Connection Sharing (ICS)', 'netsvcs', 'demand', 0],
  ['WerSvc', 'Windows Error Reporting Service', 'WerSvcGroup', 'demand', 0],
  ['wisvc', 'Windows Insider Service', 'netsvcs', 'disabled', 0],
  ['diagnosticshub.standardcollector.service', 'Microsoft (R) Diagnostics Hub Standard Collector Service', '', 'demand', 0],
  ['edgeupdate', 'Microsoft Edge Update Service (edgeupdate)', '', 'delayed', 0],
  ['edgeupdatem', 'Microsoft Edge Update Service (edgeupdatem)', '', 'demand', 0],
  ['MicrosoftEdgeElevationService', 'Microsoft Edge Elevation Service', '', 'demand', 0],
  ['ClickToRunSvc', 'Microsoft Office Click-to-Run Service', '', 'auto', 1],
  ['OneSyncSvc', 'Sync Host', 'UnistackSvcGroup', 'delayed', 1],
];
const PER_USER = [
  'CDPUserSvc', 'OneSyncSvc', 'WpnUserService', 'PimIndexMaintenanceSvc', 'UnistoreSvc', 'UserDataSvc', 'cbdhsvc',
  'BcastDVRUserService', 'BluetoothUserService', 'CaptureService', 'ConsentUxUserSvc', 'CredentialEnrollmentManagerUserSvc',
  'DeviceAssociationBrokerSvc', 'DevicePickerUserSvc', 'DevicesFlowUserSvc', 'MessagingService', 'PrintWorkflowUserSvc',
  'UdkUserSvc', 'webthreatdefusersvc', 'NPSMSvc', 'P9RdrService', 'PenService', 'AarSvc', 'cloudidsvc',
];
// [tên, tên hiển thị, nhà phát hành, exe, start_type, chạy?, MB]
const THIRD_SERVICES = [
  ['NVDisplay.ContainerLocalSystem', 'NVIDIA Display Container LS', 'NVIDIA Corporation', 'C:\\Windows\\System32\\DriverStore\\FileRepository\\nv_dispi.inf_amd64\\Display.NvContainer\\NVDisplay.Container.exe', 'auto', 1, 38],
  ['RtkAudioUniversalService', 'Realtek Audio Universal Service', 'Realtek Semiconductor', 'C:\\Windows\\System32\\RtkAudUService64.exe', 'auto', 1, 9],
  ['AdobeARMservice', 'Adobe Acrobat Update Service', 'Adobe Inc.', 'C:\\Program Files (x86)\\Common Files\\Adobe\\ARM\\1.0\\armsvc.exe', 'auto', 1, 4],
  ['gupdate', 'Google Update Service (gupdate)', 'Google LLC', 'C:\\Program Files (x86)\\Google\\Update\\GoogleUpdate.exe', 'delayed', 0, 0],
  ['GoogleChromeElevationService', 'Google Chrome Elevation Service', 'Google LLC', 'C:\\Program Files\\Google\\Chrome\\Application\\elevation_service.exe', 'demand', 0, 0],
  ['Steam Client Service', 'Steam Client Service', 'Valve Corporation', 'C:\\Program Files (x86)\\Common Files\\Steam\\steamservice.exe', 'demand', 0, 0],
  ['EpicOnlineServices', 'Epic Online Services', 'Epic Games, Inc.', 'C:\\Program Files (x86)\\Epic Games\\Epic Online Services\\EpicOnlineServices.exe', 'demand', 0, 0],
  ['TeamViewer', 'TeamViewer', 'TeamViewer Germany GmbH', 'C:\\Program Files\\TeamViewer\\TeamViewer_Service.exe', 'auto', 1, 26],
  ['AnyDesk', 'AnyDesk Service', 'AnyDesk Software GmbH', 'C:\\Program Files (x86)\\AnyDesk\\AnyDesk.exe', 'auto', 1, 18],
  ['com.docker.service', 'Docker Desktop Service', 'Docker Inc.', 'C:\\Program Files\\Docker\\Docker\\com.docker.service', 'demand', 0, 0],
  ['MySQL80', 'MySQL80', 'Oracle Corporation', 'C:\\Program Files\\MySQL\\MySQL Server 8.0\\bin\\mysqld.exe', 'auto', 1, 412],
  ['postgresql-x64-16', 'postgresql-x64-16 - PostgreSQL Server 16', 'PostgreSQL Global Development Group', 'C:\\Program Files\\PostgreSQL\\16\\bin\\pg_ctl.exe', 'auto', 1, 96],
  ['MSSQL$SQLEXPRESS', 'SQL Server (SQLEXPRESS)', 'Microsoft Corporation', 'C:\\Program Files\\Microsoft SQL Server\\MSSQL16.SQLEXPRESS\\MSSQL\\Binn\\sqlservr.exe', 'auto', 1, 1650],
  ['VMAuthdService', 'VMware Authorization Service', 'VMware, Inc.', 'C:\\Program Files (x86)\\VMware\\VMware Workstation\\vmware-authd.exe', 'auto', 1, 12],
  ['VMUSBArbService', 'VMware USB Arbitration Service', 'VMware, Inc.', 'C:\\Program Files (x86)\\Common Files\\VMware\\USB\\vmware-usbarbitrator64.exe', 'auto', 1, 6],
  ['LGHUBUpdaterService', 'LGHUB Updater Service', 'Logitech, Inc.', 'C:\\Program Files\\LGHUB\\lghub_updater.exe', 'auto', 1, 21],
  ['ASUSOptimization', 'ASUS Optimization', 'ASUSTeK Computer Inc.', 'C:\\Windows\\System32\\DriverStore\\FileRepository\\asussci2.inf_amd64\\ASUSOptimization.exe', 'auto', 1, 14],
  ['ArmouryCrateService', 'ARMOURY CRATE Service', 'ASUSTeK Computer Inc.', 'C:\\Program Files\\ASUS\\ARMOURY CRATE SE Service\\ArmouryCrate.Service.exe', 'auto', 1, 145],
  ['Razer Synapse Service', 'Razer Synapse Service', 'Razer Inc.', 'C:\\Program Files (x86)\\Razer\\Synapse3\\Service\\Razer Synapse Service.exe', 'auto', 1, 520],
  ['DbxSvc', 'DbxSvc', 'Dropbox, Inc.', 'C:\\Windows\\System32\\DbxSvc.exe', 'auto', 1, 3],
  ['WavesSysSvc', 'Waves Audio Services', 'Waves Audio Ltd.', 'C:\\Program Files\\Waves\\MaxxAudio\\WavesSysSvc64.exe', 'auto', 1, 7],
  ['IntelAudioService', 'Intel(R) Audio Service', 'Intel Corporation', 'C:\\Windows\\System32\\cAVS\\Intel(R) Audio Service\\IntelAudioService.exe', 'auto', 1, 8],
  ['jhi_service', 'Intel(R) Dynamic Application Loader Host Interface Service', 'Intel Corporation', 'C:\\Windows\\System32\\DriverStore\\FileRepository\\dal.inf_amd64\\jhi_service.exe', 'delayed', 1, 3],
  ['ZaloUpdateSvc', 'Zalo Update Service', 'VNG Corporation', 'C:\\Users\\ADMIN\\AppData\\Local\\Programs\\Zalo\\ZaloUpdate.exe', 'auto', 0, 0],
  ['UltraViewService', 'UltraViewer Service', 'DucFabulous Co.', 'C:\\Program Files (x86)\\UltraViewer\\UltraViewer_Service.exe', 'auto', 1, 11],
  ['UniKeyNT', 'UniKey Helper', 'UniKey Team', 'C:\\Program Files\\UniKey\\UniKeyNT.exe', 'disabled', 0, 0],
  ['CCleanerPerformanceOptimizerService', 'CCleaner Performance Optimizer Service', 'Piriform Software Ltd', 'C:\\Program Files\\CCleaner\\CCleanerPerformanceOptimizerService.exe', 'auto', 1, 5],
  ['avast! Antivirus', 'Avast Antivirus', 'Avast Software s.r.o.', 'C:\\Program Files\\Avast Software\\Avast\\AvastSvc.exe', 'auto', 1, 230],
  ['OracleVBoxSvc', 'VirtualBox system service', 'Oracle Corporation', 'C:\\Program Files\\Oracle\\VirtualBox\\VBoxSDS.exe', 'demand', 0, 0],
  ['WMIRegistrationService', 'Intel(R) Management Engine WMI Provider Registration', 'Intel Corporation', 'C:\\Windows\\System32\\DriverStore\\FileRepository\\mewmiprov.inf_amd64\\WMIRegistrationService.exe', 'auto', 1, 2],
];

const DESCS = [
  'Quản lý các thành phần hệ thống cần cho thiết bị âm thanh.',
  'Cung cấp hỗ trợ kết nối mạng và phân giải tên miền.',
  'Theo dõi và cập nhật trạng thái hệ thống, cần cho nhiều tính năng của Windows.',
  'Đồng bộ dữ liệu và thông báo cho ứng dụng.',
  'Dịch vụ hỗ trợ tính năng của phần cứng/driver.',
  'Tự động kiểm tra và cài bản cập nhật cho phần mềm.',
  '',
];

let PID = 1000;
const nextPid = () => (PID += 4 + Math.floor(rand() * 40));

let N = 0;
function mk(o) {
  return {
    id: o.id ?? `m${++N}`,
    kind: o.kind,
    name: o.name,
    source: o.source ?? '',
    command: o.command ?? '',
    file_path: o.file_path ?? '',
    key1: o.key1 ?? '',
    key2: o.key2 ?? '',
    key3: o.key3 ?? '',
    on: !!o.on,
    publisher: o.publisher ?? '',
    is_ms: !!o.is_ms,
    pid: o.pid ?? 0,
    start_type: o.start_type ?? '',
    state: o.state ?? '',
    description: o.description ?? '',
    account: o.account ?? (o.kind === 'Svc' ? acctFor(o.name || '') : ''),
    critical: !!o.critical,
    shared: o.shared ?? 0,
    _mb: o._mb,
    _group: o._group,
  };
}

// Tài khoản giả cho dịch vụ (ổn định theo tên).
const ACCOUNTS = ['LocalSystem', 'LocalSystem', 'NT AUTHORITY\\LocalService', 'NT AUTHORITY\\NetworkService', 'LocalSystem'];
function acctFor(name) {
  let h = 0;
  for (let i = 0; i < name.length; i++) h = (h * 31 + name.charCodeAt(i)) | 0;
  return ACCOUNTS[Math.abs(h) % ACCOUNTS.length];
}

// Cổng giả: [giao thức, cổng, địa chỉ, pid, tên, kết nối, cpu, RAM MB]
const MOCK_PORTS = [
  ['tcp', 135, ['0.0.0.0', '::'], 1104, 'svchost.exe', 0, 0, 12],
  ['tcp', 445, ['0.0.0.0', '::'], 4, 'System', 2, 0.6, 0.1],
  ['tcp', 1420, ['::1'], 18820, 'node.exe', 3, 1.4, 96],
  ['tcp', 3000, ['0.0.0.0'], 18820, 'node.exe', 0, 1.4, 96],
  ['tcp', 5173, ['127.0.0.1'], 20112, 'node.exe', 1, 0.2, 140],
  ['tcp', 5432, ['0.0.0.0', '::'], 6120, 'postgres.exe', 4, 0.1, 38],
  ['tcp', 6379, ['127.0.0.1'], 22004, 'wslrelay.exe', 1, 0, 6],
  ['tcp', 7680, ['0.0.0.0', '::'], 3020, 'svchost.exe', 0, 0, 22],
  ['tcp', 8080, ['127.0.0.1'], 14388, 'java.exe', 0, 3.2, 512],
  ['tcp', 11434, ['127.0.0.1'], 9876, 'ollama.exe', 0, 0, 210],
  ['tcp', 27017, ['127.0.0.1'], 7112, 'mongod.exe', 2, 0.3, 160],
  ['tcp', 49664, ['0.0.0.0', '::'], 812, 'lsass.exe', 0, 0, 14],
  ['tcp', 49665, ['0.0.0.0', '::'], 700, 'wininit.exe', 0, 0, 2],
  ['tcp', 57621, ['0.0.0.0'], 15120, 'Spotify.exe', 0, 0.4, 70],
  ['tcp', 2375, ['0.0.0.0'], 16400, 'com.docker.backend.exe', 0, 0.5, 180],
  ['tcp', 3389, ['0.0.0.0', '::'], 1432, 'svchost.exe', 0, 0, 18],
  ['tcp', 5040, ['192.168.1.12'], 5040, 'svchost.exe', 0, 0, 8],
  ['udp', 5353, ['0.0.0.0'], 2200, 'svchost.exe', 0, 0, 9],
  ['udp', 5353, ['0.0.0.0'], 15120, 'Spotify.exe', 0, 0.4, 70],
  ['udp', 1900, ['127.0.0.1', '::1'], 4210, 'svchost.exe', 0, 0, 7],
  ['udp', 123, ['0.0.0.0'], 2600, 'svchost.exe', 0, 0, 5],
];
const reachOf = (addrs) => (addrs.some((a) => a === '0.0.0.0' || a === '::') ? 'all' : addrs.every((a) => a === '127.0.0.1' || a === '::1') ? 'local' : 'network');
let killedPorts = new Set();

function buildServices() {
  const groups = new Map(); // group -> pid
  const out = [];
  for (const [key, display, group, st, run] of MS_SERVICES) {
    let pid = 0;
    if (run) {
      if (group) {
        if (!groups.has(group)) groups.set(group, nextPid());
        pid = groups.get(group);
      } else pid = nextPid();
    }
    const exe = group
      ? `C:\\Windows\\system32\\svchost.exe -k ${group} -p`
      : `C:\\Windows\\system32\\${key.toLowerCase()}.exe`;
    out.push(mk({
      id: `svc:${key}`, kind: 'Svc', name: display, key1: key, command: exe,
      file_path: group ? 'C:\\Windows\\system32\\svchost.exe' : `C:\\Windows\\system32\\${key.toLowerCase()}.exe`,
      source: 'Dịch vụ Windows', publisher: 'Microsoft Corporation', is_ms: true, pid,
      start_type: st, state: run ? 'running' : 'stopped', critical: CRITICAL.has(key),
      description: pick(DESCS), _group: group,
    }));
  }
  const suffix = '4a7b1c';
  for (const base of PER_USER) {
    const key = `${base}_${suffix}`;
    const run = rand() < 0.6;
    const group = 'UnistackSvcGroup';
    if (run && !groups.has(group)) groups.set(group, nextPid());
    out.push(mk({
      id: `svc:${key}`, kind: 'Svc', name: `${base.replace(/Svc$|Service$/, '')} (${key})`, key1: key,
      command: `C:\\Windows\\system32\\svchost.exe -k ${group}`, file_path: 'C:\\Windows\\system32\\svchost.exe',
      source: 'Dịch vụ theo người dùng', publisher: 'Microsoft Corporation', is_ms: true,
      pid: run ? groups.get(group) : 0, start_type: rand() < 0.5 ? 'auto' : 'demand',
      state: run ? 'running' : 'stopped', description: 'Dịch vụ theo phiên người dùng.', _group: group,
    }));
  }
  for (const [key, display, pub, exe, st, run, mb] of THIRD_SERVICES) {
    out.push(mk({
      id: `svc:${key}`, kind: 'Svc', name: display, key1: key, command: `"${exe}"`, file_path: exe,
      source: 'Dịch vụ bên thứ ba', publisher: pub, is_ms: pub === 'Microsoft Corporation', pid: run ? nextPid() : 0,
      start_type: st, state: run ? 'running' : 'stopped', description: pick(DESCS), _mb: mb,
    }));
  }
  // Dịch vụ driver / hệ thống để đủ ~250
  const drivers = ['ACPI', 'AFD', 'ahcache', 'amdgpio2', 'BasicDisplay', 'Beep', 'bowser', 'CLFS', 'CNG', 'disk', 'Dfsc', 'FileInfo', 'Filetrace', 'FltMgr', 'HTTP', 'iaStorAVC', 'intelppm', 'kbdclass', 'ksecdd', 'mouclass', 'mountmgr', 'Mup', 'NDIS', 'NetBIOS', 'NetBT', 'Npfs', 'Ntfs', 'Null', 'partmgr', 'pci', 'rdyboost', 'spaceport', 'storahci', 'Tcpip', 'tcpipreg', 'volmgr', 'volsnap', 'Wof', 'WdFilter', 'WdNisDrv'];
  for (const d of drivers) {
    const boot = ['ACPI', 'disk', 'Ntfs', 'pci', 'CLFS', 'CNG', 'FltMgr', 'ksecdd', 'mountmgr', 'partmgr', 'volmgr', 'storahci', 'Wof', 'WdFilter', 'spaceport', 'rdyboost', 'volsnap', 'iaStorAVC'].includes(d);
    out.push(mk({
      id: `svc:${d}`, kind: 'Svc', name: d, key1: d, command: `\\SystemRoot\\System32\\drivers\\${d.toLowerCase()}.sys`,
      file_path: `C:\\Windows\\System32\\drivers\\${d.toLowerCase()}.sys`, source: 'Driver', publisher: 'Microsoft Corporation',
      is_ms: true, start_type: boot ? 'boot' : pick(['system', 'demand', 'auto']), state: rand() < 0.7 ? 'running' : 'stopped',
      description: 'Driver hệ thống.',
    }));
  }
  const fillers = ['AppReadiness', 'Appinfo', 'AppXSvc', 'AssignedAccessManagerSvc', 'autotimesvc', 'AxInstSV', 'BDESVC', 'camsvc', 'CertPropSvc', 'ClipSVC', 'COMSysApp', 'cphs', 'CscService', 'defragsvc', 'DeviceInstall', 'DevQueryBroker', 'diagsvc', 'DialogBlockingService', 'DmEnrollmentSvc', 'dmwappushservice', 'dot3svc', 'EapHost', 'EFS', 'embeddedmode', 'EntAppSvc', 'FDResPub', 'FrameServer', 'FrameServerMonitor', 'GraphicsPerfSvc', 'icssvc', 'IKEEXT', 'InstallService', 'InventorySvc', 'IpxlatCfgSvc', 'KtmRm', 'lltdsvc', 'McpManagementService', 'MixedRealityOpenXRSvc', 'MSDTC', 'MSiSCSI', 'msiserver', 'NaturalAuthentication', 'NcaSvc', 'NcdAutoSetup', 'Netlogon', 'NetSetupSvc', 'NetTcpPortSharing', 'NgcCtnrSvc', 'NgcSvc', 'p2pimsvc', 'p2psvc', 'PeerDistSvc', 'perceptionsimulation', 'PerfHost', 'PhoneSvc', 'pla', 'PNRPAutoReg', 'PNRPsvc', 'PolicyAgent', 'PrintNotify', 'PushToInstall', 'QWAVE', 'RasAuto', 'RetailDemo', 'RmSvc', 'RpcLocator', 'SCardSvr', 'ScDeviceEnum', 'SCPolicySvc', 'SDRSVC', 'SEMgrSvc', 'SensorDataService', 'SensorService', 'SensrSvc', 'SessionEnv', 'SgrmBroker', 'shpamsvc', 'smphost', 'SmsRouter', 'SNMPTrap', 'spectrum', 'sppsvc', 'SstpSvc', 'svsvc', 'swprv', 'TapiSrv', 'TieringEngineService', 'TroubleshootingSvc', 'tzautoupdate', 'UevAgentService', 'UmRdpService', 'upnphost', 'vds', 'vmicguestinterface', 'vmicheartbeat', 'VSS', 'WalletService', 'WarpJITSvc', 'wbengine', 'WbioSrvc', 'wcncsvc', 'WdiSystemHost', 'WdNisSvc', 'WebClient', 'Wecsvc', 'WEPHOSTSVC', 'WFDSConMgrSvc', 'WiaRpc', 'WinRM', 'wlidsvc', 'wlpasvc', 'WManSvc', 'wmiApSrv', 'WPDBusEnum', 'WpcMonSvc', 'WwanSvc', 'XboxGipSvc'];
  for (const key of fillers) {
    if (out.length >= 252) break;
    const run = rand() < 0.3;
    const group = pick(['netsvcs', 'LocalService', 'LocalSystemNetworkRestricted', 'NetworkService']);
    if (run && !groups.has(group)) groups.set(group, nextPid());
    out.push(mk({
      id: `svc:${key}`, kind: 'Svc', name: key.replace(/([a-z])([A-Z])/g, '$1 $2'), key1: key,
      command: `C:\\Windows\\system32\\svchost.exe -k ${group} -p`, file_path: 'C:\\Windows\\system32\\svchost.exe',
      source: 'Dịch vụ Windows', publisher: 'Microsoft Corporation', is_ms: true, pid: run ? groups.get(group) : 0,
      start_type: run ? pick(['auto', 'demand', 'delayed']) : pick(['demand', 'demand', 'demand', 'disabled']),
      state: run ? 'running' : 'stopped', description: pick(DESCS), _group: group,
    }));
  }
  return out;
}

function recomputeShared(services) {
  const cnt = new Map();
  for (const s of services) if (s.pid && s.state === 'running') cnt.set(s.pid, (cnt.get(s.pid) || 0) + 1);
  for (const s of services) s.shared = s.pid && s.state === 'running' ? cnt.get(s.pid) : 0;
}

const HKCU_RUN = 'HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run';
const HKLM_RUN = 'HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run';
const HKLM_RUN32 = 'HKLM\\SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Run';
const APPROVED_CU = 'HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run';
const APPROVED_LM = 'HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run';
const APPROVED_LM32 = 'HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run32';
const U = 'C:\\Users\\ADMIN';

function buildStartup(services) {
  const out = [];
  // [tên, nhà phát hành, exe, args, hive, on, chạy, MB]
  const runs = [
    ['OneDrive', 'Microsoft Corporation', `${U}\\AppData\\Local\\Microsoft\\OneDrive\\OneDrive.exe`, '/background', 'cu', 1, 1, 142],
    ['SecurityHealth', 'Microsoft Corporation', 'C:\\Windows\\system32\\SecurityHealthSystray.exe', '', 'lm', 1, 1, 6],
    ['Discord', 'Discord Inc.', `${U}\\AppData\\Local\\Discord\\Update.exe`, '--processStart Discord.exe', 'cu', 1, 1, 612],
    ['Steam', 'Valve Corporation', 'C:\\Program Files (x86)\\Steam\\steam.exe', '-silent', 'cu', 1, 1, 286],
    ['Spotify', 'Spotify AB', `${U}\\AppData\\Roaming\\Spotify\\Spotify.exe`, '/minimized', 'cu', 0, 0, 0],
    ['Zalo', 'VNG Corporation', `${U}\\AppData\\Local\\Programs\\Zalo\\Zalo.exe`, '--startup', 'cu', 1, 1, 1720],
    ['EpicGamesLauncher', 'Epic Games, Inc.', 'C:\\Program Files (x86)\\Epic Games\\Launcher\\Portal\\Binaries\\Win64\\EpicGamesLauncher.exe', '-silent', 'cu', 0, 0, 0],
    ['RtkAudUService', 'Realtek Semiconductor', 'C:\\Windows\\System32\\DriverStore\\FileRepository\\realtekservice.inf_amd64\\RtkAudUService64.exe', '-background', 'lm', 1, 1, 9],
    ['Google Update', 'Google LLC', `${U}\\AppData\\Local\\Google\\Update\\GoogleUpdate.exe`, '/c', 'cu', 1, 0, 0],
    ['UniKey', 'UniKey Team', 'C:\\Program Files\\UniKey\\UniKeyNT.exe', '', 'cu', 1, 1, 4],
    ['IDMan', 'Tonec Inc.', 'C:\\Program Files (x86)\\Internet Download Manager\\IDMan.exe', '/onboot', 'cu', 1, 1, 48],
    ['Telegram Desktop', 'Telegram FZ-LLC', `${U}\\AppData\\Roaming\\Telegram Desktop\\Telegram.exe`, '-autostart', 'cu', 1, 1, 230],
    ['MicrosoftEdgeAutoLaunch_0A1B2C', 'Microsoft Corporation', 'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe', '--no-startup-window --win-session-start', 'cu', 0, 0, 0],
    ['Teams', 'Microsoft Corporation', `${U}\\AppData\\Local\\Microsoft\\Teams\\Update.exe`, '--processStart "Teams.exe" --process-start-args "--system-initiated"', 'cu', 1, 1, 455],
    ['Logitech Download Assistant', 'Logitech, Inc.', 'C:\\Windows\\system32\\rundll32.exe', 'C:\\Windows\\System32\\LogiLDA.dll,LogiFetch', 'lm', 0, 0, 0],
    ['iTunesHelper', 'Apple Inc.', 'C:\\Program Files\\iTunes\\iTunesHelper.exe', '', 'lm32', 1, 1, 11],
    ['Adobe Creative Cloud', 'Adobe Inc.', 'C:\\Program Files\\Adobe\\Adobe Creative Cloud\\ACC\\Creative Cloud.exe', '--showwindow=false --onOSstartup=true', 'lm32', 1, 1, 390],
    ['NVIDIA GeForce Experience', 'NVIDIA Corporation', 'C:\\Program Files\\NVIDIA Corporation\\NVIDIA GeForce Experience\\NVIDIA GeForce Experience.exe', '', 'lm', 1, 1, 168],
    ['CCleaner Smart Cleaning', 'Piriform Software Ltd', 'C:\\Program Files\\CCleaner\\CCleaner64.exe', '/MONITOR', 'lm', 0, 0, 0],
    ['WavesSvc', 'Waves Audio Ltd.', 'C:\\Program Files\\Waves\\MaxxAudio\\WavesSvc64.exe', '-Jack', 'lm', 1, 1, 12],
    ['Docker Desktop', 'Docker Inc.', 'C:\\Program Files\\Docker\\Docker\\Docker Desktop.exe', '-Autostart', 'cu', 1, 1, 2150],
  ];
  for (const [name, pub, exe, args, hive, on, run, mb] of runs) {
    const key = hive === 'cu' ? HKCU_RUN : hive === 'lm' ? HKLM_RUN : HKLM_RUN32;
    const appr = hive === 'cu' ? APPROVED_CU : hive === 'lm' ? APPROVED_LM : APPROVED_LM32;
    const src = hive === 'cu' ? 'Registry Run (Người dùng hiện tại)' : hive === 'lm' ? 'Registry Run (Máy – 64-bit)' : 'Registry Run (Máy – 32-bit)';
    out.push(mk({
      kind: 'Run', name, publisher: pub, is_ms: pub === 'Microsoft Corporation', command: `"${exe}"${args ? ' ' + args : ''}`,
      file_path: exe, key1: appr, key2: name, key3: key, on: !!on, source: src, pid: on && run ? nextPid() : 0, _mb: on && run ? mb : 0,
    }));
  }
  const regmoves = [
    ['Uninstall_Leftover', 'HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\RunOnce', 'RunOnce (Người dùng)', 'C:\\Windows\\system32\\cmd.exe /c del "%TEMP%\\setup_leftover.tmp"', '', 1],
    ['HiddenMiner?', 'HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Policies\\Explorer\\Run', 'Policy Run (Người dùng – ẩn)', `${U}\\AppData\\Roaming\\svc\\svhost.exe`, '', 1],
    ['CorpAgent', 'HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Policies\\Explorer\\Run', 'Policy Run (Máy – ẩn)', 'C:\\Program Files\\CorpAgent\\agent.exe /silent', 'Contoso Ltd.', 0],
    ['NvBackend_Setup', 'HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\RunOnce', 'RunOnce (Máy)', 'C:\\Program Files\\NVIDIA Corporation\\Installer2\\setup.exe -s', 'NVIDIA Corporation', 1],
    ['WindowsUpdateFinalize', 'HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\RunOnce', 'RunOnce (Máy)', 'C:\\Windows\\system32\\rundll32.exe C:\\Windows\\system32\\wuaueng.dll,Finalize', 'Microsoft Corporation', 1],
  ];
  for (const [name, key, src, cmd, pub, on] of regmoves) {
    out.push(mk({
      kind: 'RegMove', name, source: src, command: cmd, file_path: cmd.split(' ')[0].replace(/"/g, ''), key1: key, key2: name,
      key3: `${key}|${name}`, on: !!on, publisher: pub, is_ms: pub === 'Microsoft Corporation',
      pid: name === 'HiddenMiner?' ? nextPid() : 0, _mb: name === 'HiddenMiner?' ? 880 : 0,
    }));
  }
  const folder = [
    ['Rainmeter.lnk', 'Rainmeter', 'C:\\Program Files\\Rainmeter\\Rainmeter.exe', 'cu', 1, 35],
    ['Send to OneNote.lnk', 'Microsoft Corporation', 'C:\\Program Files\\Microsoft Office\\root\\Office16\\ONENOTEM.EXE', 'cu', 0, 0],
    ['Everything.lnk', 'voidtools', 'C:\\Program Files\\Everything\\Everything.exe', 'cu', 1, 64],
    ['ShareX.lnk', 'ShareX Team', 'C:\\Program Files\\ShareX\\ShareX.exe', 'cu', 1, 88],
    ['Zoom.lnk', 'Zoom Video Communications, Inc.', `${U}\\AppData\\Roaming\\Zoom\\bin\\Zoom.exe`, 'all', 0, 0],
    ['PowerToys.lnk', 'Microsoft Corporation', `${U}\\AppData\\Local\\PowerToys\\PowerToys.exe`, 'all', 1, 120],
  ];
  for (const [file, pub, exe, scope, on, mb] of folder) {
    const dir = scope === 'cu'
      ? `${U}\\AppData\\Roaming\\Microsoft\\Windows\\Start Menu\\Programs\\Startup`
      : 'C:\\ProgramData\\Microsoft\\Windows\\Start Menu\\Programs\\StartUp';
    out.push(mk({
      kind: 'Folder', name: file.replace(/\.lnk$/, ''), source: scope === 'cu' ? 'Thư mục Startup (Người dùng)' : 'Thư mục Startup (Tất cả người dùng)',
      command: `"${exe}"`, file_path: `${dir}\\${file}`, key1: `HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\StartupFolder`,
      key2: file, key3: dir, on: !!on, publisher: pub, is_ms: pub === 'Microsoft Corporation', pid: on ? nextPid() : 0, _mb: on ? mb : 0,
    }));
  }
  const tasks = [
    ['\\', 'GoogleUpdateTaskMachineUA{A1B2C3}', 'Google LLC', 'C:\\Program Files (x86)\\Google\\Update\\GoogleUpdate.exe /ua /installsource scheduler', 'Khi đăng nhập', 1],
    ['\\', 'MicrosoftEdgeUpdateTaskMachineCore', 'Microsoft Corporation', 'C:\\Program Files (x86)\\Microsoft\\EdgeUpdate\\MicrosoftEdgeUpdate.exe /c', 'Khi khởi động', 1],
    ['\\', 'Adobe Acrobat Update Task', 'Adobe Inc.', 'C:\\Program Files (x86)\\Common Files\\Adobe\\ARM\\1.0\\AdobeARM.exe', 'Khi đăng nhập', 1],
    ['\\', 'CCleanerSkipUAC - ADMIN', 'Piriform Software Ltd', '"C:\\Program Files\\CCleaner\\CCleaner.exe" $(Arg0)', 'Khi đăng nhập', 0],
    ['\\', 'OneDrive Reporting Task-S-1-5-21', 'Microsoft Corporation', `${U}\\AppData\\Local\\Microsoft\\OneDrive\\OneDriveStandaloneUpdater.exe /reporting`, 'Khi đăng nhập', 1],
    ['\\ASUS\\', 'ArmouryCrateUpdater', 'ASUSTeK Computer Inc.', 'C:\\Program Files\\ASUS\\ArmouryCrate\\Updater.exe', 'Khi khởi động', 1],
    ['\\', 'NvTmRep_CrashReport1_{B2FE1952}', 'NVIDIA Corporation', 'C:\\Program Files\\NVIDIA Corporation\\NvContainer\\nvtopps.exe', 'Khi đăng nhập', 1],
    ['\\Microsoft\\Office\\', 'Office Automatic Updates 2.0', 'Microsoft Corporation', 'C:\\Program Files\\Common Files\\Microsoft Shared\\ClickToRun\\OfficeC2RClient.exe /frequentupdate SCHEDULEDTASK', 'Khi khởi động', 1],
    ['\\', 'ZoomUpdateTask', 'Zoom Video Communications, Inc.', `${U}\\AppData\\Roaming\\Zoom\\bin\\Zoom.exe --update`, 'Khi đăng nhập', 0],
    ['\\Razer\\', 'Razer Synapse 3 Startup', 'Razer Inc.', 'C:\\Program Files (x86)\\Razer\\Synapse3\\WPFUI\\Framework\\Razer Synapse 3 Host\\Razer Synapse 3.exe', 'Khi đăng nhập', 1],
    ['\\', 'MSIAfterburner', 'MSI Co., LTD', 'C:\\Program Files (x86)\\MSI Afterburner\\MSIAfterburner.exe /s', 'Khi đăng nhập', 1],
    ['\\Microsoft\\Windows\\WindowsUpdate\\', 'Scheduled Start', 'Microsoft Corporation', 'C:\\Windows\\system32\\sc.exe start wuauserv', 'Khi khởi động', 1],
  ];
  for (const [path, name, pub, cmd, trig, on] of tasks) {
    const mb = name.startsWith('Razer') ? 340 : name === 'MSIAfterburner' ? 54 : 0;
    out.push(mk({
      kind: 'Task', name, source: `Scheduled Task (${trig})`, command: cmd, file_path: cmd.replace(/^"([^"]+)".*$/, '$1').split(' /')[0].split(' --')[0],
      key1: path, key2: name, on: !!on, publisher: pub, is_ms: pub === 'Microsoft Corporation', pid: on && mb ? nextPid() : 0, _mb: on ? mb : 0,
    }));
  }
  // Bản "Service" trong danh sách khởi động: các dịch vụ tự động bên thứ ba + vài dịch vụ MS
  const svcKeys = ['NVDisplay.ContainerLocalSystem', 'AdobeARMservice', 'gupdate', 'TeamViewer', 'AnyDesk', 'MySQL80', 'MSSQL$SQLEXPRESS', 'ArmouryCrateService', 'Razer Synapse Service', 'ZaloUpdateSvc', 'avast! Antivirus', 'WSearch', 'SysMain', 'DiagTrack', 'Spooler'];
  for (const k of svcKeys) {
    const s = services.find((x) => x.key1 === k);
    if (!s) continue;
    out.push(mk({
      id: `st-svc:${k}`, kind: 'Service', name: s.name, source: s.start_type === 'delayed' ? 'Dịch vụ (Tự động – trễ)' : 'Dịch vụ (Tự động)',
      command: s.command, file_path: s.file_path, key1: k, on: s.start_type === 'auto' || s.start_type === 'delayed',
      publisher: s.publisher, is_ms: s.is_ms, pid: s.pid, start_type: s.start_type, state: s.state, description: s.description,
      critical: s.critical, shared: s.shared,
    }));
  }
  const pk = [
    ['Microsoft Teams (work or school)', 'MSTeams_8wekyb3d8bbwe', 'Microsoft Corporation', 1, 380],
    ['Spotify Music', 'SpotifyAB.SpotifyMusic_zpdnekdrzrea0', 'Spotify AB', 0, 0],
    ['WhatsApp', '5319275A.WhatsAppDesktop_cv1g1gvanyjgm', 'WhatsApp Inc.', 1, 260],
    ['Phone Link', 'Microsoft.YourPhone_8wekyb3d8bbwe', 'Microsoft Corporation', 1, 95],
    ['Terminal', 'Microsoft.WindowsTerminal_8wekyb3d8bbwe', 'Microsoft Corporation', 0, 0],
    ['Netflix', '4DF9E0F8.Netflix_mcm4njqhnhss8', 'Netflix, Inc.', 0, 0],
    ['Xbox', 'Microsoft.GamingApp_8wekyb3d8bbwe', 'Microsoft Corporation', 0, 0],
  ];
  for (const [name, pkg, pub, on, mb] of pk) {
    out.push(mk({
      kind: 'Packaged', name, source: 'App Store (MSIX) – StartupTask', command: `shell:AppsFolder\\${pkg}!App`,
      file_path: `C:\\Program Files\\WindowsApps\\${pkg.replace('_', '_1.0.0.0_x64__')}`,
      key1: `HKCU\\Software\\Classes\\Local Settings\\Software\\Microsoft\\Windows\\CurrentVersion\\AppModel\\SystemAppData\\${pkg}\\StartupTask`,
      key2: 'StartupTask', on: !!on, publisher: pub, is_ms: pub === 'Microsoft Corporation', pid: on ? nextPid() : 0, _mb: on ? mb : 0,
    }));
  }
  return out;
}

export function createMockBackend() {
  const services = buildServices();
  recomputeShared(services);
  const startup = buildStartup(services);
  let scans = 0;
  const settings = {
    restart_apps: true,
    is_admin: !/[?&]noadmin\b/.test(location.search),
    log_path: 'C:\\Users\\ADMIN\\AppData\\Local\\fomeg-windows-app\\log.txt',
  };
  const TOTAL = 16 * GB;
  let used = 9.2 * GB;

  // RAM theo tiến trình
  const procMb = new Map(); // pid -> mb
  for (const s of services) {
    if (!s.pid || procMb.has(s.pid)) continue;
    procMb.set(s.pid, s._mb ?? (s._group ? 8 + rand() * (s._group === 'netsvcs' ? 60 : 25) : 2 + rand() * 30));
  }
  for (const it of startup) if (it.pid && it.kind !== 'Service') procMb.set(it.pid, it._mb || 3 + rand() * 20);
  const svcByKey = new Map(services.map((s) => [s.key1, s]));

  function itemMb(it) {
    if (it.kind === 'Service') {
      const s = svcByKey.get(it.key1);
      return s && s.state === 'running' && s.pid ? procMb.get(s.pid) || 0 : 0;
    }
    return it.pid && (it.kind === 'Svc' ? it.state === 'running' : it.on) ? procMb.get(it.pid) || 0 : 0;
  }
  const lastSent = new Map();
  function ramItems(reset) {
    const out = [];
    for (const it of startup.concat(services)) {
      const mb = Math.round(itemMb(it) * 10) / 10;
      if (reset) { if (mb > 0) out.push([it.id, mb]); }
      else if ((lastSent.get(it.id) || 0) !== mb) out.push([it.id, mb]);
      lastSent.set(it.id, mb);
    }
    if (reset) for (const k of [...lastSent.keys()]) if (!lastSent.get(k)) lastSent.delete(k);
    return out;
  }
  function top() {
    const svchostMb = [...new Set(services.filter((s) => s._group && s.pid && s.state === 'running').map((s) => s.pid))]
      .reduce((a, p) => a + (procMb.get(p) || 0), 0);
    let base = [
      { name: 'chrome.exe', mb: 1850 + rand() * 300, count: 24 },
      { name: 'Code.exe', mb: 980 + rand() * 120, count: 14 },
      { name: 'msedge.exe', mb: 640 + rand() * 80, count: 11 },
      { name: 'svchost.exe', mb: svchostMb + 380, count: 86 },
      { name: 'explorer.exe', mb: 210 + rand() * 20, count: 1 },
      { name: 'MsMpEng.exe', mb: 240 + rand() * 30, count: 1 },
      { name: 'dwm.exe', mb: 150 + rand() * 40, count: 1 },
    ];
    for (const it of startup) {
      const mb = itemMb(it);
      if (mb > 100 && it.kind !== 'Service') base.push({ name: (it.file_path.split('\\').pop() || it.name), mb, count: 1 + Math.floor(mb / 300) });
    }
    const merged = new Map();
    for (const x of base) {
      const m = merged.get(x.name);
      if (m) { m.mb += x.mb; m.count += x.count; } else merged.set(x.name, { ...x });
    }
    base = [...merged.values()];
    base.sort((a, b) => b.mb - a.mb);
    return base.slice(0, 10).map((x) => ({ ...x, mb: Math.round(x.mb) }));
  }

  const listeners = new Set();
  let first = true;
  setInterval(() => {
    // random walk
    for (const [pid, mb] of procMb) {
      if (rand() < 0.25) procMb.set(pid, Math.max(1, mb * (0.92 + rand() * 0.17)));
    }
    used = Math.min(TOTAL * 0.93, Math.max(TOTAL * 0.35, used + (rand() - 0.48) * 0.25 * GB));
    const payload = { t: Date.now(), used: Math.round(used), total: TOTAL, items: ramItems(first), top: top(), reset: first };
    first = false;
    for (const cb of listeners) cb({ event: 'ram', payload });
  }, 2000);

  const history = [];
  {
    const now = Date.now();
    let u = 8.4 * GB;
    for (let t = now - 3600_000; t <= now; t += 2000) {
      u = Math.min(TOTAL * 0.9, Math.max(TOTAL * 0.38, u + (rand() - 0.49) * 0.08 * GB + Math.sin(t / 400000) * 0.01 * GB));
      history.push([t, Math.round(u)]);
    }
    used = u;
  }

  const strip = (o) => {
    const c = clone(o);
    delete c._mb; delete c._group;
    return c;
  };
  const failSometimes = () => rand() < 0.08;

  function setSvcMode(s, mode) {
    s.start_type = mode;
    const st = startup.find((x) => x.kind === 'Service' && x.key1 === s.key1);
    if (st) { st.start_type = mode; st.on = mode === 'auto' || mode === 'delayed'; }
  }
  function syncStartupFromSvc(s) {
    const st = startup.find((x) => x.kind === 'Service' && x.key1 === s.key1);
    if (st) { st.state = s.state; st.pid = s.pid; st.shared = s.shared; st.start_type = s.start_type; st.on = s.start_type === 'auto' || s.start_type === 'delayed'; }
  }

  const handlers = {
    async scan() {
      const t0 = performance.now();
      await sleep(jitter(320));
      scans++;
      const took = Math.round(performance.now() - t0);
      return {
        startup: startup.map(strip),
        services: services.map(strip),
        took_ms: took,
        timings: [['Registry', Math.round(took * 0.18)], ['Thư mục Startup', Math.round(took * 0.05)], ['Scheduled Task', Math.round(took * 0.42)], ['Dịch vụ', Math.round(took * 0.3)], ['MSIX', Math.round(took * 0.05)]],
        warnings: scans === 1 ? ['Không đọc được task "\\Microsoft\\Windows\\UpdateOrchestrator\\Schedule Scan": Truy cập bị từ chối (mã 5).'] : [],
      };
    },
    async toggle_items({ items, on }) {
      await sleep(jitter(400));
      return items.map((x) => {
        const it = startup.find((s) => s.id === x.id);
        if (!it) return { id: x.id, ok: false, error: 'Không tìm thấy mục (đã bị xoá?). Hãy làm mới.' };
        if (failSometimes()) return { id: x.id, ok: false, error: 'Không ghi được registry: Truy cập bị từ chối (mã 5).' };
        if (it.kind === 'Service') {
          const s = svcByKey.get(it.key1);
          if (!on && s?.critical) return { id: x.id, ok: false, error: 'Dịch vụ hệ thống quan trọng, app không cho đổi.' };
          if (s) setSvcMode(s, on ? 'auto' : 'demand');
        } else {
          it.on = on;
          if (!on) it.pid = 0;
        }
        return { id: x.id, ok: true, item: strip(it) };
      });
    },
    async service_set_mode({ names, mode }) {
      await sleep(jitter(450));
      return names.map((name) => {
        const s = svcByKey.get(name);
        if (!s) return { name, ok: false, error: 'Không tìm thấy dịch vụ.' };
        if (s.critical && (mode === 'demand' || mode === 'disabled')) return { name, ok: false, error: 'Dịch vụ hệ thống quan trọng, app không cho đổi.' };
        if (failSometimes()) return { name, ok: false, error: 'OpenService thất bại: Truy cập bị từ chối (mã 5).' };
        setSvcMode(s, mode);
        return { name, ok: true, service: strip(s) };
      });
    },
    async service_control({ names, action }) {
      await sleep(jitter(1300));
      const res = names.map((name) => {
        const s = svcByKey.get(name);
        if (!s) return { name, ok: false, error: 'Không tìm thấy dịch vụ.' };
        if (action === 'stop' && s.critical) return { name, ok: false, error: 'Dịch vụ hệ thống quan trọng, app không cho dừng.', service: strip(s) };
        if (action === 'start' && s.start_type === 'disabled') return { name, ok: false, error: 'Dịch vụ đang bị tắt hẳn – hãy đổi sang "Thủ công" trước khi chạy.', service: strip(s) };
        if (failSometimes()) return { name, ok: false, error: 'Dịch vụ không phản hồi kịp thời (mã 1053).', service: strip(s) };
        if (action === 'start') {
          s.state = 'running';
          if (!s.pid) {
            s.pid = s._group ? (services.find((o) => o._group === s._group && o.pid && o.state === 'running')?.pid ?? nextPid()) : nextPid();
            if (!procMb.has(s.pid)) procMb.set(s.pid, s._mb || 5 + rand() * 30);
          }
        } else {
          s.state = 'stopped';
          s.pid = 0;
        }
        return { name, ok: true };
      });
      recomputeShared(services);
      for (const r of res) {
        const s = svcByKey.get(r.name);
        if (s) { syncStartupFromSvc(s); if (r.ok) r.service = strip(s); }
      }
      return res;
    },
    async open_location({ item }) { await sleep(120); console.info('[mock] open_location', item?.file_path || item?.key1); return null; },
    async get_settings() { await sleep(60); return { ...settings }; },
    async set_restart_apps({ on }) { await sleep(jitter(250)); settings.restart_apps = !!on; return null; },
    async mcp_info() { await sleep(30); return { path: 'C:\\Users\\ADMIN\\AppData\\Local\\fomeg-windows-app\\fomeg-windows-app-mcp.exe', exists: true, embedded: true }; },
    async open_log() { await sleep(100); console.info('[mock] open_log', settings.log_path); return null; },
    async ports_list({ udp }) {
      await sleep(jitter(30));
      const uses = MOCK_PORTS.filter((p) => (udp || p[0] === 'tcp') && !killedPorts.has(p[3])).map(([proto, port, addrs, pid, name, conns, cpu, mb]) => ({
        proto, port, addrs, pid, conns, reach: reachOf(addrs), name, exe: `C:\\mock\\${name}`,
        cpu: Math.max(0, cpu + (cpu ? (rand() - 0.5) * cpu : 0)), mem: Math.round(mb * 1048576 * (0.97 + rand() * 0.06)),
      }));
      return { uses, took_ms: 1.2 + rand(), pending: false };
    },
    async open_port({ port }) { await sleep(80); console.info('[mock] open_port', `http://localhost:${port}`); return null; },
    async open_tool({ name }) { await sleep(80); console.info('[mock] open_tool', name); return null; },
    async ram_history() { await sleep(80); return { total: TOTAL, points: history.slice() }; },
  };

  // Bus sự kiện chung cho v2 ('metrics', 'procs', 'ai').
  const bus = new Map();
  const emit = (name, payload) => {
    const set = bus.get(name);
    if (set) for (const cb of set) cb({ event: name, payload });
  };
  const duMock = createDuMock(emit);
  const portsReply = () =>
    [
      'Mình đã xem **' + MOCK_PORTS.filter((p) => p[0] === 'tcp' && !killedPorts.has(p[3])).length + ' cổng TCP** đang nghe. Đáng chú ý:',
      '',
      '- **3000 (node.exe)** mở cho *mọi mạng* – máy chủ dev nên chỉ nghe 127.0.0.1.',
      '- **2375 (Docker API)** mở cho mọi mạng mà không có TLS – rất rủi ro, ai trong mạng cũng điều khiển được Docker.',
      '- **8080 (java.exe)** chỉ máy này, nhưng có vẻ là app Spring quên tắt.',
      '- 135, 445, 3389 là của Windows (svchost/System) – bình thường, không nên đóng từ đây.',
      '',
      '```dong_cong',
      '3000 | máy chủ dev Node mở cho mọi mạng',
      '2375 | Docker API không TLS mở cho mọi mạng',
      '8080 | app Java/Spring có vẻ quên tắt',
      '445 | chia sẻ tệp của Windows',
      '```',
    ].join('\n');
  // Dọn dẹp giả: giống backend – trả kết quả ngay, 2 kho cache lớn "đang đo" rồi đến sau qua sự kiện cleanup_item.
  let cleanupId = 0;
  let cleanupItems = null;
  const L = 'C:\\Users\\An\\AppData\\Local';
  const RO = 'C:\\Users\\An\\AppData\\Roaming';
  const NOTE_APP = 'Thư mục cache của ứng dụng (Cache, Code Cache, GPUCache…). Ứng dụng tự tạo lại; lần mở đầu có thể chậm hơn một chút. Tệp đang dùng được bỏ qua.';
  const NOTE_BROWSER = 'Cache trình duyệt (ảnh, script, video của trang web đã tải). Không mất mật khẩu, lịch sử hay dấu trang. Đóng trình duyệt trước để dọn được nhiều hơn.';
  const NOTE_DEV = 'Cache của công cụ lập trình (gói, bản build đã tải). Xoá an toàn nhưng lần build / cài sau phải tải lại, có thể mất thời gian và dung lượng mạng.';
  const cleanupBase = () => {
    const day = Date.now() - 3 * 864e5;
    const it = (id, group, name, note, paths, bytes, files, recent, safety, on, error = null) => ({
      id, group, name, note, paths, bytes, files, recent_bytes: recent, recent_files: recent ? Math.ceil(files / 20) : 0,
      newest: files ? day : 0, safety, default_on: on, pending: false, error,
    });
    return [
      it('user_temp', 'temp', 'Tệp tạm của bạn', 'Thư mục %TEMP%: tệp tạm của chương trình và bộ cài. Windows và ứng dụng tự tạo lại khi cần.', [L + '\\Temp'], 4.6 * GB, 24919, 320 * MB, 'safe', true),
      it('windows_temp', 'temp', 'Tệp tạm của Windows', 'Windows\\Temp: tệp tạm của Windows, dịch vụ và bộ cài chạy quyền hệ thống.', ['C:\\Windows\\Temp'], 612 * MB, 1840, 12 * MB, 'safe', true),
      it('error_reports', 'system', 'Báo cáo lỗi Windows', 'Báo cáo sự cố đã gửi hoặc đang chờ gửi cho Microsoft. Máy không cần chúng để chạy.', [L + '\\Microsoft\\Windows\\WER', 'C:\\ProgramData\\Microsoft\\Windows\\WER\\ReportArchive', 'C:\\ProgramData\\Microsoft\\Windows\\WER\\ReportQueue'], 16 * MB, 56, 0, 'safe', true),
      it('crash_dumps', 'system', 'Tệp dump khi gặp sự cố', 'Ảnh bộ nhớ lúc chương trình hoặc Windows bị lỗi. Chỉ cần khi gửi cho kỹ thuật viên để tìm nguyên nhân.', [L + '\\CrashDumps', 'C:\\Windows\\Minidump', 'C:\\Windows\\LiveKernelReports'], 259 * MB, 10, 0, 'safe', false),
      it('windows_update', 'system', 'Bản cập nhật Windows đã tải', 'Tệp cài cập nhật đã tải về (SoftwareDistribution\\Download). Windows tự tải lại nếu còn cần. Đừng dọn khi Windows đang cập nhật.', ['C:\\Windows\\SoftwareDistribution\\Download'], 1.8 * GB, 412, 0, 'caution', false),
      it('delivery_opt', 'system', 'Bộ nhớ đệm Tối ưu hoá phân phối', 'Bản cập nhật Windows/Store lưu lại để chia sẻ cho máy khác trong mạng. Xoá an toàn.', ['C:\\Windows\\ServiceProfiles\\NetworkService\\AppData\\Local\\Microsoft\\Windows\\DeliveryOptimization\\Cache'], 920 * MB, 88, 0, 'safe', false),
      it('shader_cache', 'system', 'Cache shader đồ hoạ', 'Shader DirectX / NVIDIA / AMD đã biên dịch. Game và ứng dụng tự tạo lại; lần mở đầu có thể hơi giật.', [L + '\\D3DSCache', L + '\\NVIDIA\\DXCache', L + '\\NVIDIA\\GLCache'], 1.3 * GB, 873, 40 * MB, 'safe', false),
      it('inet_cache', 'system', 'Tệp Internet tạm', 'Cache của các thành phần web cũ trong Windows (INetCache). Tự tạo lại.', [L + '\\Microsoft\\Windows\\INetCache'], 4 * MB, 11, 0, 'safe', false),
      it('recycle', 'system', 'Thùng rác', 'Các tệp bạn đã xoá vào Thùng rác (mọi ổ). Dọn là mất hẳn, không khôi phục được.', [], 2 * GB, 6170, 0, 'caution', false),
      it('cache:google chrome', 'cache', 'Google Chrome', NOTE_BROWSER, [L + '\\Google\\Chrome\\User Data\\Default\\Cache', L + '\\Google\\Chrome\\User Data\\Default\\Code Cache', L + '\\Google\\Chrome\\User Data\\Default\\GPUCache', L + '\\Google\\Chrome\\User Data\\Profile 1\\Cache'], 1.2 * GB, 8450, 90 * MB, 'safe', false),
      it('cache:microsoft edge', 'cache', 'Microsoft Edge', NOTE_BROWSER, [L + '\\Microsoft\\Edge\\User Data\\Default\\Cache', L + '\\Microsoft\\Edge\\User Data\\Default\\Code Cache'], 380 * MB, 2210, 5 * MB, 'safe', false),
      it('cache:vs code', 'cache', 'VS Code', NOTE_APP, [R + '\\Code\\Cache', R + '\\Code\\CachedData', R + '\\Code\\Code Cache', R + '\\Code\\GPUCache'], 610 * MB, 1320, 30 * MB, 'safe', false),
      it('cache:discord', 'cache', 'Discord', NOTE_APP, [R + '\\discord\\Cache', R + '\\discord\\Code Cache', R + '\\discord\\GPUCache'], 245 * MB, 980, 8 * MB, 'safe', false),
      it('cache:spotify', 'cache', 'Spotify', NOTE_APP, [L + '\\Spotify\\Data'], 1.1 * GB, 3100, 60 * MB, 'safe', false),
      it('cache:npm', 'cache', 'npm', NOTE_DEV, [L + '\\npm-cache'], 2.4 * GB, 51000, 0, 'caution', false),
      it('cache:gradle', 'cache', 'Gradle', NOTE_DEV, ['C:\\Users\\An\\.gradle\\caches'], 3.9 * GB, 82000, 0, 'caution', false),
      it('cache:uv', 'cache', 'uv', NOTE_DEV, [L + '\\uv\\cache'], 9.7 * GB, 1250000, 0, 'caution', false),
      it('cache:~/.cache', 'cache', '~/.cache', 'Thư mục cache chung của nhiều công cụ; có thể chứa mô hình AI đã tải (Hugging Face, LM Studio…) rất lớn. Xoá thì lần dùng sau phải tải lại.', ['C:\\Users\\An\\.cache'], 14.2 * GB, 640, 0, 'caution', false),
    ];
  };
  // ---------- Ứng dụng đã cài ----------
  const PF = 'C:\\Program Files', PF86 = 'C:\\Program Files (x86)', LP = 'C:\\Users\\An\\AppData\\Local\\Programs';
  const day = (d) => Date.now() - d * 864e5;
  const app = (id, name, publisher, version, ago, est, location, o = {}) => ({
    id, kind: 'win32', name, publisher, version, installed: day(ago), est_bytes: est, location, location_guessed: false,
    icon: location ? `${location}\\${name.split(' ')[0]}.exe` : '', scope: 'machine', arch: '64-bit', can_uninstall: true, can_modify: false,
    msi: false, system: false, key: `HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\${id.split('|')[2]}`,
    command: `"${location}\\uninstall.exe"`, url: '', ...o,
  });
  const store = (full, name, publisher, ago, location) => ({
    id: `s|${full}`, kind: 'store', name, publisher, version: full.split('_')[1], installed: day(ago), est_bytes: 0, location, location_guessed: false,
    icon: `${location}\\Assets\\StoreLogo.scale-100.png`, scope: 'user', arch: '', can_uninstall: true, can_modify: false, msi: false, system: false,
    key: full, command: `Remove-AppxPackage ${full}`, url: '',
  });
  let appList = [
    app('w|m64|Google Chrome', 'Google Chrome', 'Google LLC', '131.0.6778.86', 220, 0, `${PF}\\Google\\Chrome\\Application`, { url: 'https://www.google.com/chrome' }),
    app('w|m64|Docker Desktop', 'Docker Desktop', 'Docker Inc.', '4.90.0', 12, 0, `${PF}\\Docker\\Docker`),
    app('w|u|Microsoft VS Code', 'Microsoft Visual Studio Code (User)', 'Microsoft Corporation', '1.96.2', 60, 380 * MB, `${LP}\\Microsoft VS Code`, { scope: 'user', command: `"${LP}\\Microsoft VS Code\\unins000.exe"` }),
    app('w|m64|{7C3E-1A2B}', 'Node.js', 'Node.js Foundation', '22.12.0', 90, 98 * MB, `${PF}\\nodejs`, { msi: true, can_modify: true, command: 'MsiExec.exe /X{7C3E-1A2B}', key: 'HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{7C3E-1A2B}' }),
    app('w|m32|Steam', 'Steam', 'Valve Corporation', '2.10.91.91', 400, 0, `${PF86}\\Steam`, { arch: '32-bit' }),
    app('w|m64|Dolphin Anty', 'Dolphin Anty 2026.147.225', 'Denis Zhitnyakov', '2026.147.225', 3, 0, `${PF}\\Dolphin Anty`, { location_guessed: true, command: `"${PF}\\Dolphin Anty\\Uninstall Dolphin Anty.exe" /allusers` }),
    app('w|m64|7-Zip', '7-Zip 24.08 (x64)', 'Igor Pavlov', '24.08', 300, 5.6 * MB, `${PF}\\7-Zip`),
    app('w|u|Discord', 'Discord', 'Discord Inc.', '1.0.9175', 45, 72 * MB, 'C:\\Users\\An\\AppData\\Local\\Discord', { scope: 'user', arch: '' }),
    app('w|m64|BatteryMon', 'BatteryMon V2.1', '', '', 700, 0, `${PF}\\BatteryMon`, { icon: '' }),
    app('w|m64|NVIDIA App', 'NVIDIA App', 'NVIDIA Corporation', '11.0.1.189', 30, 0, `${PF}\\NVIDIA Corporation\\NVIDIA App`, { can_uninstall: false }),
    store('Microsoft.WindowsCalculator_11.2411.1.0_x64__8wekyb3d8bbwe', 'Máy tính', 'Microsoft Corporation', 150, `${PF}\\WindowsApps\\Microsoft.WindowsCalculator_11.2411.1.0_x64__8wekyb3d8bbwe`),
    store('Microsoft.GamingApp_2411.1001.4.0_x64__8wekyb3d8bbwe', 'Xbox', 'Microsoft Corporation', 20, `${PF}\\WindowsApps\\Microsoft.GamingApp_2411.1001.4.0_x64__8wekyb3d8bbwe`),
    store('SpotifyAB.SpotifyMusic_1.252.0.0_x64__zpdnekdrzrea0', 'Spotify', 'Spotify AB', 8, `${PF}\\WindowsApps\\SpotifyAB.SpotifyMusic_1.252.0.0_x64__zpdnekdrzrea0`),
  ];
  const appSizes = { Chrome: 1.1 * GB, Docker: 3.4 * GB, 'VS Code': 412 * MB, nodejs: 104 * MB, Steam: 11.6 * GB, 'Dolphin Anty': 640 * MB, '7-Zip': 6 * MB, Discord: 380 * MB, BatteryMon: 3 * MB, 'NVIDIA App': 1.6 * GB, Calculator: 34 * MB, Gaming: 210 * MB, Spotify: 290 * MB };
  // lần dùng cuối giả lập: [số ngày trước (-1 = chưa thấy), số lần mở]
  const usedMock = { Steam: [3, 40], Docker: [200, 2], NVIDIA: [1, 0], Google: [0, 310], Dolphin: [1, 12], 'Microsoft Visual': [0, 88], Discord: [120, 5], Spotify: [9, 30], Xbox: [400, 0], Node: [-1, 0], '7-Zip': [500, 1], BatteryMon: [-1, 0], 'Máy tính': [14, 6] };
  for (const a of appList) {
    const [d, n] = usedMock[Object.keys(usedMock).find((k) => a.name.startsWith(k))] || [-1, 0];
    a.last_used = d < 0 ? 0 : day(d);
    a.launches = n;
  }
  let appGen = 0;
  let appsListed = false;
  handlers.apps_list = async ({ force } = {}) => { const slow = force || !appsListed; appsListed = true; await sleep(slow ? 300 : 5); return { apps: clone(appList), took_ms: slow ? 312 : 1, store_error: null, usage_full: true }; };
  handlers.apps_measure = async ({ gen, paths }) => {
    appGen = gen;
    for (const p of paths) {
      await sleep(180 + rand() * 300);
      if (appGen !== gen) return;
      const k = Object.keys(appSizes).find((x) => p.includes(x));
      const bytes = Math.round((k ? appSizes[k] : 50 * MB) * (0.9 + rand() * 0.2));
      emit('app_size', { gen, path: p, bytes, files: Math.round(bytes / 90000), done: !p.includes('Steam') });
    }
  };
  handlers.apps_measure_stop = async () => { appGen = -1; };
  handlers.apps_uninstall = async ({ id }) => {
    const a = appList.find((x) => x.id === id);
    if (!a) throw 'Không tìm thấy ứng dụng này nữa (có thể đã được gỡ).';
    // giả lập: trình gỡ chạy 3 s; Steam = người dùng bấm Huỷ
    setTimeout(() => {
      const removed = !a.name.startsWith('Steam');
      if (removed) appList = appList.filter((x) => x.id !== id);
      emit('app_uninstall', { id, name: a.name, removed, error: null, took_ms: 3000 });
    }, 3000);
    return a.name;
  };
  handlers.apps_modify = async ({ id }) => { console.info('[mock] modify', id); };
  handlers.apps_uninstall_stop = async () => { console.info('[mock] uninstall stop'); };
  handlers.apps_open = async ({ id, what }) => { console.info('[mock] open', what, id); };
  const R = 'C:\\Users\\An\\AppData\\Roaming', LA = 'C:\\Users\\An\\AppData\\Local';
  const found = (path, area, owner, reason, bytes, o = {}) => ({ path, package: null, kind: 'folder', area, owner, reason, sure: true, shared: null, bytes, files: Math.round(bytes / 60000), newest: day(20 + (bytes % 200)), partial: false, ...o });
  handlers.apps_related = async ({ id }) => {
    await sleep(500);
    const a = appList.find((x) => x.id === id);
    if (!a) throw 'Không tìm thấy ứng dụng này nữa.';
    const short = a.name.split(' ')[0];
    const out = [];
    if (a.location) out.push(found(a.location, 'Thư mục cài đặt', a.name, a.kind === 'store' ? 'Windows tự xoá khi gỡ app Store' : 'Trình gỡ thường tự xoá; nếu còn sót sẽ xoá nốt', appSizes[Object.keys(appSizes).find((k) => a.location.includes(k))] || 80 * MB, { kind: a.kind === 'store' ? 'info' : 'folder', sure: a.kind !== 'store' && !a.location_guessed }));
    if (a.kind === 'store') return out;
    if (a.name.startsWith('Dolphin')) out.push(found(`${RO}\\dolphin_anty`, 'AppData\\Roaming', a.name, `Tên thư mục khớp với ${a.name}`, 944 * MB));
    if (a.name.startsWith('Steam')) out.push(found(`${LA}\\Steam`, 'AppData\\Local', a.name, `Tên thư mục khớp với ${a.name}`, 410 * MB), found(`${LA}\\Valve`, 'AppData\\Local', a.name, 'Thư mục của nhà phát hành Valve Corporation', 12 * MB, { sure: false }));
    if (a.name.startsWith('Microsoft Visual')) out.push(found(`${RO}\\Code`, 'AppData\\Roaming', a.name, `Tên thư mục khớp với ${a.name}`, 1.3 * GB), found('C:\\Users\\An\\.vscode', 'Hồ sơ người dùng', a.name, `Tên thư mục khớp với ${a.name}`, 520 * MB));
    if (a.name.startsWith('Discord')) out.push(found(`${RO}\\discord`, 'AppData\\Roaming', a.name, `Tên thư mục khớp với ${a.name}`, 640 * MB));
    if (a.name.startsWith('Google')) out.push(found(`${LA}\\Google\\Chrome`, 'AppData\\Local', a.name, 'Trong thư mục của nhà phát hành Google LLC', 2.8 * GB, { shared: null }));
    if (a.name.startsWith('Node')) out.push(found(`${RO}\\npm`, 'AppData\\Roaming', a.name, 'Tên thư mục khớp', 64 * MB, { sure: false, shared: 'Microsoft Visual Studio Code (User)' }));
    if (!out.length || out.length === 1) out.push(found(`${RO}\\${short}`, 'AppData\\Roaming', a.name, `Tên thư mục khớp với ${a.name}`, 18 * MB));
    return out;
  };
  let leftList = [
    found(`${RO}\\LDPlayer9`, 'AppData\\Roaming', 'LDPlayer 9', 'Tên thư mục khớp với LDPlayer 9 (đã gỡ 02/11/2026)', 1.9 * GB),
    found('C:\\LDPlayer\\LDPlayer9', 'Thư mục cài đặt cũ', 'LDPlayer 9', 'Thư mục cài đặt vẫn còn sau khi gỡ (đã gỡ 02/11/2026)', 3.1 * GB),
    found(`${LA}\\XuanZhi9`, 'AppData\\Local', 'LDPlayer 9', 'Trong thư mục của nhà phát hành XUANZHI (đã gỡ 02/11/2026)', 240 * MB, { sure: false }),
    found(`${LA}\\Packages\\Clipchamp.Clipchamp_yxz26nhyzhsrt`, 'Dữ liệu app Store', 'Clipchamp.Clipchamp', 'Dữ liệu của app Store không còn cài cho tài khoản này', 310 * MB),
    { ...found(`${PF}\\WindowsApps\\Microsoft.MinecraftUWP_1.21.4401.0_x64__8wekyb3d8bbwe`, 'WindowsApps', 'Microsoft.MinecraftUWP', 'Chỉ còn cài cho tài khoản khác (hoặc tài khoản đã xoá) – gỡ sẽ gỡ cho mọi tài khoản', 1.1 * GB, { sure: false }), kind: 'package', package: 'Microsoft.MinecraftUWP_1.21.4401.0_x64__8wekyb3d8bbwe' },
    { ...found(`${PF}\\WindowsApps\\Microsoft.GamingApp_2409.1001.6.0_x64__8wekyb3d8bbwe`, 'WindowsApps', 'Microsoft.GamingApp', 'Bản cũ của app đang dùng – Windows tự dọn sau khi app khởi động lại', 180 * MB, { sure: false }), kind: 'info' },
  ];
  handlers.leftovers_scan = async () => { await sleep(1400); return { items: clone(leftList), took_ms: 1380, removed_known: 4, windowsapps_error: null }; };
  handlers.leftovers_remove_packages = async ({ names }) => {
    await sleep(800);
    leftList = leftList.filter((f) => !names.includes(f.package));
    return names.map((name) => ({ name, ok: true, error: null }));
  };
  handlers.leftovers_forget = async ({ name }) => { leftList = leftList.filter((f) => f.owner !== name); };

  const cleaned = new Set();
  handlers.cleanup_scan = async () => {
    await sleep(900);
    const id = ++cleanupId;
    const full = cleanupBase().map((x) => (cleaned.has(x.id) ? { ...x, bytes: Math.round(x.recent_bytes * 0.4), files: x.recent_files, recent_bytes: Math.round(x.recent_bytes * 0.4) } : x));
    cleanupItems = full;
    const slow = new Set(['cache:uv', 'cache:gradle']);
    setTimeout(() => {
      for (const x of full) if (slow.has(x.id) && cleanupId === id) emit('cleanup_item', { scan: id, item: clone(x) });
    }, 2500);
    return { id, admin: true, took_ms: 1830, items: full.map((x) => (slow.has(x.id) ? { ...x, bytes: 0, files: 0, recent_bytes: 0, recent_files: 0, newest: 0, pending: true } : clone(x))) };
  };
  handlers.cleanup_apply = async ({ id, items, keepRecent }) => {
    if (id !== cleanupId || !cleanupItems) throw 'Kết quả quét đã cũ. Hãy quét lại trước khi dọn.';
    // giả lập `cleanup_progress`: ~6 s, 4 lần/giây, đi qua từng mục theo thứ tự
    const picked = cleanupItems.filter((x) => items.includes(x.id) && x.id !== 'recycle');
    const totalFiles = picked.reduce((n, x) => n + x.files, 0);
    const totalBytes = picked.reduce((n, x) => n + x.bytes, 0);
    const steps = 24;
    for (let i = 0; i <= steps && totalFiles; i++) {
      const f = i / steps;
      let acc = 0;
      const cur = picked.find((x) => (acc += x.files) >= f * totalFiles) || picked[picked.length - 1];
      emit('cleanup_progress', { stage: 'delete', done_files: Math.round(totalFiles * f * 0.98), total_files: totalFiles, freed_bytes: Math.round(totalBytes * f * 0.95), total_bytes: totalBytes, item: cur?.name || '', recycle: items.includes('recycle'), elapsed_ms: i * 250 });
      await sleep(250);
    }
    emit('cleanup_progress', { stage: 'measure', done_files: totalFiles, total_files: totalFiles, freed_bytes: Math.round(totalBytes * 0.95), total_bytes: totalBytes, item: '', recycle: false, elapsed_ms: steps * 250 });
    await sleep(600);
    const results = items.map((k) => {
      const x = cleanupItems.find((i) => i.id === k);
      if (!x) return { id: k, removed: 0, skipped: 0, kept_recent: 0, bytes: 0, error: null };
      cleaned.add(k);
      const keep = keepRecent ? x.recent_bytes : 0;
      const locked = Math.min(x.files, 3 + (x.files % 17));
      return { id: k, removed: Math.max(0, x.files - locked - (keepRecent ? x.recent_files : 0)), skipped: locked, kept_recent: keepRecent ? x.recent_files : 0, bytes: Math.round((x.bytes - keep) * 0.97), error: k === 'windows_update' ? 'Không mở được thư mục.' : null };
    });
    const fresh = cleanupItems
      .filter((x) => items.includes(x.id))
      .map((x) => (x.id === 'recycle' ? { ...x, bytes: 0, files: 0 } : { ...x, bytes: keepRecent ? Math.round(x.recent_bytes) : 0, files: keepRecent ? x.recent_files : 0, recent_bytes: keepRecent ? x.recent_bytes : 0 }));
    for (const x of fresh) cleanupItems[cleanupItems.findIndex((i) => i.id === x.id)] = x;
    return { results, items: clone(fresh), took_ms: 1180 };
  };
  const cleanupReply = () =>
    [
      'Mình đã xem kết quả quét. Tóm tắt:',
      '',
      '- **Tệp tạm** (4,6 GB + 612 MB) và **báo cáo lỗi** – dọn an toàn, Windows tự tạo lại.',
      '- **Cache shader** (1,3 GB) – an toàn, game có thể hơi giật ở lần mở đầu.',
      '- **Google Chrome / VS Code / Spotify** – cache tải lại được, không mất dữ liệu đăng nhập. Nên đóng app trước.',
      '- **~/.cache** (14 GB) có thể là *mô hình AI* đã tải – chỉ dọn nếu bạn không dùng nữa.',
      '- **uv, Gradle, npm** – dọn được nhưng lần build sau tải lại rất lâu.',
      '- **Thùng rác** (2 GB) – dọn là mất hẳn, kiểm tra trước.',
      '',
      '```don_dep',
      'user_temp | tệp tạm, an toàn',
      'windows_temp | tệp tạm của Windows',
      'error_reports | báo cáo lỗi đã gửi',
      'shader_cache | cache shader, tự tạo lại',
      'cache:google chrome | cache trình duyệt 1,2 GB',
      'cache:spotify | cache nhạc 1,1 GB, tải lại khi nghe',
      'khong_co | mục bịa để thử',
      '```',
    ].join('\n');
  Object.assign(handlers, createV2Mock(emit, { diskReply: duMock.__aiReply, portsReply, cleanupReply }));
  Object.assign(handlers, duMock);
  // Cổng giả: "kết thúc" PID giữ cổng thì cổng biến mất (PID này có thể không có trong bảng tiến trình giả).
  const v2Kill = handlers.proc_kill;
  handlers.proc_kill = async (args) => {
    const portPids = new Set(MOCK_PORTS.map((p) => p[3]));
    const res = await v2Kill(args);
    return res.map((r) => {
      if (!portPids.has(r.pid)) return r;
      if (r.pid === 4 || r.pid === 812 || r.pid === 700) return { pid: r.pid, ok: false, error: 'Truy cập bị từ chối (mã 5).' };
      killedPorts.add(r.pid);
      return { pid: r.pid, ok: true };
    });
  };

  return {
    async invoke(cmd, args = {}) {
      const h = handlers[cmd];
      if (!h) throw `Lệnh không tồn tại: ${cmd}`;
      return h(args);
    },
    async listen(name, cb) {
      if (name === 'ram') {
        listeners.add(cb);
        return () => listeners.delete(cb);
      }
      if (!bus.has(name)) bus.set(name, new Set());
      bus.get(name).add(cb);
      return () => bus.get(name).delete(cb);
    },
  };
}
