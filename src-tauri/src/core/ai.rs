use std::collections::{BTreeMap, HashMap};
use std::env;
use crate::tr;
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use ureq::http::{HeaderValue, Response, Uri};
use ureq::typestate::WithBody;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};

const DEFAULT_MODEL: &str = "claude-opus-5-5";
const DEFAULT_EFFORT: &str = "medium";
const ANTHROPIC_VERSION: &str = "2026-10-21";
const SYSTEM: &str =
    "Bạn là trợ lý trong ứng dụng quản lý khởi động, dịch vụ, tiến trình và hiệu năng Windows. \
Trả lời ngắn gọn, cụ thể bằng ngôn ngữ người dùng, mặc định là tiếng Việt. \
Cảnh báo trước khi đề xuất tắt dịch vụ quan trọng hoặc kết thúc tiến trình hệ thống. \
Bạn chỉ hướng dẫn người dùng thao tác trong ứng dụng và không thể tự thao tác. \
Khi có khối <disk_scan> (kết quả quét ổ cứng của trang Ổ cứng), hãy giải thích thư mục nào chiếm chỗ và \
cái gì xoá an toàn; chỉ dùng đường dẫn có trong dữ liệu, không bịa đường dẫn. \
Muốn đề xuất xoá thì thêm MỘT khối mã ngôn ngữ `xoa`, mỗi dòng `đường dẫn đầy đủ | lý do ngắn`, ví dụ:\n\
```xoa\nC:\\Users\\A\\AppData\\Local\\Temp | tệp tạm, Windows tự tạo lại\n```\n\
Ứng dụng sẽ hiện danh sách để người dùng tự chọn và xác nhận; bạn không tự xoá được. \
Không đề xuất xoá thư mục hệ thống, thư mục chương trình hay dữ liệu cá nhân (Documents, Pictures, Desktop…); \
với thư mục có thể chứa dữ liệu quan trọng thì nói rõ rủi ro thay vì đưa vào khối `xoa`. \
Khi có khối <ports> (các cổng đang nghe trên máy, từ trang Cổng), hãy chỉ ra cổng đáng chú ý: mở cho mọi mạng mà \
không cần thiết, chương trình lạ, công cụ lập trình quên tắt. Cổng của Windows (System, svchost, lsass, wininit) thì \
giải thích chứ không đề xuất đóng. Muốn đề xuất đóng cổng (kết thúc chương trình đang giữ cổng) thì thêm MỘT khối mã \
ngôn ngữ `dong_cong`, mỗi dòng `số cổng | lý do ngắn`, ví dụ:\n```dong_cong\n3000 | máy chủ dev Node còn chạy, mở cho mọi mạng\n```\n\
Ứng dụng sẽ hiện danh sách để người dùng tự chọn; chỉ dùng số cổng có trong dữ liệu. \
Khi có khối <cleanup> (kết quả quét của trang Dọn dẹp: tệp tạm, báo cáo lỗi, cache ứng dụng…, mỗi mục có mã trong \
ngoặc vuông), hãy nói mục nào lớn, mục nào dọn an toàn, mục nào nên cân nhắc (cache công cụ lập trình hay mô hình AI phải \
tải lại, Thùng rác là mất hẳn, cập nhật Windows đang chạy). Dọn ở đây là xoá vĩnh viễn nội dung bên trong, tệp đang dùng \
tự được bỏ qua. Muốn đề xuất dọn thì thêm MỘT khối mã ngôn ngữ `don_dep`, mỗi dòng `mã mục | lý do ngắn`, ví dụ:\n\
```don_dep\nuser_temp | tệp tạm, an toàn\ncache:google chrome | cache trình duyệt 1.2G, tự tải lại\n```\n\
Chỉ dùng mã có trong dữ liệu; ứng dụng sẽ hiện danh sách để người dùng tự chọn. \
Muốn đề xuất TẮT mục khởi động cùng Windows thì thêm MỘT khối mã ngôn ngữ `tat_khoi_dong`, mỗi dòng \
`tên mục đúng như trong danh sách khởi động | lý do ngắn`, ví dụ:\n```tat_khoi_dong\nDiscord | không cần mở ngay khi bật máy\n```\n\
Muốn đề xuất đổi dịch vụ thì thêm MỘT khối mã ngôn ngữ `dich_vu`, mỗi dòng `tên dịch vụ: thao tác | lý do ngắn`, thao tác \
là một trong `thu_cong` (Thủ công), `tat_han` (Tắt hẳn), `tu_dong`, `tu_dong_tre`, `dung` (dừng ngay), `chay`, ví dụ:\n\
```dich_vu\nSysMain: thu_cong | máy SSD, không cần nạp trước ứng dụng\n```\n\
Không đề xuất dịch vụ quan trọng của Windows; ưu tiên `thu_cong` hơn `tat_han` khi không chắc. \
Muốn đề xuất kết thúc tiến trình thì thêm MỘT khối mã ngôn ngữ `ket_thuc`, mỗi dòng `PID hoặc tên exe | lý do ngắn` \
(tên exe = mọi tiến trình cùng tên; ứng dụng nhiều tiến trình như trình duyệt, Zalo, Claude thì ghi TÊN EXE MỘT LẦN, \
không liệt kê từng PID; PID chỉ dùng khi muốn đóng riêng một tiến trình), ví dụ:\n```ket_thuc\n12345 | node.exe treo, ăn 2 GB RAM\n```\n\
Không đề xuất tiến trình hệ thống (System, csrss, lsass, svchost…) và nhắc dữ liệu chưa lưu sẽ mất. \
Muốn đề xuất sửa biến môi trường thì thêm MỘT khối mã ngôn ngữ `bien_moi_truong`, mỗi dòng `phạm_vi TÊN thao_tác | lý do ngắn`, \
phạm vi là `user` (người dùng) hoặc `system` (hệ thống), thao tác là `-= mục` (bỏ một mục khỏi biến danh sách như Path), \
`+= mục` (thêm mục vào cuối), `= giá trị` (đặt cả giá trị) hoặc `xoa` (xoá biến), ví dụ:\n\
```bien_moi_truong\nuser Path -= C:\\old\\bin | thư mục không còn tồn tại\nuser JAVA_HOME = C:\\Program Files\\Java\\jdk-21 | trỏ tới JDK đang cài\n```\n\
Với Path ưu tiên `-=` / `+=` từng mục, không viết lại cả giá trị. Không đề xuất xoá Path, ComSpec, windir hay biến Windows cần; \
không bao giờ hỏi hay nhắc giá trị bị che (biến bí mật). \
Mọi khối đều chỉ là gợi ý: ứng dụng kiểm tra lại, người dùng tự chọn và xác nhận.";

/// Thêm vào lời dặn khi bật tool (tra cứu máy).
const SYSTEM_TOOLS: &str = "Bạn có các công cụ CHỈ ĐỌC để tự tra cứu máy này (tiến trình, cây tiến trình, khởi động, \
dịch vụ, cổng, ổ đĩa, dọn dẹp, phần cứng, hiệu năng, biến môi trường). Khi cần số liệu, hãy gọi công cụ thay vì đoán hay bảo người dùng gửi \
ảnh / danh sách; có thể gọi nhiều công cụ trong một lượt. Ví dụ: tổng RAM của một ứng dụng kèm các tiến trình con \
(WebView2, trình duyệt) → process_tree; một tiến trình là gì, có an toàn không → process_report; trong một thư mục \
có gì, cái gì chiếm chỗ (vd. một mục của trang Dọn dẹp) → scan_folder rồi folder_files, TỰ quét chứ không bảo người dùng \
quét ở trang Ổ cứng. Gợi ý gỡ ứng dụng / tắt khởi động / dừng dịch vụ thì xét cả \"dùng lần cuối\" / \"chạy lần cuối\" \
(installed_apps với sort=last_used, startup_items, services) và ngày sửa của thư mục (app_folders, leftovers): lâu không \
dùng + nặng = đáng gỡ; nhưng đó chỉ là dấu hiệu (chương trình chạy nền tự động cũng được tính), nói rõ khi dữ liệu \
\"không rõ\". Không công cụ nào thay đổi \
được máy: muốn thao tác thì dùng các khối đề xuất ở trên để người dùng tự xác nhận trong ứng dụng. Đừng kể lại toàn bộ \
kết quả công cụ, chỉ nêu điều liên quan.";

static CONFIG_LOCK: Mutex<()> = Mutex::new(());
static TEMP_FILE_ID: AtomicU64 = AtomicU64::new(0);
static HTTP: LazyLock<ureq::Agent> = LazyLock::new(|| {
    ureq::Agent::config_builder()
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_per_read(Some(Duration::from_secs(120)))
        .timeout_per_write(Some(Duration::from_secs(120)))
        .build()
        .into()
});

#[derive(Serialize, Clone)]
pub struct AiConfigView {
    pub provider: Provider,
    pub include_context: bool,
    pub tools: bool,
    pub providers: BTreeMap<Provider, ProviderSettingsView>,
}

#[derive(Serialize, Clone)]
pub struct ProviderSettingsView {
    pub model: String,
    pub effort: String,
    pub base_url: String,
    pub default_base_url: &'static str,
    pub has_key: bool,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct AiConfigUpdate {
    pub provider: Option<String>,
    pub include_context: Option<bool>,
    pub tools: Option<bool>,
    pub target: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    #[default]
    Anthropic,
    Openai,
    Custom,
}

impl Provider {
    fn parse(id: &str) -> Result<Self, String> {
        match id {
            "anthropic" => Ok(Self::Anthropic),
            "openai" => Ok(Self::Openai),
            "custom" => Ok(Self::Custom),
            _ => Err(tr!("ai.err.bad_provider")),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Anthropic => "Anthropic",
            Self::Openai => "OpenAI",
            Self::Custom => "Custom",
        }
    }

    fn default_base_url(self) -> &'static str {
        match self {
            Self::Anthropic => "https://api.anthropic.com/v1",
            Self::Openai => "https://api.openai.com/v1",
            Self::Custom => "",
        }
    }

    fn environment_key(self) -> Option<String> {
        if self == Self::Anthropic {
            env::var("ANTHROPIC_API_KEY")
                .ok()
                .filter(|key| !key.trim().is_empty())
        } else {
            None
        }
    }

