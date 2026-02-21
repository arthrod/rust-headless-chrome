//! `p` – Puppeteer CLI that loads Chrome launch config from ~/.cargo/p.json.
//!
//! Usage:
//!   p                     # Launch using settings from ~/.cargo/p.json
//!   p --headful           # Override headful mode
//!   p -e "page.goto(..)"  # One-shot command
//!
//! Config file (~/.cargo/p.json) example:
//! ```json
//! {
//!   "headless": false,
//!   "port": 9222,
//!   "width": 1920,
//!   "height": 1080,
//!   "sandbox": false,
//!   "verbose": true,
//!   "chrome_path": "/usr/bin/google-chrome-stable",
//!   "user_data_dir": "/tmp/chrome-profile",
//!   "proxy_server": "socks5://localhost:1080",
//!   "ignore_certificate_errors": true,
//!   "disable_default_args": false,
//!   "enable_gpu": false,
//!   "enable_logging": false,
//!   "devtools": false,
//!   "idle_browser_timeout_secs": 60,
//!   "chrome_args": ["--disable-extensions", "--no-first-run"],
//!   "process_envs": { "DISPLAY": ":1" }
//! }
//! ```

mod cli;

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::PathBuf;

use anyhow::{Result, Context};
use clap::Parser;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use serde::Deserialize;

use headless_chrome::{Browser, LaunchOptions};

use cli::executor::ExecutionContext;
use cli::parser;

/// JSON-serializable config loaded from ~/.cargo/p.json.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Config {
    headless: Option<bool>,
    sandbox: Option<bool>,
    devtools: Option<bool>,
    enable_gpu: Option<bool>,
    enable_logging: Option<bool>,
    width: Option<u32>,
    height: Option<u32>,
    port: Option<u16>,
    ignore_certificate_errors: Option<bool>,
    chrome_path: Option<String>,
    user_data_dir: Option<String>,
    disable_default_args: Option<bool>,
    idle_browser_timeout_secs: Option<u64>,
    proxy_server: Option<String>,
    chrome_args: Option<Vec<String>>,
    process_envs: Option<HashMap<String, String>>,
    verbose: Option<bool>,
    url: Option<String>,
}

#[derive(Parser, Debug)]
#[command(
    name = "p",
    about = "Puppeteer CLI with config from ~/.cargo/p.json",
    long_about = "Loads Chrome launch settings from ~/.cargo/p.json, then starts an interactive REPL.\n\
    CLI flags override config file values."
)]
struct Args {
    /// Run browser in headful (visible) mode
    #[arg(long)]
    headful: Option<bool>,

    /// Port number for Chrome debugging protocol
    #[arg(long)]
    port: Option<u16>,

    /// WebSocket URL to connect to an existing Chrome instance
    #[arg(long)]
    url: Option<String>,

    /// Window width
    #[arg(long)]
    width: Option<u32>,

    /// Window height
    #[arg(long)]
    height: Option<u32>,

    /// Verbose output
    #[arg(short, long)]
    verbose: Option<bool>,

    /// Execute a single command and exit
    #[arg(short = 'e', long)]
    eval: Option<String>,

    /// Execute a script file
    #[arg(short = 'f', long)]
    file: Option<String>,

    /// Extra Chrome arguments
    #[arg(long = "chrome-arg", num_args = 1)]
    chrome_args: Vec<String>,
}

fn config_path() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|home| PathBuf::from(home).join(".cargo").join("p.json"))
}

