fn main() {
    #[cfg(feature = "clippy")]
    {
        println!("cargo:warning=Skipping tauri_build during Clippy");
        // Манифест приложения кладёт tauri_build, а без него тестовый exe на
        // Windows грузит старую comctl32 без TaskDialogIndirect и не стартует
        // вовсе (STATUS_ENTRYPOINT_NOT_FOUND).
        if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
            println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
            println!(
                "cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
            );
        }
    }

    #[cfg(not(feature = "clippy"))]
    tauri_build::build();
}