    fn missing_key(self) -> String {
        let name = if self == Self::Anthropic {
            "Claude"
        } else {
            self.name()
        };
        tr!("ai.err.missing_key", name = name)
    }
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AiConfig {
    provider: Provider,
    include_context: bool,
    /// Tắt việc cho trợ lý tự tra cứu máy bằng tool (mặc định bật nên lưu dạng "không").
    no_tools: bool,
    providers: Providers,
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Providers {
    anthropic: ProviderSettings,
    openai: ProviderSettings,
    custom: ProviderSettings,
}

impl Default for Providers {
    fn default() -> Self {
        Self {
            anthropic: ProviderSettings::anthropic(),
            openai: ProviderSettings::default(),
            custom: ProviderSettings::default(),
        }
    }
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
struct ProviderSettings {
    model: String,
    effort: String,
    base_url: String,
    key_dpapi: String,
}

impl ProviderSettings {
    fn anthropic() -> Self {
        Self {
            model: DEFAULT_MODEL.into(),
            effort: DEFAULT_EFFORT.into(),
            ..Self::default()
        }
    }

    fn key(&self, provider: Provider) -> Result<Option<String>, String> {
        if self.key_dpapi.is_empty() {
            return Ok(provider.environment_key());
        }
        let mut bytes = base64_decode(&self.key_dpapi).ok_or_else(|| tr!("ai.err.saved_key_invalid"))?;
        let key = String::from_utf8(dpapi(&mut bytes, false)?)
            .map_err(|_| tr!("ai.err.saved_key_invalid"))?;
        if key.is_empty() {
            return Err(tr!("ai.err.saved_key_invalid"));
        }
        Ok(Some(key))
    }

    fn view(&self, provider: Provider) -> ProviderSettingsView {
        ProviderSettingsView {
            model: self.model.clone(),
            effort: self.effort.clone(),
            base_url: self.base_url.clone(),
            default_base_url: provider.default_base_url(),
            has_key: self
                .key(provider)
                .is_ok_and(|key| key.is_some_and(|key| api_key_header(key.trim()).is_ok())),
        }
    }
}

impl AiConfig {
    pub fn load(path: &Path) -> Result<Self, String> {
        Self::read(path).map(|(config, _)| config)
    }

    fn read(path: &Path) -> Result<(Self, bool), String> {
        let invalid_config = || tr!("ai.err.invalid_config");
        let data = match fs::read(path) {
            Ok(data) => data,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok((Self::default(), false)),
            Err(_) => return Err(tr!("ai.err.read_config")),
        };
        let data: Value = serde_json::from_slice(&data).map_err(|_| invalid_config())?;
        if !data.is_object() {
            return Err(invalid_config());
        }
        let migrated = data.get("providers").is_none();
        let mut config = Self::deserialize(&data).map_err(|_| invalid_config())?;
        if migrated {
            let mut legacy: ProviderSettings =
                serde_json::from_value(data).map_err(|_| invalid_config())?;
            if legacy.model.trim().is_empty() {
                legacy.model = DEFAULT_MODEL.into();
            }
            config.providers.anthropic = legacy;
        }
        if !valid_effort(&config.providers.anthropic.effort) {
            config.providers.anthropic.effort = DEFAULT_EFFORT.into();
        }
        config.providers.openai.effort.clear();
        config.providers.custom.effort.clear();
        Ok((config, migrated))
    }

    fn settings(&self, provider: Provider) -> &ProviderSettings {
        match provider {
            Provider::Anthropic => &self.providers.anthropic,
            Provider::Openai => &self.providers.openai,
            Provider::Custom => &self.providers.custom,
        }
    }

    fn update(&mut self, update: AiConfigUpdate) -> Result<(), String> {
        if let Some(provider) = update.provider {
            self.provider = Provider::parse(&provider)?;
        }
        if let Some(include_context) = update.include_context {
            self.include_context = include_context;
        }
        if let Some(tools) = update.tools {
            self.no_tools = !tools;
        }
        let target = update
            .target
            .as_deref()
            .map(Provider::parse)
            .transpose()?
            .unwrap_or(self.provider);
        let settings = match target {
            Provider::Anthropic => &mut self.providers.anthropic,
            Provider::Openai => &mut self.providers.openai,
            Provider::Custom => &mut self.providers.custom,
        };
        if let Some(model) = update.model {
            settings.model = model.trim().into();
        }
        if let Some(effort) = update.effort {
            if !(valid_effort(&effort) || (target != Provider::Anthropic && effort.is_empty())) {
                return Err(tr!("ai.err.bad_effort"));
            }
            if target == Provider::Anthropic {
                settings.effort = effort;
            }
        }
        if let Some(base_url) = update.base_url {
            let base_url = base_url.trim();
            if !base_url.is_empty() || target == Provider::Custom {
                BaseUrl::parse(base_url, target)?;
            }
            settings.base_url = base_url.trim_end_matches('/').into();
        }
        if let Some(key) = update.api_key {
            settings.key_dpapi = if key.trim().is_empty() {
                String::new()
            } else {
                api_key_header(key.trim())?;
                let mut bytes = key.trim().as_bytes().to_vec();
                let protected = dpapi(&mut bytes, true);
                bytes.fill(0);
                base64_encode(&protected?)
            };
        }
        Ok(())
    }

    fn save(&self, path: &Path) -> Result<(), String> {
        let data = serde_json::to_vec_pretty(self).map_err(|_| tr!("ai.err.make_config"))?;
        let directory = path.parent().ok_or_else(|| tr!("ai.err.no_config_dir"))?;
        fs::create_dir_all(directory).map_err(|_| tr!("ai.err.make_config_dir"))?;
        let file_id = TEMP_FILE_ID.fetch_add(1, Ordering::Relaxed);
        let tmp = directory.join(format!("ai.json.{}.{file_id}.tmp", std::process::id()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(|_| tr!("ai.err.make_config_file"))?;
        let written = file.write_all(&data).and_then(|()| file.sync_all());
        drop(file);
        if written.and_then(|()| fs::rename(&tmp, path)).is_err() {
            let _ = fs::remove_file(tmp);
            return Err(tr!("ai.err.save_config"));
        }
        Ok(())
    }

    fn view(&self) -> AiConfigView {
        AiConfigView {
            provider: self.provider,
            include_context: self.include_context,
            tools: !self.no_tools,
            providers: [Provider::Anthropic, Provider::Openai, Provider::Custom]
                .into_iter()
                .map(|provider| (provider, self.settings(provider).view(provider)))
                .collect(),
        }
    }

    pub fn prepare_send(&self) -> Result<SendConfig, String> {
        let provider = self.provider;
        let settings = self.settings(provider);
        if settings.model.trim().is_empty() {
            return Err(tr!("ai.err.no_model", provider = provider.name()));
        }
        let base = BaseUrl::for_provider(settings, provider)?;
        if provider != Provider::Custom
            && settings.key_dpapi.is_empty()
            && provider.environment_key().is_none()
        {
            return Err(provider.missing_key());
        }
        Ok(SendConfig {
            provider,
            base,
            settings: settings.clone(),
            include_context: self.include_context,
            tools: !self.no_tools,
        })
    }

    pub fn list_models(&self, provider: &str) -> Result<Vec<String>, String> {
        let provider = Provider::parse(provider)?;
        let settings = self.settings(provider);
        let base = BaseUrl::for_provider(settings, provider)?;
        let client = Client::new(provider, &base, settings)?;
        client.list_models()
    }
}

fn valid_effort(effort: &str) -> bool {
    matches!(effort, "low" | "medium" | "high" | "xhigh" | "max")
}

fn config_path() -> Option<PathBuf> {
    env::var_os("LOCALAPPDATA")
        .filter(|dir| !dir.is_empty())
        .map(|dir| PathBuf::from(dir).join("fomeg-windows-app").join("ai.json"))
}

pub fn get_config() -> AiConfigView {
    config_path()
        .and_then(|path| get_config_at(&path).ok())
        .unwrap_or_default()
        .view()
}

fn get_config_at(path: &Path) -> Result<AiConfig, String> {
    let _guard = CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (config, migrated) = AiConfig::read(path)?;
    if migrated {
        let _ = config.save(path);
    }
    Ok(config)
}

pub fn include_context_enabled() -> bool {
    #[derive(Deserialize)]
    struct ContextSetting {
        #[serde(default)]
        include_context: bool,
    }

    config_path()
        .and_then(|path| fs::read(path).ok())
        .and_then(|data| serde_json::from_slice::<ContextSetting>(&data).ok())
        .is_some_and(|config| config.include_context)
}

pub fn set_config(u: AiConfigUpdate) -> Result<AiConfigView, String> {
    let path = config_path().ok_or_else(|| tr!("ai.err.no_localappdata"))?;
    set_config_at(&path, u)
}

pub fn set_config_at(path: &Path, u: AiConfigUpdate) -> Result<AiConfigView, String> {
    let _guard = CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut config = AiConfig::load(path)?;
    config.update(u)?;
    config.save(path)?;
    Ok(config.view())
}

pub fn load_config() -> Result<AiConfig, String> {
    match config_path() {
        Some(path) => AiConfig::load(&path),
        None => Ok(AiConfig::default()),
    }
}

pub fn list_models(provider: &str) -> Result<Vec<String>, String> {
    load_config()?.list_models(provider)
}

fn dpapi(bytes: &mut [u8], protect: bool) -> Result<Vec<u8>, String> {
    let input = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(bytes.len()).map_err(|_| tr!("ai.err.key_too_long"))?,
        pbData: bytes.as_mut_ptr(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    // Windows chỉ đọc vùng input và cấp phát vùng output bằng LocalAlloc.
    let result = unsafe {
        if protect {
            CryptProtectData(
                &input,
                PCWSTR::null(),
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    };
    let mut data = Vec::new();
    if !output.pbData.is_null() {
        // Vùng nhớ do DPAPI cấp phát vẫn hợp lệ cho đến khi gọi LocalFree.
        unsafe {
            if result.is_ok() {
                let output_bytes =
                    std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize);
                data.extend_from_slice(output_bytes);
                output_bytes.fill(0);
            }
            let _ = LocalFree(Some(HLOCAL(output.pbData.cast())));
        }
    }
    result.map_err(|_| {
        if protect {
            tr!("ai.err.dpapi_protect")
        } else {
            tr!("ai.err.dpapi_unprotect")
        }
    })?;
    Ok(data)
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let a = chunk[0];
        let b = chunk.get(1).copied().unwrap_or(0);
        let c = chunk.get(2).copied().unwrap_or(0);
        encoded.push(BASE64[(a >> 2) as usize] as char);
        encoded.push(BASE64[((a & 3) << 4 | b >> 4) as usize] as char);
        encoded.push(if chunk.len() >= 2 {
            BASE64[((b & 15) << 2 | c >> 6) as usize] as char
        } else {
            '='
        });
        encoded.push(if chunk.len() == 3 {
            BASE64[(c & 63) as usize] as char
        } else {
            '='
        });
    }
    encoded
}

fn base64_digit(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

fn base64_decode(encoded: &str) -> Option<Vec<u8>> {
    let (groups, remainder) = encoded.as_bytes().as_chunks::<4>();
    if !remainder.is_empty() {
        return None;
    }
    let mut decoded = Vec::with_capacity(groups.len() * 3);
    let mut groups = groups.iter().peekable();
    while let Some(&[a, b, c, d]) = groups.next() {
        let a = base64_digit(a)?;
        let b = base64_digit(b)?;
        decoded.push(a << 2 | b >> 4);
        if c == b'=' {
            return (d == b'=' && b & 15 == 0 && groups.peek().is_none()).then_some(decoded);
        }
        let c = base64_digit(c)?;
        decoded.push(b << 4 | c >> 2);
        if d == b'=' {
            return (c & 3 == 0 && groups.peek().is_none()).then_some(decoded);
        }
        decoded.push(c << 6 | base64_digit(d)?);
    }
    Some(decoded)
}

#[derive(Serialize, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AiEvent {
    Delta {
        chat_id: String,
        text: String,
    },
    Thinking {
        chat_id: String,
    },
    Done {
        chat_id: String,
        stop_reason: String,
        usage: Value,
    },
    Error {
        chat_id: String,
        message: String,
    },
    /// Trợ lý gọi một tool chỉ đọc: `phase` = "start" (đang tra cứu) | "done" (xong, `ok`, `chars` ký tự kết quả).
    Tool {
        chat_id: String,
        id: String,
        name: String,
        label: String,
        args: Value,
        phase: &'static str,
        ok: bool,
        chars: usize,
    },
}

/// Bộ tool cho một lượt: mô tả (dạng MCP: name, description, inputSchema) + hàm chạy.
pub struct ToolKit<'a> {
    pub defs: Vec<Value>,
    pub run: &'a mut dyn FnMut(&str, &Value) -> Result<String, String>,
    pub label: fn(&str) -> &'static str,
}

/// Giới hạn vòng lặp tool-use (như RustWTM agent.rs): số vòng, thời gian, độ dài kết quả tool gửi lại.
const TOOL_ROUNDS: usize = 8;
const TOOL_DEADLINE: Duration = Duration::from_secs(180);
const TOOL_TRANSCRIPT: usize = 120_000;

struct ToolCall {
    id: String,
    name: String,
    input: Value,
}

#[derive(Default)]
pub struct Chats {
    chats: Mutex<HashMap<String, Chat>>,
}

#[derive(Default)]
struct Chat {
    history: Vec<HistoryTurn>,
    reset_in_flight: Option<Arc<AtomicBool>>,
}

struct Turn<'a> {
    chats: &'a Chats,
    chat_id: &'a str,
    reset: Arc<AtomicBool>,
    history: Vec<HistoryTurn>,
}

impl Drop for Turn<'_> {
    fn drop(&mut self) {
        let mut chats = self.chats.chats.lock().unwrap_or_else(|e| e.into_inner());
        if self.history.is_empty() || self.reset.load(Ordering::Relaxed) {
            chats.remove(self.chat_id);
        } else if let Some(chat) = chats.get_mut(self.chat_id) {
            chat.history = std::mem::take(&mut self.history);
            chat.reset_in_flight = None;
        }
    }
}

impl Chats {
    /// Gọi trên luồng nền. Kiểm tra yêu cầu hủy giữa các dòng SSE.
    #[allow(clippy::too_many_arguments)] // ngữ cảnh máy và kết quả quét ổ đĩa đi riêng: một cái theo cài đặt, một cái theo trang
    pub fn send(
        &self,
        config: SendConfig,
        chat_id: &str,
        text: &str,
        context: Option<String>,
        blocks: &[(&str, String)],
        tools: Option<ToolKit<'_>>,
        cancel: &AtomicBool,
        emit: &dyn Fn(AiEvent),
    ) {
        let mut user = UserMessage::new(text, context.filter(|_| config.include_context));
        for (tag, body) in blocks {
            user = user.with_block(tag, body);
        }
        let mut tools = tools.filter(|_| config.tools);
        self.send_with(
            chat_id,
            user,
            cancel,
            emit,
            |history, user, cancelled, emit| {
                let client = Client::new(config.provider, &config.base, &config.settings)?;
                client.stream_response(&config.settings, history, user, tools.as_mut(), cancelled, emit)
            },
        );
    }

    pub fn reset(&self, chat_id: &str) {
        let mut chats = self.chats.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(reset) = chats
            .get(chat_id)
            .and_then(|chat| chat.reset_in_flight.as_ref())
        {
            reset.store(true, Ordering::Relaxed);
        } else {
            let removed = chats.remove(chat_id);
            drop(chats);
            drop(removed);
        }
    }

    fn begin<'a>(&'a self, chat_id: &'a str) -> Option<Turn<'a>> {
        let mut chats = self.chats.lock().unwrap_or_else(|e| e.into_inner());
        let chat = chats.entry(chat_id.to_owned()).or_default();
        if chat.reset_in_flight.is_some() {
            return None;
        }
        let reset = Arc::new(AtomicBool::new(false));
        chat.reset_in_flight = Some(Arc::clone(&reset));
        // Chuyển lịch sử sang lượt hiện tại để tuần tự hóa JSON ngoài khóa.
        Some(Turn {
            chats: self,
            chat_id,
            reset,
            history: std::mem::take(&mut chat.history),
        })
    }

    fn send_with(
        &self,
        chat_id: &str,
        user: UserMessage,
        cancel: &AtomicBool,
        emit: &dyn Fn(AiEvent),
        request: impl FnOnce(
            &[HistoryTurn],
            &UserMessage,
            &dyn Fn() -> bool,
            &dyn Fn(StreamEvent<'_>),
        ) -> Result<Option<Reply>, String>,
    ) {
        let cancelled_event = || AiEvent::Done {
            chat_id: chat_id.into(),
            stop_reason: "cancelled".into(),
            usage: Value::Null,
        };
        let Some(mut turn) = self.begin(chat_id) else {
            emit(AiEvent::Error {
                chat_id: chat_id.into(),
                message: tr!("ai.err.busy"),
            });
            return;
        };
        let cancelled = || cancel.load(Ordering::Relaxed) || turn.reset.load(Ordering::Relaxed);
        let response = if cancelled() {
            Ok(None)
        } else {
            request(&turn.history, &user, &cancelled, &|event| match event {
                StreamEvent::Text(text) => emit(AiEvent::Delta {
                    chat_id: chat_id.into(),
                    text: text.into(),
                }),
                StreamEvent::Thinking => emit(AiEvent::Thinking {
                    chat_id: chat_id.into(),
                }),
                StreamEvent::Tool { id, name, label, args, done, ok, chars } => emit(AiEvent::Tool {
                    chat_id: chat_id.into(),
                    id: id.into(),
                    name: name.into(),
                    label: label.into(),
                    args: args.clone(),
                    phase: if done { "done" } else { "start" },
                    ok,
                    chars,
                }),
                StreamEvent::None | StreamEvent::Stop => {}
            })
        };

        let terminal = match response {
            _ if cancelled() => cancelled_event(),
            Ok(Some(reply)) => {
                let refused = reply.anthropic_raw.is_some() && reply.stop_reason == "refusal";
                turn.history.push(HistoryTurn {
                    user,
                    assistant: reply.text,
                    anthropic_raw: reply.anthropic_raw,
                    exchange: reply.exchange,
                });
                if refused {
                    AiEvent::Error {
                        chat_id: chat_id.into(),
                        message: tr!("ai.err.refused"),
                    }
                } else {
                    AiEvent::Done {
                        chat_id: chat_id.into(),
                        stop_reason: reply.stop_reason,
                        usage: reply.usage,
                    }
                }
            }
            Ok(None) => cancelled_event(),
            Err(message) => AiEvent::Error {
                chat_id: chat_id.into(),
                message,
            },
        };
        let reset = Arc::clone(&turn.reset);
        // Trả lại lịch sử và giải phóng lượt trước khi báo kết thúc.
        drop(turn);
        emit(if reset.load(Ordering::Relaxed) {
            cancelled_event()
        } else {
            terminal
        });
    }
}

struct HistoryTurn {
    user: UserMessage,
    assistant: String,
    anthropic_raw: Option<Vec<Value>>,
    /// Các tin nhắn gọi tool / kết quả tool giữa câu hỏi và câu trả lời cuối, theo định dạng của nhà cung cấp đã tạo
    /// ra chúng; chỉ gửi lại cho đúng nhà cung cấp đó (nhà khác chỉ thấy câu hỏi + câu trả lời).
    exchange: Option<(Provider, Vec<Value>)>,
}

struct UserMessage {
    text: String,
    snapshot: Option<String>,
}

impl UserMessage {
    fn new(text: &str, context: Option<String>) -> Self {
        Self {
            text: text.into(),
            snapshot: context
                .map(|context| format!("<system_snapshot>\n{context}\n</system_snapshot>")),
        }
    }

    /// Thêm một khối dữ liệu của trang (`<disk_scan>`, `<ports>`…) vào cùng phần ngữ cảnh của lượt này.
    /// Nội dung rỗng thì bỏ qua.
    fn with_block(mut self, tag: &str, body: &str) -> Self {
        if !body.is_empty() {
            let block = format!("<{tag}>\n{body}\n</{tag}>");
            self.snapshot = Some(match self.snapshot.take() {
                Some(s) => format!("{s}\n{block}"),
                None => block,
            });
        }
        self
    }

    #[cfg(test)]
    fn with_disk(self, disk: Option<String>) -> Self {
        let body = disk.unwrap_or_default();
        self.with_block("disk_scan", &body)
    }

    fn to_wire(&self, provider: Provider) -> Value {
        let content = if provider == Provider::Anthropic {
            let mut blocks = Vec::with_capacity(2);
            if let Some(snapshot) = &self.snapshot {
                blocks.push(json!({"type": "text", "text": snapshot}));
            }
            blocks.push(json!({"type": "text", "text": self.text}));
            Value::Array(blocks)
        } else if let Some(snapshot) = &self.snapshot {
            Value::String(format!("{snapshot}\n\n{}", self.text))
        } else {
            json!(self.text)
        };
        json!({"role": "user", "content": content})
    }
}

fn wire_messages(provider: Provider, history: &[HistoryTurn], user: &UserMessage, tools: bool) -> Vec<Value> {
    let mut messages = Vec::with_capacity(history.len() * 2 + 2);
    if provider != Provider::Anthropic {
        messages.push(json!({"role": "system", "content": system_text(tools)}));
    }
    for turn in history {
        messages.push(turn.user.to_wire(provider));
        if let Some((from, exchange)) = &turn.exchange {
            if *from == provider {
                messages.extend(exchange.iter().cloned());
            }
        }
        let content = match (&turn.anthropic_raw, provider) {
            (Some(raw), Provider::Anthropic) => json!(raw),
            _ => json!(turn.assistant),
        };
        messages.push(json!({"role": "assistant", "content": content}));
    }
    messages.push(user.to_wire(provider));
    messages
}

fn supports_server_fallback(model: &str) -> bool {
    matches!(
        model,
        "claude-opus-5-5"
            | "claude-sonnet-5-5"
            | "claude-fable-5-2"
            | "claude-fable-5-1"
            | "claude-opus-5"
    )
}

fn supports_effort(model: &str, effort: &str) -> bool {
    match model {
        "claude-haiku-4-5" | "claude-haiku-4-5-20251001" => false,
        "claude-opus-4-5" | "claude-opus-4-5-20251101" => {
            matches!(effort, "low" | "medium" | "high")
        }
        "claude-opus-4-6" | "claude-sonnet-4-6" => valid_effort(effort) && effort != "xhigh",
        _ => valid_effort(effort),
    }
}

fn outgoing_messages(messages: &[Value]) -> Vec<Value> {
    // Giữ nguyên lịch sử lưu trữ và chỉ lọc các khối gửi lại cho Claude.
    let mut outgoing = messages.to_vec();
    for message in &mut outgoing {
        if message["role"] != "assistant" {
            continue;
        }
        let Some(blocks) = message.get_mut("content").and_then(Value::as_array_mut) else {
            continue;
        };
        let Some(last_fallback) = blocks.iter().rposition(|block| block["type"] == "fallback")
        else {
            continue;
        };
        let mut index = 0;
        blocks.retain(|block| {
            let discard = index < last_fallback
                && matches!(
                    block["type"].as_str(),
                    Some("thinking" | "redacted_thinking")
                );
            index += 1;
            !discard
        });
    }
    outgoing
}

/// Tool cho một yêu cầu: mô tả dạng MCP + "vòng cuối" (không cho gọi tool nữa, bắt trả lời).
#[derive(Clone, Copy)]
struct ToolsOnWire<'a> {
    defs: &'a [Value],
    last: bool,
}

/// Giao diện tiếng Anh: trả lời tiếng Anh. Kết quả công cụ vẫn tiếng Việt (model tự dịch); tên khối đề xuất giữ nguyên
/// vì giao diện nhận diện theo tên đó. Thêm ở cuối để phần đầu (đã cache) không đổi.
const REPLY_EN: &str = "LANGUAGE: the user's interface is set to English. Reply in English, even though the instructions above and \
the tool results are in Vietnamese (translate anything you quote). Keep the fenced action-block names (xoa, dong_cong, don_dep, \
tat_khoi_dong, dich_vu, ket_thuc, bien_moi_truong) and their line formats exactly as specified; only the human-readable reasons may be English.";

fn system_text(tools: bool) -> String {
    let mut s = if tools { format!("{SYSTEM}\n\n{SYSTEM_TOOLS}") } else { SYSTEM.to_owned() };
    if super::i18n::is_en() {
        s.push_str("\n\n");
        s.push_str(REPLY_EN);
    }
    s
}

fn anthropic_body(
    config: &ProviderSettings,
    messages: &[Value],
    tools: Option<ToolsOnWire<'_>>,
) -> Result<Vec<u8>, serde_json::Error> {
    #[derive(Serialize)]
    struct RequestBody<'a> {
        model: &'a str,
        max_tokens: u32,
        stream: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        tools: Option<Vec<Value>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_choice: Option<Value>,
        system: [Value; 1],
        #[serde(skip_serializing_if = "Option::is_none")]
        output_config: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        fallbacks: Option<&'static str>,
        messages: Vec<Value>,
    }

    serde_json::to_vec(&RequestBody {
        model: &config.model,
        max_tokens: 32_000,
        stream: true,
        tools: tools.map(|t| {
            t.defs
                .iter()
                .map(|d| json!({"name": d["name"], "description": d["description"], "input_schema": d["inputSchema"]}))
                .collect()
        }),
        tool_choice: tools.filter(|t| t.last).map(|_| json!({"type": "none"})),
        system: [json!({"type": "text", "text": system_text(tools.is_some()), "cache_control": {"type": "ephemeral"}})],
        output_config: supports_effort(&config.model, &config.effort)
            .then(|| json!({"effort": config.effort})),
        fallbacks: supports_server_fallback(&config.model).then_some("default"),
        messages: outgoing_messages(messages),
    })
}

fn openai_body(provider: Provider, model: &str, messages: Vec<Value>, tools: Option<ToolsOnWire<'_>>) -> Value {
    let mut body = json!({"model": model, "stream": true, "messages": messages});
    if provider == Provider::Openai {
        body["stream_options"] = json!({"include_usage": true});
    }
    if let Some(t) = tools {
        body["tools"] = t
            .defs
            .iter()
            .map(|d| json!({"type": "function", "function": {"name": d["name"], "description": d["description"], "parameters": d["inputSchema"]}}))
            .collect();
        if t.last {
            body["tool_choice"] = json!("none");
        }
    }
    body
}

fn api_key_header(key: &str) -> Result<HeaderValue, String> {
    if key.is_empty() {
        return Err(tr!("ai.err.bad_key"));
    }
    let mut value = HeaderValue::from_str(key).map_err(|_| tr!("ai.err.bad_key"))?;
    value.set_sensitive(true);
    Ok(value)
}

struct BaseUrl {
    url: String,
    host: String,
}

impl BaseUrl {
    fn for_provider(settings: &ProviderSettings, provider: Provider) -> Result<Self, String> {
        let base = settings.base_url.trim();
        Self::parse(
            if base.is_empty() {
                provider.default_base_url()
            } else {
                base
            },
            provider,
        )
    }

    fn parse(url: &str, provider: Provider) -> Result<Self, String> {
        let invalid_url = || tr!("ai.err.invalid_url");
        if url.is_empty() && provider == Provider::Custom {
            return Err(tr!("ai.err.no_custom_url"));
        }
        if !(url.starts_with("http://") || url.starts_with("https://")) || url.contains(['?', '#'])
        {
            return Err(invalid_url());
        }
        let uri: Uri = url.parse().map_err(|_| invalid_url())?;
        let host = uri
            .host()
            .filter(|host| !host.is_empty())
            .ok_or_else(invalid_url)?;
        if uri
            .authority()
            .is_some_and(|authority| authority.as_str().contains('@'))
        {
            return Err(invalid_url());
        }
        Ok(Self {
            url: url.trim_end_matches('/').into(),
            host: host.into(),
        })
    }

    fn endpoint(&self, path: &str) -> String {
        format!("{}/{path}", self.url)
    }

    fn is_loopback(&self) -> bool {
        self.host.eq_ignore_ascii_case("localhost")
            || self
                .host
                .trim_matches(['[', ']'])
                .parse::<IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    }
}

pub struct SendConfig {
    provider: Provider,
    base: BaseUrl,
    settings: ProviderSettings,
    include_context: bool,
    tools: bool,
}

impl SendConfig {
    pub fn include_context(&self) -> bool {
        self.include_context
    }
}

struct Client<'a> {
    provider: Provider,
    base: &'a BaseUrl,
    key: Option<String>,
}

impl<'a> Client<'a> {
    fn new(
        provider: Provider,
        base: &'a BaseUrl,
        settings: &ProviderSettings,
    ) -> Result<Self, String> {
        let key = settings.key(provider)?;
        if key.is_none() && provider != Provider::Custom {
            return Err(provider.missing_key());
        }
        Ok(Self {
            provider,
            base,
            key,
        })
    }