fn load_config() -> Config {
    let Some(path) = config_path() else {
        return Config::default();
    };
    match std::fs::read_to_string(&path) {
        Ok(content) => match serde_json::from_str::<Config>(&content) {
            Ok(cfg) => {
                eprintln!("Loaded config from {}", path.display());
                cfg
            }
            Err(e) => {
                eprintln!("Warning: failed to parse {}: {e}", path.display());
                Config::default()
            }
        },
        Err(_) => Config::default(),
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    let cfg = load_config();

    let browser = create_browser(&args, &cfg)?;
    let verbose = args.verbose.or(cfg.verbose).unwrap_or(false);
    let mut ctx = ExecutionContext::new(browser, verbose);

    if let Some(ref eval_cmd) = args.eval {
        return run_commands(&mut ctx, std::slice::from_ref(eval_cmd));
    }

    if let Some(ref file) = args.file {
        let content = std::fs::read_to_string(file)
            .with_context(|| format!("Failed to read script '{file}'"))?;
        let lines: Vec<String> = content.lines().map(String::from).collect();
        return run_commands(&mut ctx, &lines);
    }

    run_repl(&mut ctx)
}

fn create_browser(args: &Args, cfg: &Config) -> Result<Browser> {
    // WebSocket URL: CLI overrides config
    let ws_url = args.url.as_deref().or(cfg.url.as_deref());
    if let Some(ws_url) = ws_url {
        eprintln!("Connecting to Chrome at {ws_url}...");
        return Browser::connect(ws_url.to_string());
    }

    // Merge CLI args + config chrome_args
    let mut all_chrome_args: Vec<String> = cfg.chrome_args.clone().unwrap_or_default();
    all_chrome_args.extend(args.chrome_args.iter().cloned());
    let chrome_args_os: Vec<&OsStr> = all_chrome_args.iter().map(|s| OsStr::new(s.as_str())).collect();

    // Resolve each field: CLI flag > config > default
    let headless = match args.headful {
        Some(h) => !h,
        None => cfg.headless.unwrap_or(true),
    };
    let port = args.port.or(cfg.port);
    let width = args.width.or(cfg.width).unwrap_or(1280);
    let height = args.height.or(cfg.height).unwrap_or(720);
    let sandbox = cfg.sandbox.unwrap_or(true);
    let devtools = cfg.devtools.unwrap_or(false);
    let enable_gpu = cfg.enable_gpu.unwrap_or(false);
    let enable_logging = cfg.enable_logging.unwrap_or(false);
    let ignore_certificate_errors = cfg.ignore_certificate_errors.unwrap_or(true);
    let disable_default_args = cfg.disable_default_args.unwrap_or(false);
    let idle_browser_timeout = std::time::Duration::from_secs(
        cfg.idle_browser_timeout_secs.unwrap_or(30),
    );
    let path = cfg.chrome_path.as_ref().map(PathBuf::from);
    let user_data_dir = cfg.user_data_dir.as_ref().map(PathBuf::from);
    let process_envs = cfg.process_envs.clone();

    let launch_options = LaunchOptions {
        headless,
        sandbox,
        devtools,
        enable_gpu,
        enable_logging,
        window_size: Some((width, height)),
        port,
        ignore_certificate_errors,
        path,
        user_data_dir,
        disable_default_args,
        idle_browser_timeout,
        process_envs,
        args: chrome_args_os,
        extensions: Vec::new(),
        ignore_default_args: Vec::new(),
        proxy_server: cfg.proxy_server.as_deref(),
        #[cfg(feature = "fetch")]
        fetcher_options: Default::default(),
    };

    let mode = if headless { "" } else { " (headful)" };
    eprintln!("Launching Chrome{mode}...");
    Browser::new(launch_options)
}

fn run_commands(ctx: &mut ExecutionContext, lines: &[String]) -> Result<()> {
    for (i, line) in lines.iter().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("//") || line.starts_with('#') {
            continue;
        }

        match parser::parse_line(line) {
            Ok(cmd) => match ctx.execute(&cmd) {
                Ok(Some(output)) => println!("{output}"),
                Ok(None) => {}
                Err(e) => {
                    eprintln!("Error at line {}: {e}", i + 1);
                    return Err(e);
                }
            },
            Err(e) => {
                eprintln!("Parse error at line {}: {e}", i + 1);
                return Err(anyhow::anyhow!(e));
            }
        }
    }
    Ok(())
}

