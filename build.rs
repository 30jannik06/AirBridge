// Cargo runs build.rs BEFORE the actual compilation.
// Here we compile the C code of playfair (from UxPlay) and link it in statically.
fn main() {
    cc::Build::new()
        .files([
            "vendor/playfair/playfair.c",
            "vendor/playfair/hand_garble.c",
            "vendor/playfair/modified_md5.c",
            "vendor/playfair/omg_hax.c",
            "vendor/playfair/sap_hash.c",
            "vendor/fp_tables.c",
        ])
        .warnings(false) // third-party code, we do not maintain it
        .compile("playfair");

    println!("cargo:rerun-if-changed=vendor");

    // Embed icon and version info in the .exe (Explorer, taskbar, properties). Windows only.
    if std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows") {
        println!("cargo:rerun-if-changed=assets/airbridge.ico");
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/airbridge.ico");
        res.set("ProductName", "AirBridge");
        res.set(
            "FileDescription",
            "AirBridge - AirPlay receiver for Windows",
        );
        res.set("LegalCopyright", "GPL-3.0-only");
        if let Err(e) = res.compile() {
            // Without the resource compiler (rc.exe) there is just no file icon; the program itself still works.
            println!("cargo:warning=Icon konnte nicht eingebettet werden: {e}");
        }
    }
}
