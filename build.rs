fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("CARGO_CFG_TARGET_OS")? == "windows" {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("./assets/icon.ico")
            .set("ProductName", "Neuro Karaoke Desktop")
            .set("FileDescription", "Neuro Karaoke Desktop")
            .set("CompanyName", "AverseMoon")
            .set("FileVersion", "0.1.0")
            .set("ProductVersion", "0.1.0")
            .set_version_info(winresource::VersionInfo::FILEVERSION, 0x0000000000010000)
            .set_version_info(winresource::VersionInfo::PRODUCTVERSION, 0x0000000000010000);
        res.compile()?;
    }

    Ok(())
}