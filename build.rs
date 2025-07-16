use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=crates/wasi-modules");

    // Check if modules already exist
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let modules_dir = PathBuf::from(&manifest_dir).join("modules");

    let all_modules_exist = [
        "ante_handler.wasm",
        "begin_blocker.wasm",
        "end_blocker.wasm",
        "tx_decoder.wasm",
    ]
    .iter()
    .all(|module| modules_dir.join(module).exists());

    // Build WASI modules if they don't exist or if explicitly requested
    if !all_modules_exist || env::var("BUILD_WASI_MODULES").is_ok() {
        build_wasi_modules();
    }
}

fn build_wasi_modules() {
    println!("cargo:rerun-if-changed=crates/wasi-modules");

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let workspace_root = PathBuf::from(manifest_dir);

    // Ensure we have the wasm32-wasi target
    let output = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .expect("Failed to check rustup targets");

    let installed_targets = String::from_utf8_lossy(&output.stdout);
    if !installed_targets.contains("wasm32-wasip1") {
        println!("cargo:warning=Installing wasm32-wasip1 target...");
        let status = Command::new("rustup")
            .args(["target", "add", "wasm32-wasip1"])
            .status()
            .expect("Failed to install wasm32-wasip1 target");

        if !status.success() {
            panic!("Failed to install wasm32-wasip1 target");
        }
    }

    // Create modules directory
    let modules_dir = workspace_root.join("modules");
    std::fs::create_dir_all(&modules_dir).expect("Failed to create modules directory");

    // Build each WASI module
    let wasi_modules = [
        ("ante-handler", "wasi_ante_handler"),
        ("begin-blocker", "begin_blocker"),
        ("end-blocker", "end_blocker"),
        ("tx-decoder", "tx_decoder"),
    ];

    for (module_name, crate_name) in &wasi_modules {
        println!("cargo:warning=Building WASI module: {module_name}");

        let module_path = workspace_root
            .join("crates")
            .join("wasi-modules")
            .join(module_name);

        // Build the module
        let status = Command::new("cargo")
            .current_dir(&module_path)
            .args(["build", "--target", "wasm32-wasip1", "--release"])
            .status()
            .unwrap_or_else(|_| panic!("Failed to build {module_name} module"));

        if !status.success() {
            panic!("Failed to build {module_name} module");
        }

        // Copy the built module to the modules directory
        let wasm_name = format!("{crate_name}.wasm");
        let lib_wasm_name = format!("lib{crate_name}.wasm");

        let target_dir = workspace_root
            .join("target")
            .join("wasm32-wasip1")
            .join("release");
        let mut source_path = target_dir.join(&wasm_name);

        // Try alternative name if first doesn't exist
        if !source_path.exists() {
            source_path = target_dir.join(&lib_wasm_name);
        }

        if source_path.exists() {
            let dest_name = format!("{}.wasm", module_name.replace("-", "_"));
            let dest_path = modules_dir.join(&dest_name);

            std::fs::copy(&source_path, &dest_path)
                .unwrap_or_else(|_| panic!("Failed to copy {module_name} module"));

            println!(
                "cargo:warning=Copied {} to {}",
                source_path.display(),
                dest_path.display()
            );
        } else {
            println!("cargo:warning=Warning: Could not find compiled WASM file for {module_name}");
        }
    }

    println!("cargo:warning=WASI modules built successfully!");
}