    fn auth_headers<T>(
        &self,
        request: ureq::RequestBuilder<T>,
    ) -> Result<ureq::RequestBuilder<T>, String> {
        let request = if self.base.is_loopback() {
            request.config().proxy(None).build()
        } else {
            request
        };
        if self.provider == Provider::Anthropic {
            let key = self
                .key
                .as_deref()
                .ok_or_else(|| self.provider.missing_key())?;
            Ok(request
                .header("x-api-key", api_key_header(key.trim())?)
                .header("anthropic-version", ANTHROPIC_VERSION))
        } else if let Some(key) = &self.key {
            Ok(request.header(
                "authorization",
                api_key_header(&format!("Bearer {}", key.trim()))?,
            ))
        } else {
            Ok(request)
        }
    }

    fn chat_request(
        &self,
        agent: &ureq::Agent,
        model: &str,
    ) -> Result<ureq::RequestBuilder<WithBody>, String> {
        let path = if self.provider == Provider::Anthropic {
            "messages"
        } else {
            "chat/completions"
        };
        let request = self
            .auth_headers(agent.post(self.base.endpoint(path)))?
            .header("content-type", "application/json");
        Ok(
            if self.provider == Provider::Anthropic && supports_server_fallback(model) {
                request.header("anthropic-beta", "server-side-fallback-2026-07-01")
            } else {
                request
            },
        )
    }

