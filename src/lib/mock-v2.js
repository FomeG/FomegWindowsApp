// Backend GIẢ LẬP cho contract v2: metrics, tiến trình, AI. Chỉ chạy trong trình duyệt thường.
// createV2Mock(emit) trả về các handler lệnh; sự kiện phát qua emit(tên, payload).

const KB = 1024;
const MB = 1024 * KB;
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
const rand = rng(7331);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const clamp = (v, a, b) => Math.min(b, Math.max(a, v));

const W = 'C:\\Windows\\System32\\';
const PF = 'C:\\Program Files\\';
const PF86 = 'C:\\Program Files (x86)\\';
const LA = 'C:\\Users\\ADMIN\\AppData\\Local\\';
const MS = 'Microsoft Corporation';

const CORES = 12;
const MEM_TOTAL = 16_869_330_944; // ~15,7 GB
const NET_ID = 'Intel[R] Wi-Fi 6E AX211 160MHz';
const ENGINES = ['3D', 'Copy', 'Video Decode', 'Video Encode'];

export function createV2Mock(emit, { diskReply, portsReply, cleanupReply } = {}) {
  const boot = Date.now() - (4 * 3600 + 20 * 60 + 13) * 1000;
  const info = {
    cpu_name: '12th Gen Intel(R) Core(TM) i5-12450H',
    cores: 8,
    logical: CORES,
    base_mhz: 2000,
    mem_total: MEM_TOTAL,
    mem_modules: [
      { slot: 'Bottom - Slot 1 (left)', maker: 'Micron Technology', part: '4ATF1G64HZ-3G2F1', size: 8 * GB, kind: 'DDR4', form: 'SODIMM', speed: 3200 },
      { slot: 'Bottom - Slot 2 (right)', maker: 'Micron Technology', part: '4ATF1G64HZ-3G2F1', size: 8 * GB, kind: 'DDR4', form: 'SODIMM', speed: 3200 },
    ],
    mem_slots: 2,
    board: 'HP 8A4F',
    gpus: [
      { id: '0', name: 'Intel(R) UHD Graphics', ded_total: 128 * MB, shared_total: 7.8 * GB },
      { id: '1', name: 'NVIDIA GeForce RTX 3050 Laptop GPU', ded_total: 4 * GB, shared_total: 7.8 * GB },
    ],
    disks: [{ id: '0', name: 'Đĩa 0 (C: D:)', model: 'KXG60ZNV1T02 NVMe KIOXIA 1024GB', size: 1_024_209_543_168, ssd: true }],
    system: 'HP Victus by HP Gaming Laptop 15-fa0xxx',
    bios: 'AMI · F.28 · 08/19/2024',
    os: 'Windows 11 Home 26H2 26300.1234',
    hostname: 'DESKTOP-MOCK',
    user: 'mock',
    boot_ms: boot,
    os_installed_ms: Date.now() - 400 * 864e5,
  };

  // ---------------- tiến trình ----------------
  let pidSeq = 1000;
  const nextPid = () => (pidSeq += 4 + Math.floor(rand() * 40) * 4);
  const procs = new Map(); // pid -> proc (có trường _ động)
  const fresh = []; // pid mới chưa gửi trong `added`
  let gone = []; // pid đã thoát chưa gửi trong `removed`
  const windowsMap = new Map(); // pid -> tiêu đề

  function add(o) {
    const pid = o.pid ?? nextPid();
    const p = {
      pid,
      ppid: o.ppid ?? 0,
      name: o.name,
      exe: o.exe ?? '',
      desc: o.desc ?? o.name.replace(/\.exe$/i, ''),
      publisher: o.publisher ?? '',
      cmd: o.cmd ?? (o.exe ? `"${o.exe}"` : ''),
      start_ms: o.start_ms ?? boot + Math.floor(rand() * 60_000),
      session: o.session ?? 1,
      elevated: o.elevated ?? false,
      user: o.user ?? 'ADMIN',
      kind: o.kind ?? 'background',
      _cpu: o.cpu ?? 0, // CPU nền (%)
      _mem: o.mem ?? (2 + rand() * 20) * MB,
      _disk: o.disk ?? 0,
      _gpu: o.gpu ?? 0,
      _thr: o.thr ?? 2 + Math.floor(rand() * 20),
      _hnd: o.hnd ?? 80 + Math.floor(rand() * 600),
      _ttl: o.ttl ?? 0, // >0: tự thoát sau N tick
    };
    procs.set(pid, p);
    fresh.push(pid);
    if (o.window) windowsMap.set(pid, o.window);
    return p;
  }

  const sys = (name, extra = {}) =>
    add({ name, exe: W + name, publisher: MS, kind: 'windows', user: 'SYSTEM', session: 0, desc: extra.desc, ...extra });

  // Cây cơ bản
  add({ pid: 4, ppid: 0, name: 'System', exe: '', desc: 'NT Kernel & System', publisher: MS, kind: 'windows', user: 'SYSTEM', session: 0, cpu: 0.6, mem: 0.1 * MB, thr: 280, hnd: 7200, start_ms: boot });
  add({ pid: 120, ppid: 4, name: 'Registry', exe: '', desc: 'Registry', publisher: MS, kind: 'windows', user: 'SYSTEM', session: 0, mem: 9 * MB, thr: 4, start_ms: boot });
  add({ pid: 512, ppid: 4, name: 'smss.exe', exe: W + 'smss.exe', desc: 'Windows Session Manager', publisher: MS, kind: 'windows', user: 'SYSTEM', session: 0, mem: 1 * MB, start_ms: boot });
  sys('csrss.exe', { pid: 700, ppid: 600, desc: 'Client Server Runtime Process', mem: 1.5 * MB });
  sys('csrss.exe', { pid: 812, ppid: 790, desc: 'Client Server Runtime Process', session: 1, mem: 2 * MB, cpu: 0.1 });
  const wininit = sys('wininit.exe', { pid: 788, ppid: 600, desc: 'Windows Start-Up Application', mem: 1.4 * MB });
  const services = sys('services.exe', { pid: 908, ppid: wininit.pid, desc: 'Services and Controller app', mem: 7 * MB, cpu: 0.1 });
  sys('lsass.exe', { pid: 932, ppid: wininit.pid, desc: 'Local Security Authority Process', mem: 11 * MB, cpu: 0.05 });
  const winlogon = sys('winlogon.exe', { pid: 860, ppid: 790, desc: 'Windows Logon Application', session: 1, mem: 3 * MB });
  sys('fontdrvhost.exe', { ppid: wininit.pid, desc: 'Usermode Font Driver Host', user: 'UMFD-0', mem: 1.8 * MB });
  sys('fontdrvhost.exe', { ppid: winlogon.pid, desc: 'Usermode Font Driver Host', user: 'UMFD-1', session: 1, mem: 4 * MB });
  sys('dwm.exe', { ppid: winlogon.pid, desc: 'Desktop Window Manager', user: 'DWM-1', session: 1, mem: 95 * MB, cpu: 1.2, gpu: 3, thr: 24, hnd: 2200 });

  const SVC_GROUPS = [
    ['DcomLaunch', 'SYSTEM'], ['RPCSS', 'NETWORK SERVICE'], ['netsvcs', 'SYSTEM'], ['LocalServiceNetworkRestricted', 'LOCAL SERVICE'],
    ['LocalSystemNetworkRestricted', 'SYSTEM'], ['LocalService', 'LOCAL SERVICE'], ['NetworkService', 'NETWORK SERVICE'],
    ['LocalServiceNoNetwork', 'LOCAL SERVICE'], ['UnistackSvcGroup', 'ADMIN'], ['appmodel', 'SYSTEM'], ['wsappx', 'SYSTEM'],
  ];
  const SVC_NAMES = ['Schedule', 'BITS', 'Dnscache', 'Dhcp', 'EventLog', 'AudioSrv', 'CryptSvc', 'DPS', 'WinHttpAutoProxySvc', 'nsi', 'Power',
    'BrokerInfrastructure', 'LSM', 'PlugPlay', 'UserManager', 'ProfSvc', 'Themes', 'SENS', 'ShellHWDetection', 'TokenBroker', 'StateRepository',
    'CDPSvc', 'DusmSvc', 'NlaSvc', 'netprofm', 'FontCache', 'TimeBrokerSvc', 'Winmgmt', 'WpnService', 'WpnUserService_5a3c1', 'CDPUserSvc_5a3c1',
    'OneSyncSvc_5a3c1', 'cbdhsvc_5a3c1', 'StorSvc', 'TrkWks', 'iphlpsvc', 'LanmanWorkstation', 'LanmanServer', 'SysMain', 'WSearch', 'DoSvc',
    'UsoSvc', 'wuauserv', 'InstallService', 'camsvc', 'Appinfo', 'AppXSvc', 'ClipSVC', 'DispBrokerDesktopSvc', 'DisplayEnhancementService',
    'hidserv', 'bthserv', 'BthAvctpSvc', 'WlanSvc', 'Wcmsvc', 'RmSvc', 'lmhosts', 'TabletInputService', 'SSDPSRV', 'fdPHost', 'WdiServiceHost',
    'WdiSystemHost', 'PcaSvc', 'SgrmBroker', 'webthreatdefusersvc_5a3c1', 'NgcCtnrSvc', 'NcbService', 'DeviceAssociationService', 'StiSvc',
    'Audiosrv', 'AudioEndpointBuilder', 'CoreMessagingRegistrar', 'SystemEventsBroker', 'gpsvc', 'EventSystem', 'LicenseManager', 'TextInputManagementService',
    'diagsvc', 'DiagTrack', 'MapsBroker', 'lfsvc', 'WaaSMedicSvc', 'XblAuthManager', 'NPSMSvc_5a3c1', 'PimIndexMaintenanceSvc_5a3c1', 'UdkUserSvc_5a3c1'];
  const svchosts = [];
  for (let i = 0; i < SVC_NAMES.length; i++) {
    const [grp, user] = SVC_GROUPS[i % SVC_GROUPS.length];
    const s = add({
      name: 'svchost.exe', exe: W + 'svchost.exe', ppid: services.pid, desc: 'Host Process for Windows Services', publisher: MS,
      cmd: `C:\\Windows\\system32\\svchost.exe -k ${grp} -p -s ${SVC_NAMES[i]}`, kind: 'windows', user, session: user === 'ADMIN' ? 1 : 0,
      mem: (1.5 + rand() * (i % 7 === 0 ? 60 : 12)) * MB, cpu: rand() < 0.12 ? 0.2 + rand() * 0.6 : 0, disk: SVC_NAMES[i] === 'WSearch' || SVC_NAMES[i] === 'SysMain' ? 40 * KB : 0,
    });
    svchosts.push(s);
  }
  const dcom = svchosts[0];

  const explorer = add({ name: 'explorer.exe', exe: 'C:\\Windows\\explorer.exe', ppid: 6044, desc: 'Windows Explorer', publisher: MS, kind: 'app', window: 'File Explorer', mem: 140 * MB, cpu: 0.4, thr: 110, hnd: 4300 });

  // Nhóm app nhiều tiến trình: một tiến trình gốc + con
  function family({ name, exe, desc, publisher, n, window, root = explorer.pid, mem = 80, cpu = 0.3, gpu = 0, kind, childCmd = (i) => `"${exe}" --type=renderer --renderer-client-id=${i + 5}`, elevated }) {
    const k = kind ?? (window ? 'app' : 'background');
    const top = add({ name, exe, desc, publisher, ppid: root, kind: k, window, mem: mem * MB * 1.6, cpu, gpu: gpu * 0.2, thr: 40 + Math.floor(rand() * 30), elevated });
    for (let i = 1; i < n; i++) {
      const isGpu = i === 1 && gpu > 0;
      add({
        name, exe, desc, publisher, ppid: top.pid, kind: k, elevated,
        cmd: isGpu ? `"${exe}" --type=gpu-process --gpu-preferences=UAAAAAAAAADgAAAEAAAAAAAAAAAAAAAAAABgAAEAAAA` : childCmd(i),
        mem: mem * MB * (0.15 + rand() * 1.2), cpu: rand() < 0.4 ? cpu * rand() : 0, gpu: isGpu ? gpu : 0,
      });
    }
    return top;
  }

  family({ name: 'chrome.exe', exe: PF + 'Google\\Chrome\\Application\\chrome.exe', desc: 'Google Chrome', publisher: 'Google LLC', n: 15, window: 'Tin tức hôm nay - Google Chrome', mem: 120, cpu: 1.2, gpu: 4 });
  family({ name: 'claude.exe', exe: LA + 'AnthropicClaude\\app-1.4.2\\claude.exe', desc: 'Claude', publisher: 'Anthropic, PBC', n: 12, window: 'Claude', mem: 140, cpu: 0.8, gpu: 1.5 });
  family({ name: 'Code.exe', exe: LA + 'Programs\\Microsoft VS Code\\Code.exe', desc: 'Visual Studio Code', publisher: MS, n: 10, window: 'mock-v2.js - fomeg-windows-app - Visual Studio Code', mem: 110, cpu: 0.6, gpu: 0.8 });
  family({ name: 'Spotify.exe', exe: 'C:\\Users\\ADMIN\\AppData\\Roaming\\Spotify\\Spotify.exe', desc: 'Spotify', publisher: 'Spotify AB', n: 5, window: 'Spotify Premium', mem: 70, cpu: 0.4, gpu: 0.6 });
  family({ name: 'Discord.exe', exe: LA + 'Discord\\app-1.0.9187\\Discord.exe', desc: 'Discord', publisher: 'Discord Inc.', n: 6, mem: 90, cpu: 0.3, gpu: 0.4 });
  family({ name: 'Zalo.exe', exe: LA + 'Programs\\Zalo\\Zalo-25.11.1\\Zalo.exe', desc: 'Zalo', publisher: 'VNG Online Co., Ltd', n: 6, window: 'Zalo', mem: 85, cpu: 0.3 });
  family({ name: 'steamwebhelper.exe', exe: PF86 + 'Steam\\bin\\cef\\cef.win7x64\\steamwebhelper.exe', desc: 'Steam Client WebHelper', publisher: 'Valve Corporation', n: 6, mem: 60, cpu: 0.1, root: add({ name: 'steam.exe', exe: PF86 + 'Steam\\steam.exe', desc: 'Steam', publisher: 'Valve Corporation', ppid: explorer.pid, mem: 70 * MB, cpu: 0.2 }).pid });
  family({ name: 'ms-teams.exe', exe: 'C:\\Program Files\\WindowsApps\\MSTeams_25290.205.4069.4894_x64__8wekyb3d8bbwe\\ms-teams.exe', desc: 'Microsoft Teams', publisher: MS, n: 4, mem: 75, cpu: 0.2 });
  family({ name: 'Docker Desktop.exe', exe: PF + 'Docker\\Docker\\frontend\\Docker Desktop.exe', desc: 'Docker Desktop', publisher: 'Docker Inc.', n: 4, mem: 65, cpu: 0.2 });
  add({ name: 'com.docker.backend.exe', exe: PF + 'Docker\\Docker\\resources\\com.docker.backend.exe', desc: 'Docker Desktop Backend', publisher: 'Docker Inc.', ppid: explorer.pid, mem: 45 * MB, cpu: 0.3, elevated: true });
  add({ name: 'vmmem', exe: '', desc: 'vmmem', publisher: '', ppid: 0, kind: 'windows', user: '', session: 0, mem: 1.9 * GB, cpu: 1.5 });

  // Edge WebView2 dưới vài host
  const searchHost = add({ name: 'SearchHost.exe', exe: 'C:\\Windows\\SystemApps\\MicrosoftWindows.Client.CBS_cw5n1h2txyewy\\SearchHost.exe', desc: 'Search application', publisher: MS, ppid: dcom.pid, kind: 'windows', mem: 110 * MB });
  const widgets = add({ name: 'Widgets.exe', exe: 'C:\\Program Files\\WindowsApps\\MicrosoftWindows.Client.WebExperience_525.1301.30.0_x64__cw5n1h2txyewy\\Dashboard\\Widgets.exe', desc: 'Widgets', publisher: MS, ppid: dcom.pid, mem: 40 * MB });
  const wv = PF86 + 'Microsoft\\EdgeWebView\\Application\\142.0.3595.80\\msedgewebview2.exe';
  for (const host of [searchHost, widgets, explorer]) {
    for (let i = 0; i < 6; i++) {
      add({ name: 'msedgewebview2.exe', exe: wv, desc: 'Microsoft Edge WebView2', publisher: MS, ppid: host.pid, mem: (10 + rand() * 70) * MB, cmd: `"${wv}" --embedded-browser-webview=1 --type=renderer --webview-exe-name=${host.name}` });
    }
  }

  // Ứng dụng đơn lẻ có cửa sổ
  const term = add({ name: 'WindowsTerminal.exe', exe: 'C:\\Program Files\\WindowsApps\\Microsoft.WindowsTerminal_1.23.12811.0_x64__8wekyb3d8bbwe\\WindowsTerminal.exe', desc: 'Windows Terminal', publisher: MS, ppid: explorer.pid, kind: 'app', window: 'Windows PowerShell', mem: 95 * MB, cpu: 0.2, gpu: 0.3 });
  for (let i = 0; i < 2; i++) {
    const ps = add({ name: 'powershell.exe', exe: W + 'WindowsPowerShell\\v1.0\\powershell.exe', desc: 'Windows PowerShell', publisher: MS, ppid: term.pid, mem: 70 * MB, cmd: '"C:\\WINDOWS\\System32\\WindowsPowerShell\\v1.0\\powershell.exe" -NoLogo' });
    add({ name: 'conhost.exe', exe: W + 'conhost.exe', desc: 'Console Window Host', publisher: MS, ppid: ps.pid, kind: 'windows', mem: 7 * MB });
    if (i === 0) for (let k = 0; k < 5; k++) add({ name: 'node.exe', exe: PF + 'nodejs\\node.exe', desc: 'Node.js JavaScript Runtime', publisher: 'Node.js', ppid: ps.pid, mem: (40 + rand() * 180) * MB, cpu: k === 0 ? 2.5 : 0.2, cmd: `"C:\\Program Files\\nodejs\\node.exe" ${k === 0 ? 'node_modules/vite/bin/vite.js' : 'node_modules/esbuild/lib/main.js --service=0.25.0 --ping'}` });
  }
  add({ name: 'fomeg-windows-app.exe', exe: 'C:\\Users\\ADMIN\\Downloads\\quanlykhoidong\\fomeg-windows-app.exe', desc: "Vietnamese's Ultimate W App", publisher: '', ppid: explorer.pid, kind: 'app', window: "Vietnamese's Ultimate W App", mem: 22 * MB, cpu: 0.3, elevated: true });
  add({ name: 'notepad.exe', exe: W + 'notepad.exe', desc: 'Notepad', publisher: MS, ppid: explorer.pid, kind: 'app', window: 'ghichu.txt - Notepad', mem: 30 * MB });
  add({ name: 'Taskmgr.exe', exe: W + 'Taskmgr.exe', desc: 'Task Manager', publisher: MS, ppid: explorer.pid, kind: 'app', window: 'Task Manager', mem: 45 * MB, cpu: 0.9, elevated: true });

  // Nền của bên thứ ba
  add({ name: 'OneDrive.exe', exe: LA + 'Microsoft\\OneDrive\\OneDrive.exe', desc: 'Microsoft OneDrive', publisher: MS, ppid: explorer.pid, mem: 60 * MB, disk: 15 * KB });
  add({ name: 'UniKeyNT.exe', exe: 'C:\\Tools\\UniKey\\UniKeyNT.exe', desc: 'UniKey', publisher: '', ppid: explorer.pid, mem: 4 * MB });
  add({ name: 'SecurityHealthSystray.exe', exe: W + 'SecurityHealthSystray.exe', desc: 'Windows Security notification icon', publisher: MS, ppid: explorer.pid, kind: 'windows', mem: 3 * MB });
  for (const [n, d, p, m] of [
    ['nvcontainer.exe', 'NVIDIA Container', 'NVIDIA Corporation', 30], ['nvcontainer.exe', 'NVIDIA Container', 'NVIDIA Corporation', 22], ['nvcontainer.exe', 'NVIDIA Container', 'NVIDIA Corporation', 18],
    ['NVDisplay.Container.exe', 'NVIDIA Container', 'NVIDIA Corporation', 25], ['NVDisplay.Container.exe', 'NVIDIA Container', 'NVIDIA Corporation', 12],
    ['RtkAudUService64.exe', 'Realtek HD Audio Universal Service', 'Realtek Semiconductor', 5], ['RtkAudUService64.exe', 'Realtek HD Audio Universal Service', 'Realtek Semiconductor', 4],
    ['igfxEM.exe', 'igfxEM Module', 'Intel Corporation', 8], ['jusched.exe', 'Java Update Scheduler', 'Oracle Corporation', 3],
    ['AdobeCollabSync.exe', 'Acrobat Collaboration Synchronizer', 'Adobe Inc.', 12], ['GoogleUpdate.exe', 'Google Installer', 'Google LLC', 2],
  ]) add({ name: n, exe: PF + n.replace('.exe', '') + '\\' + n, desc: d, publisher: p, ppid: n.startsWith('nv') || n.startsWith('NV') ? services.pid : explorer.pid, mem: m * MB, cpu: rand() < 0.3 ? 0.1 : 0, user: n.startsWith('nv') || n.startsWith('NV') ? 'SYSTEM' : 'ADMIN' });

  // Tiến trình Windows khác
  for (const [n, d, extra] of [
    ['sihost.exe', 'Shell Infrastructure Host', {}], ['taskhostw.exe', 'Host Process for Windows Tasks', {}], ['taskhostw.exe', 'Host Process for Windows Tasks', {}],
    ['ctfmon.exe', 'CTF Loader', { mem: 18 * MB }], ['spoolsv.exe', 'Spooler SubSystem App', { user: 'SYSTEM', session: 0 }],
    ['MsMpEng.exe', 'Antimalware Service Executable', { user: 'SYSTEM', session: 0, mem: 210 * MB, cpu: 0.8, disk: 300 * KB, exe: 'C:\\ProgramData\\Microsoft\\Windows Defender\\Platform\\4.18.25100.9-0\\MsMpEng.exe' }],
    ['NisSrv.exe', 'Microsoft Network Realtime Inspection Service', { user: 'LOCAL SERVICE', session: 0, mem: 9 * MB }],
    ['SecurityHealthService.exe', 'Windows Security Health Service', { user: 'SYSTEM', session: 0 }], ['audiodg.exe', 'Windows Audio Device Graph Isolation', { user: 'LOCAL SERVICE', session: 0, mem: 14 * MB, cpu: 0.3 }],
    ['smartscreen.exe', 'Windows Defender SmartScreen', {}], ['SgrmBroker.exe', 'System Guard Runtime Monitor Broker', { user: 'SYSTEM', session: 0 }],
    ['StartMenuExperienceHost.exe', 'Windows Start Experience Host', { mem: 60 * MB, exe: 'C:\\Windows\\SystemApps\\Microsoft.Windows.StartMenuExperienceHost_cw5n1h2txyewy\\StartMenuExperienceHost.exe' }],
    ['ShellExperienceHost.exe', 'Windows Shell Experience Host', { mem: 40 * MB, exe: 'C:\\Windows\\SystemApps\\ShellExperienceHost_cw5n1h2txyewy\\ShellExperienceHost.exe' }],
    ['TextInputHost.exe', 'Text Input Host', { mem: 35 * MB, exe: 'C:\\Windows\\SystemApps\\MicrosoftWindows.Client.CBS_cw5n1h2txyewy\\TextInputHost.exe' }],
    ['ApplicationFrameHost.exe', 'Application Frame Host', { mem: 20 * MB }], ['LockApp.exe', 'LockApp.exe', { mem: 30 * MB, exe: 'C:\\Windows\\SystemApps\\Microsoft.LockApp_cw5n1h2txyewy\\LockApp.exe' }],
    ['SearchIndexer.exe', 'Microsoft Windows Search Indexer', { user: 'SYSTEM', session: 0, mem: 35 * MB, disk: 120 * KB }],
    ['WUDFHost.exe', 'Windows Driver Foundation - User-mode Driver Framework Host Process', { user: 'LOCAL SERVICE', session: 0 }],
    ['WUDFHost.exe', 'Windows Driver Foundation - User-mode Driver Framework Host Process', { user: 'LOCAL SERVICE', session: 0 }],
    ['PhoneExperienceHost.exe', 'Microsoft Phone Link', { mem: 50 * MB, exe: 'C:\\Program Files\\WindowsApps\\Microsoft.YourPhone_1.25092.98.0_x64__8wekyb3d8bbwe\\PhoneExperienceHost.exe' }],
    ['UserOOBEBroker.exe', 'User OOBE Broker', {}], ['dasHost.exe', 'Device Association Framework Provider Host', { user: 'LOCAL SERVICE', session: 0 }],
    ['MpDefenderCoreService.exe', 'Antimalware Core Service', { user: 'SYSTEM', session: 0, mem: 25 * MB }],
    ['wlanext.exe', 'Windows Wireless LAN 802.11 Extensibility Framework', { user: 'SYSTEM', session: 0 }],
    ['unsecapp.exe', 'Sink to receive asynchronous callbacks for WMI client application', {}],
    ['Memory Compression', 'Memory Compression', { exe: '', user: 'SYSTEM', session: 0, mem: 600 * MB }],
  ]) add({ name: n, exe: W + n, desc: d, publisher: MS, ppid: extra.user === 'SYSTEM' || extra.session === 0 ? services.pid : dcom.pid, kind: 'windows', ...extra });
  for (let i = 0; i < 12; i++) add({ name: 'RuntimeBroker.exe', exe: W + 'RuntimeBroker.exe', desc: 'Runtime Broker', publisher: MS, ppid: dcom.pid, kind: 'windows', mem: (2 + rand() * 20) * MB });
  for (let i = 0; i < 10; i++) add({ name: 'dllhost.exe', exe: W + 'dllhost.exe', desc: 'COM Surrogate', publisher: MS, ppid: dcom.pid, kind: 'windows', mem: (2 + rand() * 10) * MB, cmd: `C:\\WINDOWS\\system32\\DllHost.exe /Processid:{${(0xab8902b4 + i).toString(16).toUpperCase()}-09CA-4BB6-B78D-A8F59079A8D5}` });
  for (let i = 0; i < 6; i++) add({ name: 'WmiPrvSE.exe', exe: W + 'wbem\\WmiPrvSE.exe', desc: 'WMI Provider Host', publisher: MS, ppid: dcom.pid, kind: 'windows', user: i % 2 ? 'NETWORK SERVICE' : 'SYSTEM', session: 0, mem: (4 + rand() * 20) * MB, cpu: i === 0 ? 0.4 : 0 });
  for (let i = 0; i < 14; i++) add({ name: 'conhost.exe', exe: W + 'conhost.exe', desc: 'Console Window Host', publisher: MS, ppid: svchosts[(i * 7) % svchosts.length].pid, kind: 'windows', user: 'SYSTEM', session: 0, mem: (1 + rand() * 6) * MB, cmd: '\\??\\C:\\WINDOWS\\system32\\conhost.exe 0x4' });
  for (let i = 0; i < 4; i++) add({ name: 'backgroundTaskHost.exe', exe: W + 'backgroundTaskHost.exe', desc: 'Background Task Host', publisher: MS, ppid: dcom.pid, kind: 'windows', mem: 15 * MB });
  fresh.length = 0; // trạng thái ban đầu: lần reset sẽ gửi tất cả

  // Ứng dụng có cửa sổ: cả nhóm (cùng exe) thành "app"
  const exeApp = new Set();
  for (const pid of windowsMap.keys()) exeApp.add(procs.get(pid)?.exe);
  for (const p of procs.values()) if (exeApp.has(p.exe) && p.exe) p.kind = 'app';

  const TRANSIENT = [
    ['conhost.exe', W + 'conhost.exe', 'Console Window Host'],
    ['backgroundTaskHost.exe', W + 'backgroundTaskHost.exe', 'Background Task Host'],
    ['dllhost.exe', W + 'dllhost.exe', 'COM Surrogate'],
    ['WmiPrvSE.exe', W + 'wbem\\WmiPrvSE.exe', 'WMI Provider Host'],
    ['SearchProtocolHost.exe', W + 'SearchProtocolHost.exe', 'Microsoft Windows Search Protocol Host'],
    ['git.exe', PF + 'Git\\cmd\\git.exe', 'Git for Windows'],
  ];
  function churn() {
    if (rand() < 0.35) {
      const [n, exe, d] = TRANSIENT[Math.floor(rand() * TRANSIENT.length)];
      const isGit = n === 'git.exe';
      add({ name: n, exe, desc: d, publisher: isGit ? 'The Git Development Community' : MS, ppid: isGit ? term.pid : dcom.pid, kind: isGit ? 'background' : 'windows', start_ms: Date.now(), mem: (2 + rand() * 12) * MB, cpu: rand() * 1.5, ttl: 3 + Math.floor(rand() * 12), cmd: isGit ? 'git.exe status --porcelain' : undefined });
    }
    for (const p of procs.values()) {
      if (p._ttl > 0 && --p._ttl === 0) removeProc(p.pid);
    }
  }
  function removeProc(pid) {
    if (!procs.has(pid)) return;
    procs.delete(pid);
    windowsMap.delete(pid);
    const fi = fresh.indexOf(pid);
    if (fi >= 0) fresh.splice(fi, 1);
    else gone.push(pid);
  }

  // ---------------- số liệu máy ----------------
  const cores = Array.from({ length: CORES }, () => 5 + rand() * 20);
  let memUsed = 12.9 * GB;
  let diskRead = 0, diskWrite = 0, rx = 2 * KB, tx = 3 * KB;
  const gpuState = info.gpus.map(() => ({ eng: ENGINES.map(() => 0), ded: 0, shared: 0 }));
  gpuState[0].ded = 90 * MB; gpuState[0].shared = 600 * MB;
  gpuState[1].ded = 520 * MB; gpuState[1].shared = 120 * MB;
  const battery = { percent: 87, charging: true, plugged: true };
  let phase = 0;

  function sampleProcs() {
    let cpuSum = 0;
    for (const p of procs.values()) {
      let c = 0;
      if (p._cpu > 0) c = p._cpu * (0.2 + rand() * 1.6) * (rand() < 0.03 ? 6 : 1);
      else if (rand() < 0.02) c = rand() * 0.8;
      p.cpu = Math.round(clamp(c, 0, 100) * 10) / 10;
      cpuSum += p.cpu;
      if (rand() < 0.3) p._mem = Math.max(0.2 * MB, p._mem * (0.985 + rand() * 0.032));
      p.disk = p._disk > 0 && rand() < 0.5 ? Math.round(p._disk * rand() * 3) : rand() < 0.01 ? Math.round(rand() * 2 * MB) : 0;
      p.gpu = p._gpu > 0 ? Math.round(p._gpu * (0.3 + rand() * 1.4) * 10) / 10 : 0;
      if (rand() < 0.1) p._thr = Math.max(1, p._thr + (rand() < 0.5 ? -1 : 1));
      if (rand() < 0.2) p._hnd = Math.max(10, p._hnd + Math.round((rand() - 0.5) * 12));
    }
    return cpuSum;
  }

  function sampleMetrics() {
    phase += 0.07;
    const cpuSum = sampleProcs();
    const target = clamp(cpuSum + 3 + Math.sin(phase) * 6, 1, 100);
    let tot = 0;
    for (let i = 0; i < CORES; i++) {
      const pref = i < 4 ? 1.5 : i < 8 ? 1 : 0.6; // P-core bận hơn
      cores[i] = clamp(cores[i] * 0.5 + target * pref * (0.4 + rand() * 1.2) * 0.5, 0, 100);
      tot += cores[i];
    }
    const cpuTotal = Math.round((tot / CORES) * 10) / 10;
    memUsed = clamp(memUsed + (rand() - 0.5) * 60 * MB, 0.55 * MEM_TOTAL, 0.93 * MEM_TOTAL);
    const burst = rand() < 0.12;
    diskRead = burst ? (2 + rand() * 40) * MB : Math.max(0, diskRead * 0.4 + rand() * 200 * KB);
    diskWrite = rand() < 0.1 ? (0.5 + rand() * 10) * MB : Math.max(0, diskWrite * 0.5 + rand() * 120 * KB);
    const active = clamp((diskRead + diskWrite) / (80 * MB) * 100 + rand() * 1.5, 0, 100);
    const nb = rand() < 0.08;
    rx = nb ? (200 + rand() * 3000) * KB : Math.max(300, rx * 0.55 + rand() * 6 * KB);
    tx = nb ? (20 + rand() * 300) * KB : Math.max(200, tx * 0.55 + rand() * 4 * KB);
    gpuState.forEach((g, gi) => {
      const base = gi === 0 ? 4 : 7;
      g.eng[0] = clamp(g.eng[0] * 0.5 + base * (0.3 + rand() * 1.4) * (rand() < 0.05 ? 6 : 1) * 0.5, 0, 100);
      g.eng[1] = rand() < 0.15 ? rand() * 6 : 0;
      g.eng[2] = gi === 1 && Math.sin(phase / 3) > 0.3 ? 8 + rand() * 6 : 0;
      g.eng[3] = 0;
      g.ded = clamp(g.ded + (rand() - 0.5) * 8 * MB, 40 * MB, info.gpus[gi].ded_total * 0.9);
      g.shared = clamp(g.shared + (rand() - 0.5) * 6 * MB, 50 * MB, 2 * GB);
    });
    if (rand() < 0.02) battery.percent = clamp(battery.percent + (battery.charging ? 1 : -1), 5, 100);
    if (battery.percent >= 100) battery.charging = false;
    let threads = 0, handles = 0;
    for (const p of procs.values()) { threads += p._thr; handles += p._hnd; }
    const t = Date.now();
    return {
      t,
      cpu: { total: cpuTotal, cores: cores.map((c) => Math.round(c * 10) / 10), mhz: Math.round(2900 + rand() * 900) },
      mem: {
        used: Math.round(memUsed), total: MEM_TOTAL, avail: Math.round(MEM_TOTAL - memUsed),
        committed: Math.round(memUsed * 1.55), commit_limit: Math.round(MEM_TOTAL + 16 * GB), cached: Math.round(2.4 * GB + rand() * 50 * MB),
        paged_pool: Math.round(610 * MB), nonpaged_pool: Math.round(320 * MB + rand() * 2 * MB),
      },
      disks: [{ id: '0', name: info.disks[0].name, active: Math.round(active * 10) / 10, read: Math.round(diskRead), write: Math.round(diskWrite) }],
      nets: [{ id: NET_ID, name: 'Wi-Fi', kind: 'wifi', rx: Math.round(rx), tx: Math.round(tx) }],
      gpus: info.gpus.map((g, gi) => {
        const s = gpuState[gi];
        const eng = ENGINES.map((name, k) => ({ name, util: Math.round(s.eng[k] * 10) / 10 }));
        return { id: g.id, name: g.name, util: Math.max(...eng.map((e) => e.util)), engines: eng, ded_used: Math.round(s.ded), ded_total: g.ded_total, shared_used: Math.round(s.shared), shared_total: g.shared_total };
      }),
      counts: { processes: procs.size, threads, handles },
      uptime_s: Math.floor((t - boot) / 1000),
      battery: { ...battery },
    };
  }

  // Lịch sử dạng cột: 5 phút gần nhất ở 1 s
  const hist = { t: [], cpu: [], mem: [], disks: { 0: { active: [], read: [], write: [] } }, nets: { [NET_ID]: { rx: [], tx: [] } }, gpus: { 0: [], 1: [] } };
  function pushHist(m) {
    hist.t.push(m.t);
    hist.cpu.push(m.cpu.total);
    hist.mem.push(m.mem.used);
    const d = hist.disks[0];
    d.active.push(m.disks[0].active); d.read.push(m.disks[0].read); d.write.push(m.disks[0].write);
    const n = hist.nets[NET_ID];
    n.rx.push(m.nets[0].rx); n.tx.push(m.nets[0].tx);
    hist.gpus[0].push(m.gpus[0].util); hist.gpus[1].push(m.gpus[1].util);
    const cut = m.t - 3600_000;
    if (hist.t.length > 100 && hist.t[0] < cut) {
      let k = 0;
      while (hist.t[k] < cut) k++;
      const cut0 = (a) => a.splice(0, k);
      cut0(hist.t); cut0(hist.cpu); cut0(hist.mem); cut0(d.active); cut0(d.read); cut0(d.write); cut0(n.rx); cut0(n.tx); cut0(hist.gpus[0]); cut0(hist.gpus[1]);
    }
  }
  {
    const realNow = Date.now;
    const now = realNow();
    for (let i = 300; i >= 1; i--) {
      Date.now = () => now - i * 1000;
      pushHist(sampleMetrics());
    }
    Date.now = realNow;
  }

  // ---------------- vòng tick ----------------
  let interval = 1000;
  let visible = true;
  let watch = false;
  let needReset = false;
  let timer = 0;
  let tickNo = 0;

  function procsPayload(t) {
    const reset = needReset;
    needReset = false;
    let added;
    if (reset) {
      added = [...procs.values()].map(staticOf);
      fresh.length = 0;
      gone = [];
    } else {
      added = fresh.splice(0).map((pid) => staticOf(procs.get(pid)));
    }
    const removed = reset ? [] : gone.splice(0);
    const rows = [];
    for (const p of procs.values()) rows.push([p.pid, p.cpu, Math.round(p._mem), p.disk, p.gpu, p._thr, p._hnd]);
    let windows = null;
    if (reset || tickNo % 3 === 0) {
      windows = {};
      for (const [pid, title] of windowsMap) windows[pid] = title;
    }
    return { t, reset, added, removed, rows, windows };
  }
  function staticOf(p) {
    return { pid: p.pid, ppid: p.ppid, name: p.name, exe: p.exe, desc: p.desc, publisher: p.publisher, cmd: p.cmd, start_ms: p.start_ms, session: p.session, elevated: p.elevated, user: p.user, kind: p.kind };
  }

  function tick() {
    tickNo++;
    churn();
    const m = sampleMetrics();
    pushHist(m);
    if (visible && interval > 0) {
      emit('metrics', m);
      if (watch) emit('procs', procsPayload(m.t));
    }
    schedule();
  }
  function schedule() {
    clearTimeout(timer);
    // ẩn hoặc tạm dừng: vẫn lấy mẫu mỗi 5 s cho lịch sử, không phát sự kiện
    const ms = !visible || interval === 0 ? 5000 : interval;
    timer = setTimeout(tick, ms);
  }
  schedule();

  // ---------------- icon giả ----------------
  const iconCache = new Map();
  function makeIcon(path) {
    if (!path || /^C:\\Windows\\/i.test(path)) return null;
    let h = 0;
    for (const ch of path) h = (h * 31 + ch.charCodeAt(0)) >>> 0;
    if (h % 5 === 0) return null; // một số exe không có icon
    try {
      const c = document.createElement('canvas');
      c.width = c.height = 32;
      const g = c.getContext('2d');
      g.fillStyle = `hsl(${h % 360} 65% 48%)`;
      g.beginPath();
      g.roundRect(2, 2, 28, 28, 7);
      g.fill();
      g.fillStyle = '#fff';
      g.font = 'bold 17px Segoe UI, sans-serif';
      g.textAlign = 'center';
      g.textBaseline = 'middle';
      const base = path.split('\\').pop() || '?';
      g.fillText(base[0].toUpperCase(), 16, 17);
      return c.toDataURL('image/png');
    } catch {
      return null;
    }
  }

  // ---------------- AI ----------------
  const ai = {
    provider: 'anthropic',
    include_context: false,
    tools: true,
    providers: {
      anthropic: { model: 'claude-opus-5-5', effort: 'medium', base_url: '', default_base_url: 'https://api.anthropic.com/v1', has_key: false },
      openai: { model: '', effort: '', base_url: '', default_base_url: 'https://api.openai.com/v1', has_key: false },
      custom: { model: '', effort: '', base_url: 'http://localhost:1234/v1', default_base_url: '', has_key: false },
    },
  };
  const modelCatalog = {
    anthropic: ['claude-opus-5-5', 'claude-sonnet-5-5', 'claude-haiku-5-5'],
    openai: ['gpt-4.1', 'gpt-4.1-mini', 'gpt-4o'],
    custom: ['llama3.2', 'qwen2.5', 'deepseek-r1'],
  };
  const badKeys = new Set();
  const streams = new Map(); // chat_id -> timer
  const cfgOut = () => structuredClone(ai);

  function providerSettings(provider) {
    if (!Object.hasOwn(ai.providers, provider)) throw 'Nhà cung cấp AI không hợp lệ.';
    return ai.providers[provider];
  }

  function checkConnection(provider, settings) {
    if (provider === 'custom' && !settings.base_url) throw 'Chưa nhập Base URL cho nhà cung cấp tùy chỉnh.';
    if (provider !== 'custom' && !settings.has_key) throw `Chưa có API key cho ${provider}.`;
    if (settings.base_url.includes('bad')) throw 'Không kết nối được với Base URL đã lưu.';
    if (badKeys.has(provider)) throw 'API key không hợp lệ (401).';
  }

  function reply(text) {
    const q = text.toLowerCase();
    if (q.includes('thông tin') || q.includes('thong tin') || q.includes('markdown')) {
      return 'Ngoài mức RAM của Brave, ảnh chụp hiện tại cho thấy:\n\n| Hạng mục | Thông tin |\n|---|---|\n| CPU | 12 luồng logic, đang dùng 18%, tốc độ hiển thị 2,0 GHz |\n| RAM | Đang dùng **10,7/15,7 GB** (68%), còn khoảng 5 GB |\n| GPU | NVIDIA GeForce RTX 3050 Laptop GPU (17%) và Intel UHD Graphics (13%) |\n| Ổ đĩa | Một ổ vật lý chứa `C:`, `D:`, `E:`, `F:`; đang bận 1% |\n| Thời gian hoạt động | 9 giờ 2 phút |\n| Tiến trình và dịch vụ | 282 tiến trình; 149/338 dịch vụ đang chạy |\n| Khởi động cùng Windows | 208/300 mục đang bật, trong đó 87 mục của bên thứ ba |\n\n**Điểm đáng chú ý:** tại thời điểm đo, CPU, GPU và ổ đĩa đều **không tải cao**.\n\n---\n\n### Gợi ý\n\n1. Vào trang **Khởi động**:\n   - lọc *Bên thứ ba*\n   - tắt mục không cần chạy ngay khi đăng nhập\n2. Theo dõi ~~CPU~~ **RAM** ở trang *Hiệu năng*.\n\n- [x] Đã đo RAM\n- [ ] Chưa có model CPU, dung lượng ổ đĩa, nhiệt độ\n\n> Con số 208 là số mục **được bật**, không có nghĩa là cả 208 đều chạy cùng lúc.\n\nTham khảo thêm [tài liệu Microsoft](https://learn.microsoft.com/windows).';
    }
    if (q.includes('tối ưu') || q.includes('toi uu')) {
      const chrome = [...procs.values()].find((p) => p.name === 'chrome.exe' && procs.get(p.ppid)?.name !== 'chrome.exe');
      return 'Mình đã xem danh sách khởi động, dịch vụ và tiến trình. Gợi ý:\n\n**1. Tắt bớt mục khởi động** (mở tay khi cần):\n\n```tat_khoi_dong\nDiscord | chỉ cần khi chat, ~150 MB RAM\nSteam | chỉ cần khi chơi game\nKhông có mục này | mục bịa, phải bị chặn\n```\n\n**2. Dịch vụ** không cần chạy liên tục:\n\n```dich_vu\nSysMain: thu_cong | máy dùng SSD\nDiagTrack: tat_han | gửi dữ liệu chẩn đoán\nSpooler: dung | không dùng máy in\nWinmgmt: tat_han | dịch vụ quan trọng, phải bị chặn\n```\n\n**3. Tiến trình** đang tốn RAM:\n\n```ket_thuc\n' + (chrome ? chrome.pid : 9999) + ' | Chrome gốc (kèm tiến trình con nếu chọn cả cây)\nlsass.exe | hệ thống, phải bị chặn\n```\n\nMọi thao tác chỉ chạy sau khi bạn tự chọn và xác nhận.';
    }
    if (q.includes('khởi động')) {
      return '## Nên tắt mục khởi động nào?\n\nDựa trên danh sách hiện tại, các mục **có thể tắt an toàn**:\n\n1. **Discord** – mở tay khi cần, tiết kiệm ~150 MB RAM.\n2. **Steam** – chỉ cần khi chơi game.\n3. **Docker Desktop** – khởi động rất nặng (`vmmem` chiếm ~1,9 GB).\n\nKhông nên tắt:\n\n- `SecurityHealthSystray.exe` (Windows Security)\n- Driver âm thanh / đồ hoạ như `RtkAudUService64.exe`\n\nBạn có thể tắt trong trang **Khởi động** bằng công tắc ở cột đầu.';
    }
    if (q.includes('ram')) {
      return 'Hiện tại các nhóm tốn RAM nhất là:\n\n- **vmmem** (WSL/Docker): ~1,9 GB\n- **Google Chrome** (15 tiến trình): ~1,5 GB\n- **Claude** (12 tiến trình): ~1,3 GB\n\nGợi ý: đóng bớt tab Chrome, hoặc chạy lệnh sau để tắt WSL khi không dùng:\n\n```powershell\nwsl --shutdown\n```\n\nLink tham khảo: https://learn.microsoft.com (chỉ hiển thị dạng chữ).';
    }
    return '### Vì sao máy chậm?\n\nMình thấy **CPU đang ở mức vừa phải** nhưng **bộ nhớ đã dùng khoảng 82%**, nên Windows phải dùng tệp hoán đổi (`pagefile.sys`) nhiều hơn.\n\nNguyên nhân chính:\n\n1. `vmmem` (Docker/WSL) giữ ~1,9 GB RAM.\n2. Chrome + Claude + VS Code cộng lại hơn 3 GB.\n3. Có **27 mục khởi động** đang bật.\n\nBạn nên:\n\n- Tắt bớt mục khởi động không cần thiết\n- Đóng ứng dụng nền như *Discord*, *Steam*\n- Khởi động lại máy nếu đã chạy lâu (uptime hiện tại hơn 4 giờ)\n\n<b>Thẻ HTML này phải hiện dạng chữ</b>, không được thực thi.';
  }

  function mockToolCalls(text) {
    const q = (text || '').toLowerCase();
    if (q.includes('thư mục') || q.includes('.cache') || q.includes('bên trong')) return [['scan_folder', 'quét thư mục', { path: 'C:\\Users\\ADMIN\\.cache', depth: 2 }], ['folder_files', 'tệp trong thư mục', { path: 'C:\\Users\\ADMIN\\.cache\\codex-runtimes', limit: 10 }]];
    if (q.includes('ram') || q.includes('webview')) return [['list_processes', 'danh sách tiến trình', { group: true, sort: 'memory' }], ['process_tree', 'cây tiến trình', { pid: 12860 }]];
    if (q.includes('chậm') || q.includes('cpu')) return [['system_overview', 'tổng quan máy', {}], ['startup_items', 'mục khởi động', { enabled_only: true }]];
    if (q.includes('khởi động')) return [['startup_items', 'mục khởi động', { enabled_only: true }]];
    if (q.includes('dịch vụ')) return [['services', 'dịch vụ', { running_only: true }]];
    if (q.includes('thông tin') || q.includes('cấu hình')) return [['hardware', 'cấu hình phần cứng', {}]];
    return [];
  }

  function stream(chat_id, text, provider, model, disk, ports, cleanup) {
    const body = (disk && diskReply?.(disk.focus)) || (ports && portsReply?.()) || (cleanup && cleanupReply?.()) || reply(text);
    const words = (`Phản hồi mô phỏng từ **${provider}** · ${model}.\n\n` + body).split(/(\s+)/);
    let i = 0;
    const out = { chat_id };
    const step = () => {
      if (i >= words.length) {
        streams.delete(chat_id);
        emit('ai', { ...out, kind: 'done', stop_reason: provider === 'anthropic' ? 'end_turn' : 'stop', usage: { input_tokens: 1180 + Math.floor(rand() * 400), output_tokens: Math.floor(words.length / 1.5) } });
        return;
      }
      const n = 1 + Math.floor(rand() * 4);
      const chunk = words.slice(i, i + n).join('');
      i += n;
      emit('ai', { ...out, kind: 'delta', text: chunk });
      streams.set(chat_id, setTimeout(step, 30 + rand() * 50));
    };
    if (provider !== 'openai') emit('ai', { ...out, kind: 'thinking' });
    // Trợ lý tự tra cứu (tool chỉ đọc) trước khi trả lời – giống vòng lặp tool-use của backend
    const calls = ai.tools && !disk && !ports && !cleanup ? mockToolCalls(text) : [];
    let delay = provider === 'openai' ? 120 : 900 + rand() * 600;
    calls.forEach(([name, label, args], k) => {
      const id = `tool_${chat_id}_${k}`;
      setTimeout(() => emit('ai', { ...out, kind: 'tool', id, name, label, args, phase: 'start', ok: false, chars: 0 }), delay);
      delay += 350 + rand() * 300;
      setTimeout(() => emit('ai', { ...out, kind: 'tool', id, name, label, args, phase: 'done', ok: true, chars: 800 + Math.floor(rand() * 2000) }), delay);
      delay += 80;
    });
    streams.set(chat_id, setTimeout(step, delay));
  }

  return {
    async system_info() { await sleep(40); return structuredClone(info); },
    async metrics_history() { await sleep(60); return structuredClone(hist); },
    async set_interval({ ms }) {
      if (![0, 500, 1000, 2000, 5000].includes(ms)) throw 'Chu kỳ không hợp lệ';
      interval = ms;
      schedule();
      return null;
    },
    async set_visible({ visible: v }) { visible = !!v; schedule(); return null; },
    async set_locale() { return null; },
    // tự cập nhật: giả lập có bản 0.3.1 và tiến trình tải ~9.8 MB (xem giao diện trong trình duyệt)
    async update_info() { return { enabled: true, version: '0.3.0', repo: 'mock/mock' }; },
    async update_check() {
      await sleep(600);
      return { version: '0.3.1', current: '0.3.0', notes: '## Có gì mới\n- Sửa lỗi A\n- Thêm tính năng B', date_ms: Date.now() - 864e5 };
    },
    async update_install() {
      const total = 9.8 * 1024 * 1024;
      for (let got = 0; got < total; got += total / 20) {
        await sleep(150);
        emit('update_progress', { stage: 'download', got, total });
      }
      emit('update_progress', { stage: 'install', got: 0, total: 0 });
      await sleep(1500);
      throw 'Bản xem thử không cài được bản mới.';
    },
    async set_idle_interval({ ms }) {
      if (![0, 5000, 15000, 60000].includes(ms)) throw 'Chu kỳ khi ẩn không hợp lệ';
      return null;
    },
    async relaunch_admin() { await sleep(200); throw 'Bản xem thử không mở lại được bằng quyền Admin.'; },
    async set_watch({ procs: on }) {
      if (on && !watch) needReset = true;
      watch = !!on;
      return null;
    },
    async proc_lookup({ targets }) {
      await sleep(40);
      const out = [];
      const crit = ['system', 'registry', 'memory compression', 'csrss.exe', 'wininit.exe', 'winlogon.exe', 'smss.exe', 'services.exe', 'lsass.exe'];
      for (const t of targets) {
        const key = String(t).trim().replace(/^pid\s*/i, '');
        const pid = /^\d+$/.test(key) ? Number(key) : 0;
        const found = [...procs.values()].filter((p) => (pid ? p.pid === pid : p.name.toLowerCase() === key.toLowerCase())).slice(0, 40);
        if (!found.length) out.push({ target: t, pid, name: '', exe: '', desc: '', mem: 0, children: 0, blocked: 'Không tìm thấy (đã thoát?)' });
        for (const p of found) {
          const n = p.name.toLowerCase();
          const blocked = p.pid === 4 || crit.includes(n) ? 'Tiến trình hệ thống quan trọng của Windows' : n === 'svchost.exe' ? 'Chứa dịch vụ Windows – hãy dừng dịch vụ ở trang Dịch vụ' : undefined;
          const children = [...procs.values()].filter((c) => c.ppid === p.pid && c.pid !== p.pid).length;
          out.push({ target: t, pid: p.pid, name: p.name, exe: p.exe, desc: p.desc, mem: Math.round(p._mem), children, ...(blocked ? { blocked } : {}) });
        }
      }
      return out;
    },
    async proc_kill({ pids, tree }) {
      await sleep(80 + rand() * 200);
      const res = [];
      const kill = (pid) => {
        const p = procs.get(pid);
        if (!p) return { pid, ok: false, error: 'Tiến trình không còn tồn tại.' };
        if (pid === 4 || pid === 120 || ['csrss.exe', 'wininit.exe', 'services.exe', 'lsass.exe', 'smss.exe', 'Memory Compression'].includes(p.name)) {
          return { pid, ok: false, error: 'Truy cập bị từ chối (mã 5).' };
        }
        if (rand() < 0.05) return { pid, ok: false, error: 'Tiến trình không thoát sau 3 giây.' };
        removeProc(pid);
        return { pid, ok: true };
      };
      for (const pid of pids) {
        if (tree) {
          const order = [];
          const walk = (q) => {
            for (const c of procs.values()) if (c.ppid === q && c.pid !== q) walk(c.pid);
            order.push(q);
          };
          walk(pid);
          for (const q of order) res.push(kill(q));
        } else res.push(kill(pid));
      }
      return res;
    },
    async proc_restart({ pid }) {
      await sleep(300 + rand() * 400);
      const p = procs.get(pid);
      if (!p) return { ok: false, error: 'Tiến trình không còn tồn tại.' };
      if (!p.exe || !p.cmd) return { ok: false, error: 'Không đọc được dòng lệnh (truy cập bị từ chối).' };
      if (p.kind === 'windows' && p.session === 0) return { ok: false, error: 'Không khởi động lại tiến trình hệ thống.' };
      const title = windowsMap.get(pid);
      removeProc(pid);
      const n = add({ ...staticOf(p), pid: undefined, start_ms: Date.now(), window: title, cpu: p._cpu, mem: p._mem * 0.6, gpu: p._gpu, thr: p._thr, hnd: p._hnd });
      for (const c of procs.values()) if (c.ppid === pid) c.ppid = n.pid;
      return { ok: true, pid: n.pid };
    },
    async proc_open_location({ path }) { await sleep(50); console.info('[mock] proc_open_location', path); return null; },
    async proc_properties({ path }) { await sleep(50); console.info('[mock] proc_properties', path); return null; },
    async proc_set_priority({ pid, level }) {
      await sleep(60);
      const p = procs.get(pid);
      if (!p) return { ok: false, error: 'Tiến trình không còn tồn tại.' };
      if (p.session === 0 && p.user === 'SYSTEM' && rand() < 0.7) return { ok: false, error: 'Truy cập bị từ chối (mã 5).' };
      console.info('[mock] priority', pid, level);
      return { ok: true };
    },
    async get_icons({ paths }) {
      await sleep(40 + rand() * 60);
      const out = {};
      for (const p of paths) {
        if (!iconCache.has(p)) iconCache.set(p, makeIcon(p));
        out[p] = iconCache.get(p);
      }
      return out;
    },
    async ai_get_config() { await sleep(30); return cfgOut(); },
    async ai_set_config({ provider, include_context, tools, target, model, effort, base_url, api_key }) {
      await sleep(120);
      provider ??= ai.provider;
      target ??= provider;
      providerSettings(provider);
      const settings = providerSettings(target);
      if (effort !== undefined && !['low', 'medium', 'high', 'xhigh', 'max'].includes(effort)) throw 'Mức suy nghĩ không hợp lệ.';
      if (base_url !== undefined) {
        base_url = base_url.trim();
        if (base_url) {
          try {
            const url = new URL(base_url);
            if (!['https:', 'http:'].includes(url.protocol)) throw new Error();
          } catch {
            throw 'Base URL không hợp lệ. Hãy nhập địa chỉ HTTP hoặc HTTPS.';
          }
        }
      }
      ai.provider = provider;
      if (include_context !== undefined) ai.include_context = include_context;
      if (tools !== undefined) ai.tools = tools;
      if (model !== undefined) settings.model = model.trim();
      if (effort !== undefined && target === 'anthropic') settings.effort = effort;
      if (base_url !== undefined) settings.base_url = base_url;
      if (api_key !== undefined) {
        const key = api_key.trim();
        settings.has_key = !!key;
        if (target !== 'custom' && key.startsWith('bad')) badKeys.add(target);
        else badKeys.delete(target);
      }
      return cfgOut();
    },
    async ai_list_models({ provider }) {
      await sleep(450);
      checkConnection(provider, providerSettings(provider));
      return [...modelCatalog[provider]].sort();
    },
    async ai_send({ chat_id, text, disk, ports, cleanup }) {
      const { provider } = ai;
      const settings = providerSettings(provider);
      const { model } = settings;
      if (!model) throw `Chưa chọn model cho ${provider}.`;
      if (provider !== 'custom' && !settings.has_key) throw `Chưa có API key cho ${provider}.`;
      let error;
      try {
        checkConnection(provider, settings);
      } catch (e) {
        error = e;
      }
      streams.set(chat_id, setTimeout(() => {
        if (error) {
          streams.delete(chat_id);
          emit('ai', { chat_id, kind: 'error', message: error });
        } else {
          stream(chat_id, text, provider, model, disk, ports, cleanup);
        }
      }, 60));
      return null;
    },
    async ai_cancel({ chat_id }) {
      clearTimeout(streams.get(chat_id));
      if (streams.delete(chat_id)) emit('ai', { chat_id, kind: 'done', stop_reason: 'cancelled', usage: null });
      return null;
    },
    async ai_reset({ chat_id }) { clearTimeout(streams.get(chat_id)); streams.delete(chat_id); return null; },
  };
}
