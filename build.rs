use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    // Tell Cargo to recognize the 'production' cfg
    println!("cargo::rustc-check-cfg=cfg(production)");
    // Get current date
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();

    // Get git hash (short form)
    let git_hash = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                String::from_utf8(output.stdout).ok()
            } else {
                None
            }
        })
        .unwrap_or_else(|| "unknown".to_string())
        .trim()
        .to_string();

    // Set environment variables for compile time
    println!("cargo:rustc-env=BUILD_DATE={}", date);
    println!("cargo:rustc-env=GIT_HASH={}", git_hash);

    // Detect if we're building in release mode
    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());
    if profile == "release" {
        println!("cargo:rustc-cfg=production");

        // Add static-libgcc only for release builds to create portable executables
        if cfg!(target_os = "linux") {
            println!("cargo:rustc-link-arg=-static-libgcc");
        }
    }

    // Handle Windows cross-compilation with SDL2_ttf
    let target = env::var("TARGET").unwrap_or_default();
    if target.contains("windows") {
        handle_windows_sdl2_ttf();
    }

    // Rerun if git HEAD changes
    println!("cargo:rerun-if-changed=../.git/HEAD");
}

fn handle_windows_sdl2_ttf() {
    // For Windows cross-compilation, we need to provide SDL2_ttf
    // The bundled feature in sdl2 crate only handles SDL2, not SDL2_ttf

    // Check for SDL2_LIB_DIR environment variable (set by our scripts)
    if let Ok(sdl2_lib_dir) = env::var("SDL2_LIB_DIR") {
        let sdl2_ttf_path = PathBuf::from(&sdl2_lib_dir).join("libSDL2_ttf.a");
        if sdl2_ttf_path.exists() {
            println!("cargo:rustc-link-search=native={}", sdl2_lib_dir);
            eprintln!("Using SDL2_ttf from: {}", sdl2_lib_dir);
            return;
        }
    }

    // Check if SDL2_ttf is available in mingw system libraries
    if Command::new("x86_64-w64-mingw32-gcc")
        .args(["-lSDL2_ttf", "-E", "-"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        // System has SDL2_ttf, no extra work needed
        return;
    }

    // Try to find SDL2_ttf in common mingw locations
    let mingw_paths = vec![
        "/usr/x86_64-w64-mingw32/lib",
        "/usr/lib/gcc/x86_64-w64-mingw32",
        "/usr/x86_64-w64-mingw32/sys-root/mingw/lib",
    ];

    for path in mingw_paths {
        let sdl2_ttf_path = PathBuf::from(path).join("libSDL2_ttf.a");
        if sdl2_ttf_path.exists() {
            println!("cargo:rustc-link-search=native={}", path);
            return;
        }
    }

    // If we get here, SDL2_ttf is not available
    eprintln!("Warning: SDL2_ttf not found for Windows cross-compilation.");
    eprintln!("Install with:");
    eprintln!("  Ubuntu/Debian: sudo apt install libsdl2-ttf-mingw-w64-dev");
    eprintln!("  Or download from: https://github.com/libsdl-org/SDL_ttf/releases");
    eprintln!("  Or run: ./scripts/download-sdl2-windows.sh");
    eprintln!();
    eprintln!("Alternatively, you can disable the TTF feature by removing 'ttf' from sdl2 features in Cargo.toml");
}
