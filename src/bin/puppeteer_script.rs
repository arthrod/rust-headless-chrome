//! Script runner for Puppeteer-compatible scripts.
//!
//! Usage:
//!   puppeteer-script script.js                  # Run a Puppeteer-style script
//!   puppeteer-script script.js --headful        # Run with visible browser
//!   puppeteer-script -e "page.goto('url')"      # Run inline command(s)
//!
//! The script format is the same as Puppeteer JavaScript, one command per line:
//!   const page = await browser.newPage();
//!   await page.goto('https://example.com');
//!   await page.screenshot({path: 'shot.png'});
//!   await browser.close();

mod cli;

use std::ffi::OsStr;

use anyhow::Result;
use clap::Parser;

use headless_chrome::{Browser, LaunchOptions};

use cli::executor::ExecutionContext;
use cli::parser;

#[derive(Parser, Debug)]
#[command(
    name = "puppeteer-script",
    about = "Run Puppeteer-style scripts with headless Chrome",
    long_about = "Execute Puppeteer-compatible scripts for browser automation.\n\n\
    The script format mirrors Puppeteer JavaScript:\n  \
    const page = await browser.newPage();\n  \
    await page.goto('https://example.com');\n  \
    await page.screenshot({path: 'screenshot.png'});\n  \
    await browser.close();"
)]
struct Args {
    /// Script file to execute
    script: Option<String>,

    /// Run browser in headful (visible) mode
    #[arg(long, default_value_t = false)]
    headful: bool,

    /// Port number for Chrome debugging protocol
    #[arg(long)]
    port: Option<u16>,

    /// WebSocket URL to connect to an existing Chrome instance
    #[arg(long)]
    url: Option<String>,

    /// Window width
    #[arg(long, default_value_t = 1280)]
    width: u32,

    /// Window height
    #[arg(long, default_value_t = 720)]
    height: u32,

    /// Verbose output (show each command as it executes)
    #[arg(short, long, default_value_t = false)]
    verbose: bool,

    /// Execute inline command(s), semicolon-separated
    #[arg(short = 'e', long)]
    eval: Option<String>,

    /// Extra Chrome arguments
    #[arg(long = "chrome-arg", num_args = 1)]
    chrome_args: Vec<String>,

    /// Auto-create a page if the script doesn't start with browser.newPage()
    #[arg(long, default_value_t = true)]
    auto_page: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Determine the script content
    let lines = if let Some(ref eval_cmd) = args.eval {
        // Inline commands, split by semicolons
        eval_cmd
            .split(';')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
    } else if let Some(ref script) = args.script {
        let content = std::fs::read_to_string(script)
            .map_err(|e| anyhow::anyhow!("Failed to read script '{}': {}", script, e))?;
        content.lines().map(String::from).collect()
    } else {
        // Read from stdin
        eprintln!("Reading script from stdin...");
        let mut content = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut content)?;
        content.lines().map(String::from).collect()
    };

    let browser = create_browser(&args)?;
    let mut ctx = ExecutionContext::new(browser, args.verbose);

    // Auto-create page if needed
    if args.auto_page {
        let has_new_page = lines.iter().any(|l| l.contains("browser.newPage()"));
        if !has_new_page {
            if args.verbose {
                eprintln!("Auto-creating page...");
            }
            ctx.execute(&parser::Command::BrowserNewPage {
                assign_to: Some("page".to_string()),
            })?;
        }
    }

    // Execute each line
    let mut exit_requested = false;
    for (i, line) in lines.iter().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("//") || line.starts_with('#') {
            continue;
        }

        if args.verbose {
            eprintln!("[{:>3}] {}", i + 1, line);
        }

        match parser::parse_line(line) {
            Ok(cmd) => {
                if matches!(cmd, parser::Command::BrowserClose) {
                    exit_requested = true;
                    if args.verbose {
                        eprintln!("Browser close requested.");
                    }
                    break;
                }
                match ctx.execute(&cmd) {
                    Ok(Some(output)) => println!("{output}"),
                    Ok(None) => {}
                    Err(e) => {
                        eprintln!("Error at line {}: {e}", i + 1);
                        std::process::exit(1);
                    }
                }
            }
            Err(e) => {
                eprintln!("Parse error at line {}: {e}", i + 1);
                std::process::exit(1);
            }
        }
    }

    if !exit_requested && args.verbose {
        eprintln!("Script finished. Browser will close on exit.");
    }

    Ok(())
}

fn create_browser(args: &Args) -> Result<Browser> {
    if let Some(ref ws_url) = args.url {
        if args.verbose {
            eprintln!("Connecting to Chrome at {ws_url}...");
        }
        return Browser::connect(ws_url.clone());
    }

    let chrome_args: Vec<&OsStr> = args
        .chrome_args
        .iter()
        .map(|s| OsStr::new(s.as_str()))
        .collect();

    let launch_options = LaunchOptions::default_builder()
        .headless(!args.headful)
        .port(args.port)
        .window_size(Some((args.width, args.height)))
        .args(chrome_args)
        .build()?;

    if args.verbose {
        eprintln!(
            "Launching Chrome{}...",
            if args.headful { " (headful)" } else { "" }
        );
    }
    Browser::new(launch_options)
}
