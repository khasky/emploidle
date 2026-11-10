fn main() {
    // GPUI loads the window icon from resource 1; process_name clears VERSIONINFO 1 from copies.
    #[cfg(target_os = "windows")]
    winresource::WindowsResource::new()
        .set_icon_with_id("icons/icon.ico", "1")
        .set("ProductName", "Emploidle")
        .set("FileDescription", "Emploidle")
        .set("CompanyName", "Khasky")
        .set("LegalCopyright", "Copyright (c) 2026 Khasky")
        .compile()
        .expect("cannot embed Windows resources");
}
