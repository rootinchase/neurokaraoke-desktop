fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("CARGO_CFG_TARGET_OS")? == "windows" {
        // 1. Fetch version string components automatically provided by Cargo
        let version = env!("CARGO_PKG_VERSION");
        let major: u64 = env!("CARGO_PKG_VERSION_MAJOR").parse().unwrap_or(0);
        let minor: u64 = env!("CARGO_PKG_VERSION_MINOR").parse().unwrap_or(0);
        let patch: u64 = env!("CARGO_PKG_VERSION_PATCH").parse().unwrap_or(0);

        // 2. Convert major, minor, patch, and revision (0) into the required u64 bit pattern
        // This replaces the winresource::v4!() macro with its underlying raw calculation
        let numeric_version = (major << 48) | (minor << 32) | (patch << 16) | 0;

        let mut res = winresource::WindowsResource::new();
        res.set_icon("./assets/icon.ico")
            .set("ProductName", "Neuro Karaoke Desktop")
            .set("FileDescription", "Neuro Karaoke Desktop")
            .set("CompanyName", "AverseMoon")
            .set("FileVersion", version)
            .set("ProductVersion", version)
            // 3. Pass the fully dynamic numeric version bits
            .set_version_info(winresource::VersionInfo::FILEVERSION, numeric_version)
            .set_version_info(winresource::VersionInfo::PRODUCTVERSION, numeric_version);
        res.compile()?;
    }

    Ok(())
}
