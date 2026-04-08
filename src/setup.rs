/// CLI subcommands: --version, --setup, --download-model, --activate <KEY>.
///
/// Returns Some(exit_code) if a subcommand was handled (and main should exit),
/// or None if the server should run normally.

use crate::config::Config;
use std::io::Write;
use std::path::PathBuf;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Public model URL — EmbeddingGemma-300M Q8_0 GGUF (~313MB).
/// Hosted on Hugging Face's CDN.
const MODEL_URL: &str =
    "https://huggingface.co/google/embeddinggemma-300m-qat-q8_0-unquantized/resolve/main/embeddinggemma-300M-qat-q8_0-unquantized.gguf";
const MODEL_FILENAME: &str = "embeddinggemma-300m-qat-Q8_0.gguf";

pub fn handle_cli() -> Option<i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        return None;
    }

    match args[0].as_str() {
        "--version" | "-v" => {
            println!("haku {VERSION}");
            Some(0)
        }
        "--help" | "-h" => {
            print_help();
            Some(0)
        }
        "--setup" => {
            println!("Setting up Haku for first run...");
            ensure_dirs();
            if !model_exists() {
                println!("Embedding model not found.");
                if confirm("Download EmbeddingGemma-300M (~313MB) for semantic tool search?") {
                    if let Err(e) = download_model() {
                        eprintln!("Download failed: {e}");
                        return Some(1);
                    }
                }
            } else {
                println!("Model already present at {}", model_path().display());
            }
            println!("Setup complete. Run `haku` to start the server.");
            Some(0)
        }
        "--download-model" => {
            ensure_dirs();
            match download_model() {
                Ok(()) => Some(0),
                Err(e) => {
                    eprintln!("Download failed: {e}");
                    Some(1)
                }
            }
        }
        "--activate" => {
            let key = match args.get(1) {
                Some(k) => k.clone(),
                None => {
                    eprintln!("Usage: haku --activate <LICENSE_KEY>");
                    return Some(2);
                }
            };
            ensure_dirs();
            let config = Config::load_or_create();
            let mut license = crate::license::LicenseManager::new(&config);
            match license.activate(&key) {
                Ok(()) => {
                    println!("License activated successfully.");
                    Some(0)
                }
                Err(e) => {
                    eprintln!("Activation failed: {e}");
                    Some(1)
                }
            }
        }
        other => {
            eprintln!("Unknown argument: {other}");
            print_help();
            Some(2)
        }
    }
}

fn print_help() {
    println!(
        r#"haku {VERSION} — local MCP server for native + browser automation

USAGE:
    haku                       Start the server (MCP on 127.0.0.1:19820, WS on 19822)
    haku --version             Print version
    haku --help                Show this help
    haku --setup               First-run setup: create dirs, optionally download model
    haku --download-model      Download the EmbeddingGemma-300M GGUF model (~313MB)
    haku --activate <KEY>      Activate a license key

CONFIG:
    ~/.haku/config.json        Server config (port, token, license)
    ~/.haku/models/            Embedding model directory
    ~/.haku/flows.db           Recorded flow store (Windows: %LOCALAPPDATA%\Haku\)
"#
    );
}

fn confirm(prompt: &str) -> bool {
    print!("{prompt} [Y/n] ");
    let _ = std::io::stdout().flush();
    let mut input = String::new();
    if std::io::stdin().read_line(&mut input).is_err() {
        return false;
    }
    let answer = input.trim().to_lowercase();
    answer.is_empty() || answer == "y" || answer == "yes"
}

fn ensure_dirs() {
    let _ = std::fs::create_dir_all(Config::config_dir().join("models"));
}

fn model_path() -> PathBuf {
    Config::config_dir().join("models").join(MODEL_FILENAME)
}

fn model_exists() -> bool {
    model_path().exists()
}

fn download_model() -> Result<(), String> {
    let dest = model_path();
    println!("Downloading from: {MODEL_URL}");
    println!("Saving to: {}", dest.display());
    println!("(This is ~313MB and may take several minutes...)");

    let resp = ureq::get(MODEL_URL)
        .call()
        .map_err(|e| format!("HTTP error: {e}"))?;

    let total: Option<u64> = resp
        .headers()
        .get("Content-Length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok());

    let mut reader = resp.into_body().into_reader();
    let mut file = std::fs::File::create(&dest).map_err(|e| format!("Create file: {e}"))?;
    let mut buf = vec![0u8; 65536];
    let mut written: u64 = 0;
    let start = std::time::Instant::now();

    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("Read: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| format!("Write: {e}"))?;
        written += n as u64;

        if let Some(t) = total {
            let pct = (written as f64 / t as f64 * 100.0) as u32;
            print!("\r  {written} / {t} bytes ({pct}%)");
        } else {
            print!("\r  {written} bytes");
        }
        let _ = std::io::stdout().flush();
    }

    let elapsed = start.elapsed().as_secs_f64();
    println!(
        "\nDownloaded {} bytes in {:.1}s ({:.1} MB/s)",
        written,
        elapsed,
        written as f64 / 1_048_576.0 / elapsed
    );
    Ok(())
}

use std::io::Read;
