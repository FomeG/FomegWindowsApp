// Ngôn ngữ của thông báo backend (lỗi, nhãn gửi lên giao diện, câu trả lời của trợ lý AI).
// Chuỗi nằm trong `src-tauri/locales/*.yml` (rust-i18n, `_version: 2`: mỗi khoá có `vi` + `en`), nhúng vào exe lúc build.
// Mặc định tiếng Việt (kể cả trong test / MCP); giao diện gọi `set_locale` lúc mở và khi người dùng đổi ngôn ngữ.
// Dùng macro `tr!("khoa")` / `tr!("khoa", ten = gia_tri)` (chỗ giữ chỗ trong yml: `%{ten}`).
#[cfg(not(test))]
mod flag {
    use std::sync::atomic::{AtomicBool, Ordering};

    static EN: AtomicBool = AtomicBool::new(false);

    pub fn store(en: bool) {
        EN.store(en, Ordering::Relaxed);
    }

    pub fn load() -> bool {
        EN.load(Ordering::Relaxed)
    }
}

// Trong unit test: ngôn ngữ theo từng luồng, để test đổi sang "en" không làm hỏng các test chạy song song
// đang so chuỗi tiếng Việt.
#[cfg(test)]
mod flag {
    use std::cell::Cell;

    thread_local!(static EN: Cell<bool> = const { Cell::new(false) });

    pub fn store(en: bool) {
        EN.with(|c| c.set(en));
    }

    pub fn load() -> bool {
        EN.with(Cell::get)
    }
}

pub fn set(locale: &str) {
    flag::store(locale == "en");
}

/// "vi" hoặc "en".
pub fn locale() -> &'static str {
    if flag::load() {
        "en"
    } else {
        "vi"
    }
}

pub fn is_en() -> bool {
    flag::load()
}

/// Chuỗi theo ngôn ngữ hiện tại; trả `String`.
#[macro_export]
macro_rules! tr {
    ($key:expr) => {
        rust_i18n::t!($key, locale = $crate::core::i18n::locale()).into_owned()
    };
    ($key:expr, $($name:ident = $value:expr),+ $(,)?) => {
        rust_i18n::t!($key, locale = $crate::core::i18n::locale(), $($name = $value),+).into_owned()
    };
}

#[cfg(test)]
mod tests {
    #[test]
    fn switches_language() {
        assert_eq!(crate::tr!("common.unknown_error"), "Lỗi không xác định");
        super::set("en");
        assert_eq!(crate::tr!("common.unknown_error"), "Unknown error");
        assert_eq!(crate::tr!("common.test_param", name = "X"), "Hello X");
        super::set("vi");
        assert_eq!(crate::tr!("common.test_param", name = "X"), "Chào X");
    }
}
