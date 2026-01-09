//! Command-line interface handling for the terminal emulator.
//!
//! This module handles parsing of command-line arguments including:
//! - Help and version information display
//! - Test server port configuration
//! - Early exit for non-GUI modes

/// CLI arguments parsed from command line
#[derive(Debug)]
pub struct CliArgs {
    /// Port number for test server (if enabled)
    pub test_port: Option<u16>,
}

/// Parse command line arguments and handle help/version flags.
///
/// This function will exit the process if --help or --version flags are provided.
///
/// # Arguments
/// * `build_date` - Build date string for version display
/// * `git_hash` - Git hash string for version display
///
/// # Returns
/// Parsed CLI arguments
pub fn parse_args(build_date: &str, git_hash: &str) -> CliArgs {
    let args: Vec<String> = std::env::args().collect();
    let mut test_port: Option<u16> = None;

    // Handle --help and --version before initializing SDL
    for arg in args.iter().skip(1) {
        if arg == "--help" || arg == "-h" {
            print_help(build_date, git_hash);
            std::process::exit(0);
        } else if arg == "--version" || arg == "-v" {
            print_version(build_date, git_hash);
            std::process::exit(0);
        }
    }

    // Parse --test-port argument
    for (i, arg) in args.iter().enumerate() {
        if arg == "--test-port" && i + 1 < args.len() {
            if let Ok(port) = args[i + 1].parse::<u16>() {
                test_port = Some(port);
                eprintln!("[CLI] Test server will be enabled on port {}", port);
            }
        }
    }

    CliArgs { test_port }
}

/// Print help information and usage
fn print_help(build_date: &str, git_hash: &str) {
    println!("Nisdos Terminal v{} ({}, built {})", env!("CARGO_PKG_VERSION"), git_hash, build_date);
    println!();
    println!("USAGE:");
    println!("    nist [OPTIONS]");
    println!();
    println!("OPTIONS:");
    println!("    -h, --help          Print help information");
    println!("    -v, --version       Print version information");
    println!("    --test-port <PORT>  Enable test server on specified port");
}

/// Print version information
fn print_version(build_date: &str, git_hash: &str) {
    println!("Nisdos Terminal {} ({}, built {})", env!("CARGO_PKG_VERSION"), git_hash, build_date);
}
