use std::process::ExitCode;

use epic_manifest::Manifest;

const FILES_SHOWN: usize = 40;

fn main() -> ExitCode {
    // Colored output; RUST_LOG=debug shows the parser's internal steps.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug")).init();

    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: cargo run --example dump -- <path to .manifest>");
        return ExitCode::from(2);
    };

    match Manifest::from_file(&path) {
        Ok(manifest) => {
            print_summary(&manifest);
            ExitCode::SUCCESS
        }
        Err(error) => {
            log::error!("could not parse {path}: {error}");
            ExitCode::FAILURE
        }
    }
}

fn print_summary(manifest: &Manifest) {
    println!("app name:      {}", manifest.meta.app_name);
    println!("build version: {}", manifest.meta.build_version);
    println!("files:         {}", manifest.files.len());
    println!("total size:    {} bytes", manifest.total_size());

    println!("\nfirst {FILES_SHOWN} files:");
    for file in manifest.files.iter().take(FILES_SHOWN) {
        println!("  {:>12}  {}", file.file_size, file.filename);
    }
}