fn run_repl(ctx: &mut ExecutionContext) -> Result<()> {
    let mut rl = DefaultEditor::new()?;

    println!("p – Puppeteer CLI (config: ~/.cargo/p.json)");
    println!("Type Puppeteer-style commands. Use Ctrl+D to exit.");
    println!();

    let history_path = dirs_home().map(|home| format!("{home}/.puppeteer_cli_history"));
    if let Some(ref path) = history_path {
        let _ = rl.load_history(path);
    }

    loop {
        match rl.readline("p> ") {
            Ok(line) => {
                let line = line.trim().to_string();
                if line.is_empty() {
                    continue;
                }

                let _ = rl.add_history_entry(&line);

                if line == "exit" || line == "quit" || line == ".exit" {
                    break;
                }
                if line == "help" || line == ".help" {
                    print_help();
                    continue;
                }
                if line == "vars" || line == ".vars" {
                    print_vars(ctx);
                    continue;
                }
                if line == "config" || line == ".config" {
                    print_config();
                    continue;
                }

                match parser::parse_line(&line) {
                    Ok(cmd) => {
                        if matches!(cmd, parser::Command::BrowserClose) {
                            match ctx.execute(&cmd) {
                                Ok(Some(output)) => println!("{output}"),
                                Ok(None) => {}
                                Err(e) => eprintln!("Error: {e}"),
                            }
                            break;
                        }
                        match ctx.execute(&cmd) {
                            Ok(Some(output)) => println!("{output}"),
                            Ok(None) => {}
                            Err(e) => eprintln!("Error: {e}"),
                        }
                    }
                    Err(e) => eprintln!("Parse error: {e}"),
                }
            }
            Err(ReadlineError::Interrupted) => {
                println!("Ctrl+C pressed. Use Ctrl+D or 'exit' to quit.");
            }
            Err(ReadlineError::Eof) => {
                println!("Bye!");
                break;
            }
            Err(err) => {
                eprintln!("Readline error: {err}");
                break;
            }
        }
    }

    if let Some(ref path) = history_path {
        let _ = rl.save_history(path);
    }

    Ok(())
}

fn dirs_home() -> Option<String> {
    std::env::var("HOME").ok()
}

fn print_config() {
    match config_path() {
        Some(path) => match std::fs::read_to_string(&path) {
            Ok(content) => {
                println!("Config file: {}", path.display());
                println!("{content}");
            }
            Err(_) => println!("No config file found at {}", path.display()),
        },
        None => println!("Could not determine HOME directory."),
    }
}

fn print_help() {
    println!("p – Puppeteer CLI with config from ~/.cargo/p.json");
    println!();
    println!("  Browser:");
    println!("    const page = await browser.newPage()     Create a new page/tab");
    println!("    await browser.close()                     Close browser and exit");
    println!();
    println!("  Navigation:");
    println!("    await page.goto('url')                   Navigate to URL");
    println!("    await page.reload()                      Reload page");
    println!("    await page.goBack() / page.goForward()   History navigation");
    println!("    page.url() / page.title() / page.content()");
    println!();
    println!("  Interaction:");
    println!("    await page.click('selector')             Click element");
    println!("    await page.type('selector', 'text')      Type into element");
    println!("    await page.keyboard.press('Enter')       Press a key");
    println!("    await page.locator('sel').click()        Locator click");
    println!("    await page.locator('sel').fill('text')   Locator fill");
    println!();
    println!("  Capture:");
    println!("    await page.screenshot({{path: 'f.png'}}) Screenshot");
    println!("    await page.pdf({{path: 'f.pdf'}})        PDF");
    println!();
    println!("  REPL:");
    println!("    help / .help                             This help");
    println!("    vars / .vars                             Show variables");
    println!("    config / .config                         Show loaded config");
    println!("    exit / .exit / Ctrl+D                    Exit");
}

fn print_vars(ctx: &ExecutionContext) {
    if ctx.variables.is_empty() {
        println!("No variables stored.");
        return;
    }
    println!("Variables:");
    for (name, var) in &ctx.variables {
        match var {
            cli::executor::Variable::ElementSelector(sel) => {
                println!("  {name} = [Element: {sel}]");
            }
            cli::executor::Variable::StringValue(s) => {
                let display = if s.len() > 80 {
                    format!("{}...", &s[..80])
                } else {
                    s.clone()
                };
                println!("  {name} = \"{display}\"");
            }
            cli::executor::Variable::JsonValue(v) => {
                println!("  {name} = {v}");
            }
        }
    }
}
