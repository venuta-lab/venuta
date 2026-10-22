fn main() {
    tauri_build::build();
    // The dev probe in examples/ and the tests that build a Tauri app (the updater's, on the
    // mock runtime) are Tauri apps too, and Windows won't start one without the Common Controls 6
    // manifest the app's own binary gets from tauri-build.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        for target in ["examples", "tests"] {
            println!("cargo:rustc-link-arg-{target}=/MANIFEST:EMBED");
            println!(
                "cargo:rustc-link-arg-{target}=/MANIFESTDEPENDENCY:type='win32' \
                 name='Microsoft.Windows.Common-Controls' version='6.0.0.0' \
                 processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
            );
        }
    }
}