    fn response(
        &self,
        response: Result<Response<ureq::Body>, ureq::Error>,
    ) -> Result<Response<ureq::Body>, String> {
        let mut response = response.map_err(|_| network_error(&self.base.host))?;
        let status = response.status();
        if !status.is_success() {
            return Err(http_error(
                self.provider,
                status.as_u16(),
                response.body_mut().as_reader(),
            ));
        }
        Ok(response)
    }

    fn redact(&self, message: String) -> String {
        // Chi tiết lỗi từ máy chủ không được làm lộ khóa dùng để gửi yêu cầu.
        match self.key.as_deref().map(str::trim) {
            Some(key) if !key.is_empty() && message.contains(key) => {
                message.replace(key, &tr!("ai.redacted"))
            }
            _ => message,
        }
    }

    fn list_models(&self) -> Result<Vec<String>, String> {
        let request = self.auth_headers(HTTP.get(self.base.endpoint("models")))?;
        let result = (|| {
            let mut response = self.response(request.call())?;
            let body = response
                .body_mut()
                .read_to_vec()
                .map_err(|_| network_error(&self.base.host))?;
            let data: Value =
                serde_json::from_slice(&body).map_err(|_| tr!("ai.err.bad_models"))?;
            models_from_response(self.provider, &data)
        })();
        result.map_err(|message| self.redact(message))
    }

