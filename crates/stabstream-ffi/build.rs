fn main() {
    let crate_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let output_dir = format!("{crate_dir}/cbindgen-output");

    // Non-fatal: if cbindgen cannot parse the crate (e.g. during cross-compilation
    // bootstrap), we emit a warning and skip header generation rather than aborting.
    std::fs::create_dir_all(&output_dir).ok();

    let mut config = cbindgen::Config::default();
    config.language = cbindgen::Language::C;
    config.include_guard = Some("STABSTREAM_H".to_string());
    config.pragma_once = true;

    match cbindgen::Builder::new()
        .with_crate(&crate_dir)
        .with_config(config)
        .generate()
    {
        Ok(bindings) => {
            bindings.write_to_file(format!("{output_dir}/stabstream.h"));
        }
        Err(e) => {
            println!("cargo:warning=cbindgen header generation skipped: {e}");
        }
    }
}
