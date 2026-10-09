use std::fs;
use std::io::{BufWriter, Write};
use std::path::Path;

/// The lazy Noto fallback fonts embedded gzip-compressed by `src/app/fonts.rs`.
const LAZY_FONT_FILES: [&str; 4] = [
    "NotoSansJP-Regular.ttf",
    "NotoSansKR-Regular.ttf",
    "NotoSansSC-Regular.ttf",
    "NotoSansCuneiform-Regular.ttf",
];

/// Compress each lazy font to `<name>.ttf.gz` next to its `.ttf` source, at the
/// highest gzip level available (`Compression::best()` = level 9).
///
/// Results are cached in place: a `.ttf.gz` that is newer than its `.ttf` is
/// reused as-is, so incremental builds don't recompress. Both the `.ttf` source
/// and the generated `.gz` are watched, so a font swap recompresses and a
/// deleted `.gz` is regenerated; the mtime cache keeps those re-runs cheap.
fn compress_fonts() -> Result<(), Box<dyn std::error::Error>> {
    let fonts_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/fonts");

    for name in LAZY_FONT_FILES {
        // Watch the source so a font swap recompresses, and watch the generated
        // archive so a deleted `.gz` is regenerated. Writing the `.gz` makes
        // cargo re-run this script each build, but the mtime cache below makes
        // those re-runs a no-op — no recompression loop.
        println!("cargo::rerun-if-changed=assets/fonts/{name}");
        println!("cargo::rerun-if-changed=assets/fonts/{name}.gz");

        let src = fonts_dir.join(name);
        let dst = fonts_dir.join(format!("{name}.gz"));

        // Cache: reuse the existing archive when it is newer than the source.
        if let (Ok(src_meta), Ok(dst_meta)) = (fs::metadata(&src), fs::metadata(&dst)) {
            if dst_meta.modified()? > src_meta.modified()? {
                continue;
            }
        }

        let bytes = fs::read(&src)?;
        let mut encoder = flate2::GzBuilder::new()
            .filename(name)
            .write(BufWriter::new(fs::File::create(&dst)?), flate2::Compression::best());
        encoder.write_all(&bytes)?;
        encoder.finish()?;

        println!(
            "cargo::warning=Compressed {name} ({} bytes) to {} ({} bytes)",
            bytes.len(),
            dst.display(),
            fs::metadata(&dst)?.len()
        );
    }

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    compress_fonts()?;

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
