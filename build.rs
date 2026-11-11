use std::io::Write;
use std::path::Path;

include!("src/icon_draw.rs");

const MANIFEST_ADMIN: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity version="1.0.0.0" processorArchitecture="amd64" name="GeoStoreUpdater" type="win32"/>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="__LEVEL__" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
    </windowsSettings>
  </application>
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
</assembly>
"#;

fn write_ico(path: &Path) {
    let sizes = [16u32, 24, 32, 48, 64, 128, 256];
    let mut images: Vec<Vec<u8>> = Vec::new();
    for &s in &sizes {
        let rgba = render_icon(s);
        let mut dib = Vec::new();
        let and_row = ((s + 31) / 32) * 4;
        dib.extend_from_slice(&40u32.to_le_bytes());
        dib.extend_from_slice(&(s as i32).to_le_bytes());
        dib.extend_from_slice(&((s * 2) as i32).to_le_bytes());
        dib.extend_from_slice(&1u16.to_le_bytes());
        dib.extend_from_slice(&32u16.to_le_bytes());
        dib.extend_from_slice(&0u32.to_le_bytes());
        dib.extend_from_slice(&(s * s * 4 + and_row * s).to_le_bytes());
        dib.extend_from_slice(&[0u8; 16]);
        for y in (0..s).rev() {
            for x in 0..s {
                let i = ((y * s + x) * 4) as usize;
                dib.extend_from_slice(&[rgba[i + 2], rgba[i + 1], rgba[i], rgba[i + 3]]);
            }
        }
        dib.extend(std::iter::repeat(0u8).take((and_row * s) as usize));
        images.push(dib);
    }
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (i, &s) in sizes.iter().enumerate() {
        let dim = if s >= 256 { 0u8 } else { s as u8 };
        out.extend_from_slice(&[dim, dim, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(images[i].len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += images[i].len() as u32;
    }
    for img in &images {
        out.extend_from_slice(img);
    }
    std::fs::File::create(path).unwrap().write_all(&out).unwrap();
}

fn main() {
    println!("cargo:rerun-if-changed=src/icon_draw.rs");
    println!("cargo:rerun-if-changed=build.rs");
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let ico = Path::new(&out_dir).join("app.ico");
    write_ico(&ico);

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // Debug builds stay asInvoker so they can be started from a normal shell during development.
        let level = if std::env::var("PROFILE").as_deref() == Ok("release") {
            "requireAdministrator"
        } else {
            "asInvoker"
        };
        let mut res = winresource::WindowsResource::new();
        res.set_icon(ico.to_str().unwrap());
        res.set_manifest(&MANIFEST_ADMIN.replace("__LEVEL__", level));
        res.set("FileDescription", "GeoStore Updater");
        res.set("ProductName", "GeoStore Updater");
        res.set("CompanyName", "");
        res.compile().expect("failed to compile Windows resources");
    }
}
