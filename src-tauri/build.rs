// Bản release nhúng manifest đòi quyền Admin. Bản debug (cargo tauri dev / build --debug)
// dùng asInvoker để chạy được không cần UAC; đặt FOMEG_NO_ADMIN=1 để build release không đòi Admin.
//
// Nhúng MCP: nếu đã có <workspace>/target/release/fomeg-windows-app-mcp.exe (npm run build:mcp, chạy tự động trước `npm run tauri build`)
// thì bật cfg `embed_mcp` để main.rs nhúng nó vào exe của app.
fn main() {
    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rerun-if-changed=app-dev.manifest");
    println!("cargo:rerun-if-env-changed=FOMEG_NO_ADMIN");
    // tự cập nhật: CI nhúng repo + khoá công khai (src/update.rs, option_env!)
    println!("cargo:rerun-if-env-changed=FOMEG_UPDATE_REPO");
    println!("cargo:rerun-if-env-changed=FOMEG_UPDATE_PUBKEY");
    println!("cargo:rustc-check-cfg=cfg(embed_mcp)");
    let release = std::env::var("PROFILE").as_deref() == Ok("release");
    if release {
        let target = std::env::var_os("CARGO_TARGET_DIR")
            .map(std::path::PathBuf::from)
            // workspace ở gốc repo (Cargo.toml cấp trên): target/ nằm cạnh src-tauri/
            .unwrap_or_else(|| std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("..").join("target"));
        let mcp = target.join("release").join("fomeg-windows-app-mcp.exe");
        // luôn theo dõi tệp (kể cả khi chưa có): build:mcp tạo ra nó sau -> build script chạy lại và nhúng
        println!("cargo:rerun-if-changed={}", mcp.display());
        if mcp.is_file() {
            println!("cargo:rustc-cfg=embed_mcp");
            println!("cargo:rustc-env=FOMEG_MCP_EXE={}", mcp.display());
        }
    }
    let admin = release && std::env::var_os("FOMEG_NO_ADMIN").is_none();
    let manifest = if admin { include_str!("app.manifest") } else { include_str!("app-dev.manifest") };
    let attrs = tauri_build::Attributes::new()
        .windows_attributes(tauri_build::WindowsAttributes::new().app_manifest(manifest));
    tauri_build::try_build(attrs).expect("tauri-build thất bại");
}
