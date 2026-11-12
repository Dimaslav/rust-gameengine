fn main() {
    #[cfg(windows)]
    windows_resources();
}

#[cfg(windows)]
fn windows_resources() {
    use std::env;

    let mut res = winres::WindowsResource::new();

    // Иконка — опционально. Если файла нет, соберётся с дефолтной.
    let icon_path = std::path::Path::new("assets/icon.ico");
    if icon_path.exists() {
        res.set_icon(icon_path.to_str().unwrap());
        println!("cargo:rerun-if-changed=assets/icon.ico");
    } else {
        println!("cargo:warning=assets/icon.ico not found, exe will use default icon");
    }

    // Метаданные (видны в Свойствах → Подробно).
    let version = env!("CARGO_PKG_VERSION");
    let name = env!("CARGO_PKG_NAME");
    let description = env::var("CARGO_PKG_DESCRIPTION").unwrap_or_default();
    let authors = env::var("CARGO_PKG_AUTHORS").unwrap_or_default();

    res.set("FileDescription", &description);
    res.set("ProductName", "Rust Engine 3D");
    res.set("CompanyName", &authors);
    res.set("LegalCopyright", &format!("© {} 2026", authors));
    res.set("FileVersion", version);
    res.set("ProductVersion", version);
    res.set("OriginalFilename", &format!("{}.exe", name));
    res.set("InternalName", name);

    // Manifest: DPI-awareness + long paths + supported OS.
    res.set_manifest(
        r#"
<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity type="win32" name="RustEngine3D" version="1.0.0.0"
                    processorArchitecture="amd64"/>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2, PerMonitor</dpiAwareness>
      <longPathAware xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">true</longPathAware>
      <activeCodePage xmlns="http://schemas.microsoft.com/SMI/2019/WindowsSettings">UTF-8</activeCodePage>
    </windowsSettings>
  </application>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
</assembly>"#,
    );

    if let Err(e) = res.compile() {
        panic!(
            "\n\n=== winres compile failed ===\n{}\n\n\
             Убедись, что установлен Windows SDK (идёт с MSVC Build Tools).\n\
             Проверь: `where rc.exe` в Developer Command Prompt.\n",
            e
        );
    }
}