    /// Một lượt trả lời. Có tool: lặp "gọi model → chạy các tool model xin → gửi kết quả" tới khi model trả lời không
    /// gọi tool nữa (tối đa TOOL_ROUNDS vòng / TOOL_DEADLINE / TOOL_TRANSCRIPT ký tự kết quả; vòng cuối cấm gọi tool).
    fn stream_response(
        &self,
        settings: &ProviderSettings,
        history: &[HistoryTurn],
        user: &UserMessage,
        mut tools: Option<&mut ToolKit<'_>>,
        cancelled: &dyn Fn() -> bool,
        emit: &dyn Fn(StreamEvent<'_>),
    ) -> Result<Option<Reply>, String> {
        let started = std::time::Instant::now();
        let mut exchange: Vec<Value> = Vec::new();
        let mut text = String::new();
        let mut usage = Map::new();
        let mut transcript = 0usize;
        let mut round = 0usize;
        loop {
            let last = round + 1 >= TOOL_ROUNDS || started.elapsed() > TOOL_DEADLINE || transcript > TOOL_TRANSCRIPT;
            let on_wire = tools.as_ref().map(|t| ToolsOnWire { defs: t.defs.as_slice(), last });
            let mut messages = wire_messages(self.provider, history, user, on_wire.is_some());
            messages.extend(exchange.iter().cloned());
            let reply = match self.request_once(settings, messages, on_wire, cancelled, emit) {
                Ok(Some(reply)) => reply,
                Ok(None) => return Ok(None),
                // máy chủ OpenAI-compatible không nhận `tools`: trả lời không tra cứu
                Err((Some(400 | 422), _)) if round == 0 && tools.is_some() && self.provider != Provider::Anthropic => {
                    tools = None;
                    continue;
                }
                Err((_, message)) => return Err(self.redact(message)),
            };
            add_usage(&mut usage, &reply.usage);
            text.push_str(&reply.text);
            let calls = reply.tool_calls();
            let kit = match tools.as_deref_mut() {
                Some(kit) if !calls.is_empty() && !last => kit,
                _ => {
                    return Ok(Some(Reply {
                        text,
                        anthropic_raw: reply.anthropic_raw,
                        stop_reason: reply.stop_reason,
                        usage: Value::Object(usage),
                        exchange: (!exchange.is_empty()).then_some((self.provider, exchange)),
                        openai_calls: None,
                    }));
                }
            };
            exchange.push(reply.assistant_message(self.provider));
            let mut results = Vec::with_capacity(calls.len());
            for call in &calls {
                if cancelled() {
                    return Ok(None);
                }
                let label = (kit.label)(&call.name);
                emit(StreamEvent::Tool { id: &call.id, name: &call.name, label, args: &call.input, done: false, ok: false, chars: 0 });
                let (ok, out) = match (kit.run)(&call.name, &call.input) {
                    Ok(out) => (true, out),
                    Err(e) => (false, e),
                };
                transcript += out.len();
                emit(StreamEvent::Tool { id: &call.id, name: &call.name, label, args: &call.input, done: true, ok, chars: out.len() });
                results.push((call.id.clone(), ok, out));
            }
            if self.provider == Provider::Anthropic {
                let blocks: Vec<Value> = results
                    .iter()
                    .map(|(id, ok, out)| json!({"type": "tool_result", "tool_use_id": id, "content": out, "is_error": !ok}))
                    .collect();
                exchange.push(json!({"role": "user", "content": blocks}));
            } else {
                for (id, _, out) in results {
                    exchange.push(json!({"role": "tool", "tool_call_id": id, "content": out}));
                }
            }
            if !text.is_empty() && !text.ends_with('\n') {
                emit(StreamEvent::Text("\n\n"));
                text.push_str("\n\n");
            }
            round += 1;
        }
    }

    /// Một yêu cầu tới model. Lỗi kèm mã HTTP (nếu có) để biết máy chủ có từ chối `tools` không.
    fn request_once(
        &self,
        settings: &ProviderSettings,
        messages: Vec<Value>,
        tools: Option<ToolsOnWire<'_>>,
        cancelled: &dyn Fn() -> bool,
        emit: &dyn Fn(StreamEvent<'_>),
    ) -> Result<Option<Reply>, (Option<u16>, String)> {
        let request = self.chat_request(&HTTP, &settings.model).map_err(|e| (None, e))?;
        let body = if self.provider == Provider::Anthropic {
            anthropic_body(settings, &messages, tools).map_err(|_| (None, tr!("ai.err.make_request_claude")))?
        } else {
            serde_json::to_vec(&openai_body(self.provider, &settings.model, messages, tools))
                .map_err(|_| (None, tr!("ai.err.make_request")))?
        };
        if cancelled() {
            return Ok(None);
        }
        let response = request.send(body.as_slice());
        if cancelled() {
            return Ok(None);
        }
        let mut response = response.map_err(|_| (None, network_error(&self.base.host)))?;
        let status = response.status();
        if !status.is_success() {
            return Err((Some(status.as_u16()), http_error(self.provider, status.as_u16(), response.body_mut().as_reader())));
        }
        let mut reader = BufReader::new(response.body_mut().as_reader());
        let result = if self.provider == Provider::Anthropic {
            read_anthropic_stream(&mut reader, &self.base.host, cancelled, emit)
        } else {
            read_openai_stream(&mut reader, &self.base.host, cancelled, emit)
        };
        result.map_err(|e| (None, e))
    }
}

/// Cộng dồn số token của các vòng (trường số được cộng, trường khác lấy giá trị mới).
fn add_usage(acc: &mut Map<String, Value>, usage: &Value) {
    let Some(fields) = usage.as_object() else { return };
    for (k, v) in fields {
        match (acc.get(k).and_then(Value::as_u64), v.as_u64()) {
            (Some(a), Some(b)) => {
                acc.insert(k.clone(), json!(a + b));
            }
            _ => {
                acc.insert(k.clone(), v.clone());
            }
        }
    }
}

impl Reply {
    /// Các lời gọi tool trong câu trả lời này.
    fn tool_calls(&self) -> Vec<ToolCall> {
        if let Some(raw) = &self.anthropic_raw {
            return raw
                .iter()
                .filter(|b| b["type"] == "tool_use")
                .map(|b| ToolCall {
                    id: b["id"].as_str().unwrap_or_default().to_owned(),
                    name: b["name"].as_str().unwrap_or_default().to_owned(),
                    input: if b["input"].is_object() { b["input"].clone() } else { json!({}) },
                })
                .collect();
        }
        self.openai_calls
            .as_ref()
            .and_then(Value::as_array)
            .map(|calls| {
                calls
                    .iter()
                    .map(|c| ToolCall {
                        id: c["id"].as_str().unwrap_or_default().to_owned(),
                        name: c["function"]["name"].as_str().unwrap_or_default().to_owned(),
                        input: c["function"]["arguments"]
                            .as_str()
                            .and_then(|a| serde_json::from_str::<Value>(a).ok())
                            .filter(Value::is_object)
                            .unwrap_or_else(|| json!({})),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Tin nhắn của model (có lời gọi tool) để gửi lại ở vòng sau.
    fn assistant_message(&self, provider: Provider) -> Value {
        match (&self.anthropic_raw, provider) {
            (Some(raw), Provider::Anthropic) => json!({"role": "assistant", "content": raw}),
            _ => {
                let content = if self.text.is_empty() { Value::Null } else { json!(self.text) };
                json!({"role": "assistant", "content": content, "tool_calls": self.openai_calls.clone().unwrap_or_else(|| json!([]))})
            }
        }
    }
}

fn models_from_response(provider: Provider, data: &Value) -> Result<Vec<String>, String> {
    let mut models: Vec<_> = if let Some(entries) = data["data"].as_array() {
        entries
            .iter()
            .filter_map(|entry| entry["id"].as_str())
            .collect()
    } else if let Some(entries) = data["models"]
        .as_array()
        .filter(|_| provider != Provider::Anthropic)
    {
        entries
            .iter()
            .filter_map(|entry| {
                entry
                    .as_str()
                    .or_else(|| entry["id"].as_str())
                    .or_else(|| entry["name"].as_str())
                    .or_else(|| entry["model"].as_str())
            })
            .collect()
    } else {
        return Err(tr!("ai.err.bad_models"));
    };
    models.retain(|id| !id.trim().is_empty());
    models.sort_unstable();
    models.dedup();
    Ok(models.into_iter().map(str::to_owned).collect())
}

fn network_error(host: &str) -> String {
    tr!("ai.err.network", host = host)
}

fn error_detail(payload: &Value) -> Option<&str> {
    payload["error"]["message"]
        .as_str()
        .filter(|message| !message.is_empty())
}

fn with_detail(mut message: String, detail: Option<&str>) -> String {
    if let Some(detail) = detail {
        message.push(' ');
        message.push_str(detail);
    }
    message
}

fn http_error(provider: Provider, status: u16, body: impl Read) -> String {
    let provider = if provider == Provider::Anthropic {
        "Claude"
    } else {
        provider.name()
    };
    let message = match status {
        401 => tr!("ai.err.bad_key"),
        403 => tr!("ai.err.forbidden"),
        404 => tr!("ai.err.not_found"),
        429 => tr!("ai.err.rate_limited"),
        500..=599 => tr!("ai.err.overloaded", provider = provider),
        _ => tr!("ai.err.http", provider = provider, status = status),
    };
    let mut raw = Vec::new();
    let payload: Option<Value> = body
        .take(64 * 1024)
        .read_to_end(&mut raw)
        .ok()
        .and_then(|_| serde_json::from_slice(&raw).ok());
    with_detail(message, payload.as_ref().and_then(error_detail))
}

#[derive(Default)]
struct SseFrame {
    event: String,
    data: String,
}

impl SseFrame {
    fn push_line(&mut self, line: &str) -> bool {
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            return true;
        }
        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);
        match field {
            "event" => {
                self.event.clear();
                self.event.push_str(value);
            }
            "data" => {
                self.data.push_str(value);
                self.data.push('\n');
            }
            _ => {}
        }
        false
    }
}

#[derive(Default)]
struct StreamState {
    blocks: BTreeMap<u64, ContentBlock>,
    usage: Map<String, Value>,
    stop_reason: Option<String>,
    thinking_emitted: bool,
}

struct ContentBlock {
    fields: Map<String, Value>,
    input_json: String,
    stopped: bool,
}

struct Reply {
    text: String,
    anthropic_raw: Option<Vec<Value>>,
    stop_reason: String,
    usage: Value,
    /// tin nhắn gọi tool / kết quả tool của lượt (xem HistoryTurn::exchange)
    exchange: Option<(Provider, Vec<Value>)>,
    /// OpenAI: mảng `tool_calls` của câu trả lời này
    openai_calls: Option<Value>,
}

enum StreamEvent<'a> {
    None,
    Thinking,
    Text(&'a str),
    Stop,
    Tool { id: &'a str, name: &'a str, label: &'static str, args: &'a Value, done: bool, ok: bool, chars: usize },
}

fn invalid_stream() -> String {
    tr!("ai.err.invalid_stream_claude")
}

impl StreamState {
    fn on_event<'a>(&mut self, name: &str, data: &'a Value) -> Result<StreamEvent<'a>, String> {
        match name {
            "message_start" => self.merge_usage(&data["message"]["usage"]),
            "content_block_start" => {
                let index = block_index(data)?;
                let fields = data["content_block"]
                    .as_object()
                    .cloned()
                    .ok_or_else(invalid_stream)?;
                let should_announce = !self.thinking_emitted
                    && fields.get("type").and_then(Value::as_str) == Some("thinking");
                let block = ContentBlock {
                    fields,
                    input_json: String::new(),
                    stopped: false,
                };
                if self.blocks.insert(index, block).is_some() {
                    return Err(invalid_stream());
                }
                if should_announce {
                    self.thinking_emitted = true;
                    return Ok(StreamEvent::Thinking);
                }
            }
            "content_block_delta" => {
                let block = self.open_block(data)?;
                let delta = data["delta"].as_object().ok_or_else(invalid_stream)?;
                return block.apply_delta(delta);
            }
            "content_block_stop" => {
                let block = self.open_block(data)?;
                if !block.input_json.is_empty() {
                    let input =
                        serde_json::from_str(&block.input_json).map_err(|_| invalid_stream())?;
                    block.fields.insert("input".into(), input);
                    block.input_json.clear();
                }
                block.stopped = true;
            }
            "message_delta" => {
                if let Some(reason) = data["delta"]["stop_reason"].as_str() {
                    self.stop_reason = Some(reason.into());
                }
                self.merge_usage(&data["usage"]);
            }
            "message_stop" => return Ok(StreamEvent::Stop),
            "error" => {
                let message = match data["error"]["type"].as_str() {
                    Some("authentication_error") => tr!("ai.err.bad_key"),
                    Some("rate_limit_error") => tr!("ai.err.rate_limited"),
                    Some("overloaded_error" | "api_error") => {
                        tr!("ai.err.overloaded", provider = "Claude")
                    }
                    _ => tr!("ai.err.claude_failed"),
                };
                return Err(with_detail(message, error_detail(data)));
            }
            _ => {}
        }
        Ok(StreamEvent::None)
    }

    fn merge_usage(&mut self, value: &Value) {
        if let Some(fields) = value.as_object() {
            self.usage.extend(fields.clone());
        }
    }

    fn open_block(&mut self, data: &Value) -> Result<&mut ContentBlock, String> {
        self.blocks
            .get_mut(&block_index(data)?)
            .filter(|block| !block.stopped)
            .ok_or_else(invalid_stream)
    }

    fn finish(self) -> Result<Reply, String> {
        let stop_reason = self.stop_reason.ok_or_else(invalid_stream)?;
        let mut content = Vec::with_capacity(self.blocks.len());
        for (index, block) in self.blocks {
            if !block.stopped || index != content.len() as u64 {
                return Err(invalid_stream());
            }
            content.push(Value::Object(block.fields));
        }
        let text = content
            .iter()
            .filter(|block| block["type"] == "text")
            .filter_map(|block| block["text"].as_str())
            .collect();
        Ok(Reply {
            text,
            anthropic_raw: Some(content),
            stop_reason,
            usage: Value::Object(self.usage),
            exchange: None,
            openai_calls: None,
        })
    }
}

fn block_index(data: &Value) -> Result<u64, String> {
    data["index"].as_u64().ok_or_else(invalid_stream)
}

impl ContentBlock {
    fn apply_delta<'a>(
        &mut self,
        delta: &'a Map<String, Value>,
    ) -> Result<StreamEvent<'a>, String> {
        let kind = delta
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(invalid_stream)?;
        if kind == "input_json_delta" {
            self.input_json.push_str(delta_text(delta, "partial_json")?);
            return Ok(StreamEvent::None);
        }

        let field = match kind {
            "text_delta" => "text",
            "thinking_delta" => "thinking",
            "signature_delta" => "signature",
            "citations_delta" => {
                let citation = delta.get("citation").ok_or_else(invalid_stream)?;
                let citations = self.fields.entry("citations").or_insert_with(|| json!([]));
                if citations.is_null() {
                    *citations = json!([]);
                }
                citations
                    .as_array_mut()
                    .ok_or_else(invalid_stream)?
                    .push(citation.clone());
                return Ok(StreamEvent::None);
            }
            _ => {
                // Giữ trường mới và nối các đoạn chuỗi hoặc mảng theo tên trường.
                for (name, value) in delta.iter().filter(|(name, _)| *name != "type") {
                    if let Some(current) = self.fields.get_mut(name) {
                        merge_fragment(current, value);
                    } else {
                        self.fields.insert(name.clone(), value.clone());
                    }
                }
                return Ok(StreamEvent::None);
            }
        };
        let text = delta_text(delta, field)?;
        if let Some(Value::String(current)) = self.fields.get_mut(field) {
            current.push_str(text);
        } else {
            self.fields.insert(field.into(), Value::String(text.into()));
        }
        if kind == "text_delta" {
            Ok(StreamEvent::Text(text))
        } else {
            Ok(StreamEvent::None)
        }
    }
}

fn merge_fragment(current: &mut Value, fragment: &Value) {
    match (current, fragment) {
        (Value::String(current), Value::String(fragment)) => current.push_str(fragment),
        (Value::Array(current), Value::Array(fragment)) => current.extend(fragment.iter().cloned()),
        (Value::Object(current), Value::Object(fragment)) => {
            for (name, value) in fragment {
                if let Some(field) = current.get_mut(name) {
                    merge_fragment(field, value);
                } else {
                    current.insert(name.clone(), value.clone());
                }
            }
        }
        (current, fragment) => *current = fragment.clone(),
    }
}

fn delta_text<'a>(delta: &'a Map<String, Value>, field: &str) -> Result<&'a str, String> {
    delta
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(invalid_stream)
}

fn read_anthropic_stream(
    reader: &mut impl BufRead,
    host: &str,
    cancelled: &dyn Fn() -> bool,
    emit: &dyn Fn(StreamEvent<'_>),
) -> Result<Option<Reply>, String> {
    let mut state = StreamState::default();
    let mut frame = SseFrame::default();
    let mut line = String::new();
    loop {
        if cancelled() {
            return Ok(None);
        }
        line.clear();
        let read = reader.read_line(&mut line);
        if cancelled() {
            return Ok(None);
        }
        if read.map_err(|_| network_error(host))? == 0 {
            return Err(tr!("ai.err.claude_ended"));
        }
        if !frame.push_line(&line) {
            continue;
        }
        if !frame.data.is_empty() && frame.event != "ping" {
            let data: Value = serde_json::from_str(&frame.data).map_err(|_| invalid_stream())?;
            let name = if frame.event.is_empty() {
                data["type"].as_str().unwrap_or("")
            } else {
                &frame.event
            };
            match state.on_event(name, &data)? {
                StreamEvent::Stop => return state.finish().map(Some),
                StreamEvent::None => {}
                event => emit(event),
            }
        }
        frame.event.clear();
        frame.data.clear();
    }
}

fn read_openai_stream(
    reader: &mut impl BufRead,
    host: &str,
    cancelled: &dyn Fn() -> bool,
    emit: &dyn Fn(StreamEvent<'_>),
) -> Result<Option<Reply>, String> {
    let invalid_stream = || tr!("ai.err.invalid_stream");
    let mut text = String::new();
    let mut stop_reason = None;
    let mut usage = Value::Null;
    let mut thinking_emitted = false;
    // lời gọi tool đến từng mảnh theo `index`: (id, tên, đối số JSON dạng chuỗi)
    let mut calls: BTreeMap<u64, (String, String, String)> = BTreeMap::new();
    let mut line = String::new();
    while !cancelled() {
        line.clear();
        let read = reader.read_line(&mut line);
        if cancelled() {
            return Ok(None);
        }
        if read.map_err(|_| network_error(host))? == 0 {
            return Err(tr!("ai.err.stream_ended"));
        }
        let Some(data) = line
            .strip_prefix("data:")
            .map(str::trim)
            .filter(|data| !data.is_empty())
        else {
            continue;
        };
        if data == "[DONE]" {
            let openai_calls = (!calls.is_empty()).then(|| {
                calls
                    .values()
                    .map(|(id, name, args)| json!({"id": id, "type": "function", "function": {"name": name, "arguments": args}}))
                    .collect()
            });
            return Ok(Some(Reply {
                text,
                anthropic_raw: None,
                stop_reason: stop_reason.ok_or_else(invalid_stream)?,
                usage,
                exchange: None,
                openai_calls,
            }));
        }
        let data: Value = serde_json::from_str(data).map_err(|_| invalid_stream())?;
        if data["error"].is_object() {
            return Err(with_detail(
                tr!("ai.err.server_failed"),
                error_detail(&data),
            ));
        }
        let choice = &data["choices"][0];
        let delta = &choice["delta"];
        if !thinking_emitted
            && (delta["reasoning_content"].is_string() || delta["reasoning"].is_string())
        {
            thinking_emitted = true;
            emit(StreamEvent::Thinking);
        }
        if let Some(content) = delta["content"]
            .as_str()
            .filter(|content| !content.is_empty())
        {
            text.push_str(content);
            emit(StreamEvent::Text(content));
        }
        if let Some(parts) = delta["tool_calls"].as_array() {
            for (i, part) in parts.iter().enumerate() {
                let index = part["index"].as_u64().unwrap_or(i as u64);
                let entry = calls.entry(index).or_default();
                if let Some(id) = part["id"].as_str() {
                    entry.0.push_str(id);
                }
                if let Some(name) = part["function"]["name"].as_str() {
                    entry.1.push_str(name);
                }
                if let Some(args) = part["function"]["arguments"].as_str() {
                    entry.2.push_str(args);
                }
            }
        }
        if let Some(reason) = choice["finish_reason"].as_str() {
            stop_reason = Some(reason.into());
        }
        if let (Some(input), Some(output)) = (
            data["usage"]["prompt_tokens"].as_u64(),
            data["usage"]["completion_tokens"].as_u64(),
        ) {
            usage = json!({"input_tokens": input, "output_tokens": output});
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn legacy_config_migrates_and_dpapi_keys_round_trip_independently() {
        struct TemporaryConfig(PathBuf);
        impl Drop for TemporaryConfig {
            fn drop(&mut self) {
                let _ = fs::remove_file(&self.0);
                let _ = fs::remove_dir(self.0.parent().unwrap());
            }
        }

        let defaults = serde_json::to_value(AiConfig::default().view()).unwrap();
        assert_eq!(defaults["provider"], "anthropic");
        assert_eq!(defaults["include_context"], false);
        assert_eq!(defaults["providers"]["anthropic"]["model"], DEFAULT_MODEL);
        assert_eq!(defaults["providers"]["anthropic"]["effort"], DEFAULT_EFFORT);
        assert_eq!(defaults["providers"]["openai"]["model"], "");
        assert_eq!(
            defaults["providers"]["openai"]["default_base_url"],
            "https://api.openai.com/v1"
        );
        assert_eq!(defaults["providers"]["custom"]["model"], "");
        assert_eq!(defaults["providers"]["custom"]["default_base_url"], "");

        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory =
            env::temp_dir().join(format!("fomeg-ai-unit-{}-{stamp}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        let path = TemporaryConfig(directory.join("ai.json"));
        let legacy_key = base64_encode(&dpapi(&mut b"anthropic-test-key".to_vec(), true).unwrap());
        let legacy = json!({
            "model": "claude-legacy",
            "effort": "high",
            "include_context": true,
            "key_dpapi": legacy_key,
        });
        fs::write(&path.0, serde_json::to_vec(&legacy).unwrap()).unwrap();

        let loaded = get_config_at(&path.0).unwrap();
        assert_eq!(loaded.providers.anthropic.model, "claude-legacy");
        assert_eq!(loaded.providers.anthropic.effort, "high");
        assert!(loaded.include_context);
        let migrated: Value = serde_json::from_slice(&fs::read(&path.0).unwrap()).unwrap();
        assert!(migrated.get("key_dpapi").is_none());
        assert_eq!(migrated["providers"]["anthropic"]["key_dpapi"], legacy_key);

        let updated = set_config_at(
            &path.0,
            AiConfigUpdate {
                provider: Some("openai".into()),
                model: Some(" chosen-model ".into()),
                effort: Some("".into()),
                api_key: Some(" openai-test-key ".into()),
                ..AiConfigUpdate::default()
            },
        )
        .unwrap();
        let view = serde_json::to_value(updated).unwrap();
        assert_eq!(view["provider"], "openai");
        assert_eq!(view["providers"]["openai"]["model"], "chosen-model");
        assert_eq!(view["providers"]["openai"]["effort"], "");
        assert_eq!(view["providers"]["openai"]["has_key"], true);
        assert_eq!(view["providers"]["custom"]["has_key"], false);
        set_config_at(
            &path.0,
            AiConfigUpdate {
                target: Some("custom".into()),
                base_url: Some(" http://127.0.0.1:1234/v1/// ".into()),
                api_key: Some("custom-test-key".into()),
                ..AiConfigUpdate::default()
            },
        )
        .unwrap();
        let bytes = fs::read(&path.0).unwrap();
        let disk: Value = serde_json::from_slice(&bytes).unwrap();
        for (provider, id) in [
            (Provider::Anthropic, "anthropic"),
            (Provider::Openai, "openai"),
            (Provider::Custom, "custom"),
        ] {
            let key = format!("{id}-test-key");
            let loaded = AiConfig::load(&path.0).unwrap();
            assert_eq!(
                loaded.settings(provider).key(provider).unwrap(),
                Some(key.clone())
            );
            assert!(!disk["providers"][id]["key_dpapi"]
                .as_str()
                .unwrap()
                .is_empty());
            assert!(!String::from_utf8_lossy(&bytes).contains(&key));
        }
        assert_eq!(
            disk["providers"]["custom"]["base_url"],
            "http://127.0.0.1:1234/v1"
        );

        assert!(set_config_at(
            &path.0,
            AiConfigUpdate {
                target: Some("custom".into()),
                base_url: Some("ftp://localhost/v1".into()),
                ..AiConfigUpdate::default()
            }
        )
        .is_err());
        assert_eq!(fs::read(&path.0).unwrap(), bytes);

        set_config_at(
            &path.0,
            AiConfigUpdate {
                api_key: Some(String::new()),
                ..AiConfigUpdate::default()
            },
        )
        .unwrap();
        let loaded = AiConfig::load(&path.0).unwrap();
        assert_eq!(loaded.providers.openai.key(Provider::Openai).unwrap(), None);
        assert_eq!(
            loaded
                .providers
                .anthropic
                .key(Provider::Anthropic)
                .unwrap()
                .as_deref(),
            Some("anthropic-test-key")
        );
        assert_eq!(
            loaded
                .providers
                .custom
                .key(Provider::Custom)
                .unwrap()
                .as_deref(),
            Some("custom-test-key")
        );
        assert_eq!(
            loaded.prepare_send().err().as_deref(),
            Some("Chưa có API key OpenAI. Hãy thêm key trong phần cài đặt.")
        );
    }

    #[test]
    fn send_preflight_requires_a_model_and_custom_url() {
        let mut config = AiConfig::default();
        config
            .update(AiConfigUpdate {
                provider: Some("custom".into()),
                ..AiConfigUpdate::default()
            })
            .unwrap();
        assert_eq!(
            config.prepare_send().err().as_deref(),
            Some("Chưa chọn model cho Custom.")
        );
        config
            .update(AiConfigUpdate {
                model: Some("local-model".into()),
                ..AiConfigUpdate::default()
            })
            .unwrap();
        assert_eq!(
            config.prepare_send().err().as_deref(),
            Some("Chưa nhập URL API cho Custom.")
        );
        assert!(config
            .update(AiConfigUpdate {
                target: Some("unknown".into()),
                ..AiConfigUpdate::default()
            })
            .is_err());
        assert!(config
            .update(AiConfigUpdate {
                effort: Some("unknown".into()),
                ..AiConfigUpdate::default()
            })
            .is_err());
        for url in [
            "",
            "http://",
            "https://localhost/v1?mode=test",
            "http://user@localhost/v1",
        ] {
            assert!(config
                .update(AiConfigUpdate {
                    base_url: Some(url.into()),
                    ..AiConfigUpdate::default()
                })
                .is_err());
        }
    }

    #[test]
    fn openai_compatible_requests_use_bearer_auth_and_only_openai_requests_usage() {
        let agent = ureq::Agent::new_with_defaults();
        for (provider, key) in [
            (Provider::Openai, Some("openai-key")),
            (Provider::Custom, Some("custom-key")),
            (Provider::Custom, None),
        ] {
            let base = BaseUrl::parse("http://127.0.0.1:8080/v1", provider).unwrap();
            let client = Client {
                provider,
                base: &base,
                key: key.map(str::to_owned),
            };
            let request = client.chat_request(&agent, "test-model").unwrap();
            let headers = request.headers_ref().unwrap();
            let auth = headers.get("authorization");
            assert_eq!(
                auth.map(|header| header.to_str().unwrap()),
                key.map(|key| format!("Bearer {key}")).as_deref()
            );
            assert!(auth.is_none_or(HeaderValue::is_sensitive));
            assert!(!headers.contains_key("x-api-key"));
            assert!(!headers.contains_key("anthropic-beta"));

            let user = UserMessage::new("Question", Some("Snapshot".into()));
            let body = openai_body(provider, "test-model", wire_messages(provider, &[], &user, false), None);
            let mut expected = json!({
                "model": "test-model", "stream": true,
                "messages": [
                    {"role": "system", "content": SYSTEM},
                    {"role": "user", "content": "<system_snapshot>\nSnapshot\n</system_snapshot>\n\nQuestion"},
                ],
            });
            if provider == Provider::Openai {
                expected["stream_options"] = json!({"include_usage": true});
            }
            assert_eq!(body, expected);
        }
    }

    #[test]
    fn disk_scan_joins_the_snapshot_or_stands_alone() {
        let both = UserMessage::new("Q", Some("S".into())).with_disk(Some("D".into()));
        assert_eq!(both.snapshot.as_deref(), Some("<system_snapshot>\nS\n</system_snapshot>\n<disk_scan>\nD\n</disk_scan>"));
        let only = UserMessage::new("Q", None).with_disk(Some("D".into()));
        assert_eq!(only.snapshot.as_deref(), Some("<disk_scan>\nD\n</disk_scan>"));
        assert!(UserMessage::new("Q", None).with_disk(Some(String::new())).snapshot.is_none());
    }

    #[test]
    fn openai_sse_emits_reasoning_once_and_maps_usage_and_errors() {
        let stream = concat!(
            ": keep-alive\r\n",
            "data:{\"choices\":[{\"delta\":{\"reasoning\":\"internal\",\"content\":\"one\"}}]}\r\n",
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"more\",\"content\":\" two\"}}]}\n",
            "data: {\"choices\":[{\"finish_reason\":\"length\"}]}\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":2}}\n",
            "data:[DONE]\n",
        );
        let events = RefCell::new(Vec::new());
        let reply = read_openai_stream(&mut stream.as_bytes(), "localhost", &|| false, &|event| {
            events.borrow_mut().push(match event {
                StreamEvent::Thinking => "thinking".into(),
                StreamEvent::Text(text) => text.to_owned(),
                _ => panic!("Sự kiện không hợp lệ."),
            });
        })
        .unwrap()
        .unwrap();
        assert_eq!(events.into_inner(), ["thinking", "one", " two"]);
        assert_eq!(reply.text, "one two");
        assert_eq!(reply.stop_reason, "length");
        assert_eq!(reply.usage, json!({"input_tokens": 1, "output_tokens": 2}));
        assert!(reply.anthropic_raw.is_none());

        let error = "data: {\"error\":{\"message\":\"model failed\"}}\n";
        assert_eq!(
            read_openai_stream(&mut error.as_bytes(), "localhost", &|| false, &|_| {})
                .err()
                .as_deref(),
            Some("Máy chủ AI báo lỗi khi trả lời. model failed")
        );
    }

    #[test]
    fn anthropic_sse_preserves_thinking_and_stream_error_mapping() {
        let stream = concat!(
            "event: ping\ndata: {}\n\n",
            "data:{\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":2}}}\n\n",
            "event: content_block_start\ndata: {\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n",
            "event: content_block_delta\ndata: {\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"signed\"}}\n\n",
            "event: content_block_stop\ndata: {\"index\":0}\n\n",
            "event: content_block_start\ndata: {\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"Start\"}}\n\n",
            "event: content_block_delta\ndata: {\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\" end\"}}\n\n",
            "event: content_block_stop\ndata: {\"index\":1}\n\n",
            "event: message_delta\ndata: {\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":4}}\n\n",
            "event: message_stop\ndata: {}\n\n",
        );
        let events = RefCell::new(Vec::new());
        let reply =
            read_anthropic_stream(&mut stream.as_bytes(), "localhost", &|| false, &|event| {
                events.borrow_mut().push(match event {
                    StreamEvent::Thinking => "thinking".into(),
                    StreamEvent::Text(text) => text.to_owned(),
                    _ => panic!("Sự kiện không hợp lệ."),
                });
            })
            .unwrap()
            .unwrap();
        assert_eq!(events.into_inner(), ["thinking", " end"]);
        assert_eq!(reply.stop_reason, "end_turn");
        assert_eq!(reply.text, "Start end");
        assert_eq!(reply.anthropic_raw.unwrap()[0]["signature"], "signed");
        assert_eq!(reply.usage, json!({"input_tokens": 2, "output_tokens": 4}));

        let error = "event: error\ndata: {\"error\":{\"type\":\"rate_limit_error\",\"message\":\"slow down\"}}\n\n";
        assert_eq!(
            read_anthropic_stream(&mut error.as_bytes(), "localhost", &|| false, &|_| {})
                .err()
                .as_deref(),
            Some("Bị giới hạn tốc độ, thử lại sau ít phút. slow down")
        );
    }

    #[test]
    fn outgoing_history_keeps_only_thinking_after_the_last_fallback() {
        let first = json!({"type": "fallback", "model": "claude-sonnet-5-5"});
        let last = json!({"type": "fallback", "model": "claude-haiku-5-5"});
        let thinking = json!({"type": "thinking", "signature": "chữ ký"});
        let redacted = json!({"type": "redacted_thinking", "data": "dữ liệu"});
        let text = json!({"type": "text", "text": "Câu trả lời"});
        let other = json!({"type": "connector_text", "text": "Kết quả"});
        let messages = [
            json!({"role": "assistant", "content": [thinking, redacted, text]}),
            json!({"role": "user", "content": [thinking, last, text]}),
            json!({"role": "assistant", "content": [
                thinking, first, redacted, thinking, other, text, last, redacted, thinking, text
            ]}),
            json!({"role": "assistant", "content": [redacted, first, thinking]}),
            json!({"role": "assistant", "content": "Câu trả lời dạng chuỗi"}),
            json!({"role": "assistant"}),
        ];
        let stored = messages.clone();
        let body = anthropic_body(&ProviderSettings::anthropic(), &messages, None).unwrap();
        let outgoing: Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(outgoing["messages"][0], messages[0]);
        assert_eq!(outgoing["messages"][1], messages[1]);
        assert_eq!(
            outgoing["messages"][2]["content"],
            json!([first, other, text, last, redacted, thinking, text])
        );
        assert_eq!(outgoing["messages"][3]["content"], json!([first, thinking]));
        assert_eq!(outgoing["messages"][4], messages[4]);
        assert_eq!(outgoing["messages"][5], messages[5]);
        assert_eq!(messages, stored);
    }

    #[test]
    fn request_fallback_body_and_header_follow_model_support() {
        let agent = ureq::Agent::new_with_defaults();
        let base = BaseUrl::parse("http://127.0.0.1:8080/v1", Provider::Anthropic).unwrap();
        let client = Client {
            provider: Provider::Anthropic,
            base: &base,
            key: Some("test-key".into()),
        };
        for (model, fallback, effort) in [
            ("claude-opus-5-5", true, Some("max")),
            ("claude-sonnet-5-5", true, Some("max")),
            ("claude-fable-5-2", true, Some("max")),
            ("claude-fable-5-1", true, Some("max")),
            ("claude-opus-5", true, Some("max")),
            ("claude-haiku-5-5", false, Some("max")),
            ("claude-haiku-4-5-20251001", false, None),
            ("mô-hình-mới", false, Some("max")),
        ] {
            let config = ProviderSettings {
                model: model.into(),
                effort: "max".into(),
                ..ProviderSettings::default()
            };
            let body = anthropic_body(&config, &[], None).unwrap();
            let body: Value = serde_json::from_slice(&body).unwrap();
            let request = client.chat_request(&agent, model).unwrap();
            let headers = request.headers_ref().unwrap();
            let beta = headers.get("anthropic-beta");

            assert_eq!(headers["x-api-key"], "test-key");
            assert_eq!(headers["anthropic-version"], "2026-10-21");
            assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
            assert_eq!(
                body.get("fallbacks").and_then(Value::as_str),
                fallback.then_some("default")
            );
            assert_eq!(
                beta.map(|header| header.to_str().unwrap()),
                fallback.then_some("server-side-fallback-2026-07-01")
            );
            assert_eq!(
                body.get("output_config"),
                effort.map(|effort| json!({"effort": effort})).as_ref()
            );
        }

        let invalid = ProviderSettings {
            model: "mô-hình-mới".into(),
            effort: "không-hợp-lệ".into(),
            ..ProviderSettings::default()
        };
        let body: Value = serde_json::from_slice(&anthropic_body(&invalid, &[], None).unwrap()).unwrap();
        assert!(body.get("output_config").is_none());
    }

    #[test]
    fn overlapping_turn_is_rejected_and_terminal_callback_can_send_again() {
        use std::cell::Cell;

        for succeed in [true, false] {
            let chats = Chats::default();
            let cancel = AtomicBool::new(false);
            let rejected = Cell::new(false);
            let retried = Cell::new(false);
            chats.send_with(
                "chat",
                UserMessage::new("Câu hỏi đầu tiên", None),
                &cancel,
                &|event| {
                    match event {
                        AiEvent::Done { stop_reason, .. } if succeed => {
                            assert_eq!(stop_reason, "end_turn");
                        }
                        AiEvent::Error { message, .. } if !succeed => {
                            assert_eq!(message, "Lỗi thử nghiệm.");
                        }
                        _ => panic!("Sự kiện kết thúc không đúng."),
                    }
                    chats.send_with(
                        "chat",
                        UserMessage::new("Câu hỏi tiếp theo", None),
                        &cancel,
                        &|event| {
                            assert!(matches!(event, AiEvent::Done { stop_reason, .. } if stop_reason == "cancelled"));
                            retried.set(true);
                        },
                        |history, user, _, _| {
                            assert_eq!(history.len(), usize::from(succeed));
                            assert_eq!(user.text, "Câu hỏi tiếp theo");
                            Ok(None)
                        },
                    );
                },
                |_, _, _, _| {
                    chats.send_with(
                        "chat",
                        UserMessage::new("Lượt trùng", None),
                        &AtomicBool::new(true),
                        &|event| {
                            let AiEvent::Error { chat_id, message } = event else {
                                panic!("Lượt gửi trùng phải trả về lỗi.");
                            };
                            assert_eq!(chat_id, "chat");
                            assert_eq!(message, "Cuộc trò chuyện đang có một lượt trả lời, hãy đợi hoặc bấm Dừng.");
                            rejected.set(true);
                        },
                        |_, _, _, _| panic!("Lượt gửi trùng đã gửi yêu cầu."),
                    );
                    if succeed {
                        Ok(Some(Reply {
                            text: "Câu trả lời".into(),
                            anthropic_raw: None,
                            stop_reason: "end_turn".into(),
                            usage: Value::Null,
                            exchange: None,
                            openai_calls: None,
                        }))
                    } else {
                        Err("Lỗi thử nghiệm.".into())
                    }
                },
            );
            assert!(rejected.get());
            assert!(retried.get());
        }
    }
}
