//! Puppeteer-compatible CLI REPL for headless Chrome automation.
//!
//! Usage:
//!   puppeteer                     # Start interactive REPL
//!   puppeteer --headful           # Start with visible browser
//!   puppeteer --port 9222         # Connect to Chrome on specific port
//!   puppeteer --url ws://...      # Connect to remote Chrome via WebSocket
//!
//! The REPL accepts Puppeteer-style JavaScript commands:
//!   > const page = await browser.newPage();
//!   > await page.goto('https://example.com');
//!   > await page.screenshot({path: 'shot.png'});
//!   > await browser.close();

mod cli;

use std::ffi::OsStr;

use anyhow::Result;
use clap::Parser;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;

use headless_chrome::{Browser, LaunchOptions};

use cli::executor::ExecutionContext;
use cli::parser;

#[derive(Parser, Debug)]
#[command(
    name = "puppeteer",
    about = "Puppeteer-compatible CLI for headless Chrome automation",
    long_about = "Interactive REPL that accepts Puppeteer-style JavaScript commands to control Chrome.\n\n\
    Examples:\n  \
    const page = await browser.newPage();\n  \
    await page.goto('https://example.com');\n  \
    await page.setViewport({width: 1080, height: 1024});\n  \
    await page.screenshot({path: 'screenshot.png'});\n  \
    await browser.close();"
)]
struct Args {
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

    /// Verbose output
    #[arg(short, long, default_value_t = false)]
    verbose: bool,

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

fn main() -> Result<()> {
    let args = Args::parse();

    let browser = create_browser(&args)?;
    let mut ctx = ExecutionContext::new(browser, args.verbose);

    // If --eval is specified, run a single command
    if let Some(ref eval_cmd) = args.eval {
        return run_commands(&mut ctx, std::slice::from_ref(eval_cmd));
    }

    // If --file is specified, run a script file
    if let Some(ref file) = args.file {
        let content = std::fs::read_to_string(file)?;
        let lines: Vec<String> = content.lines().map(String::from).collect();
        return run_commands(&mut ctx, &lines);
    }

    // Interactive REPL
    run_repl(&mut ctx)
}

fn create_browser(args: &Args) -> Result<Browser> {
    // Connect to existing Chrome via WebSocket URL
    if let Some(ref ws_url) = args.url {
        println!("Connecting to Chrome at {ws_url}...");
        return Browser::connect(ws_url.clone());
    }

    // Launch new Chrome
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

    println!("Launching Chrome{}...", if args.headful { " (headful)" } else { "" });
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

    println!("Puppeteer CLI (rust-headless-chrome)");
    println!("Type Puppeteer-style commands. Use Ctrl+D to exit.");
    println!("Example: const page = await browser.newPage();");
    println!();

    let history_path = dirs_history_path();
    if let Some(ref path) = history_path {
        let _ = rl.load_history(path);
    }

    loop {
        match rl.readline("puppeteer> ") {
            Ok(line) => {
                let line = line.trim().to_string();
                if line.is_empty() {
                    continue;
                }

                let _ = rl.add_history_entry(&line);

                // Handle special REPL commands
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

                match parser::parse_line(&line) {
                    Ok(cmd) => {
                        let is_close = matches!(cmd, parser::Command::BrowserClose);
                        match ctx.execute(&cmd) {
                            Ok(Some(output)) => println!("{output}"),
                            Ok(None) => {}
                            Err(e) => eprintln!("Error: {e}"),
                        }
                        if is_close {
                            break;
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

fn dirs_history_path() -> Option<String> {
    dirs_home().map(|home| format!("{home}/.puppeteer_cli_history"))
}

fn dirs_home() -> Option<String> {
    std::env::var("HOME").ok()
}

fn print_help() {
    println!("Available commands (Puppeteer-compatible):");
    println!();
    println!("  Browser:");
    println!("    const page = await browser.newPage()     Create a new page/tab");
    println!("    await browser.close()                     Close browser and exit");
    println!();
    println!("  Navigation:");
    println!("    await page.goto('url')                   Navigate to URL");
    println!("    await page.reload()                      Reload page");
    println!("    await page.goBack()                      Navigate back");
    println!("    await page.goForward()                   Navigate forward");
    println!("    page.url()                               Get current URL");
    println!("    page.title()                             Get page title");
    println!("    page.content()                           Get page HTML");
    println!();
    println!("  Viewport & Display:");
    println!("    await page.setViewport({{width: W, height: H}})");
    println!("    await page.setUserAgent('ua')");
    println!();
    println!("  Interaction:");
    println!("    await page.click('selector')             Click element");
    println!("    await page.type('selector', 'text')      Type into element");
    println!("    await page.keyboard.press('Enter')       Press a key");
    println!("    await page.keyboard.type('text')         Type text");
    println!("    await page.mouse.click(x, y)             Click at coordinates");
    println!("    await page.mouse.move(x, y)              Move mouse");
    println!();
    println!("  Locators (Puppeteer-style):");
    println!("    await page.locator('sel').click()        Click via locator");
    println!("    await page.locator('sel').fill('text')   Fill via locator");
    println!("    await page.locator('sel').waitHandle()   Wait for element");
    println!("    await page.waitForSelector('sel')        Wait for selector");
    println!();
    println!("  Evaluation:");
    println!("    await page.evaluate(() => expr)          Run JS expression");
    println!("    el?.evaluate(fn)                         Run JS on element");
    println!();
    println!("  Capture:");
    println!("    await page.screenshot({{path: 'f.png'}}) Save screenshot");
    println!("    await page.pdf({{path: 'f.pdf'}})        Save as PDF");
    println!();
    println!("  Output:");
    println!("    console.log('msg', var)                  Print to console");
    println!();
    println!("  REPL:");
    println!("    help / .help                             Show this help");
    println!("    vars / .vars                             Show stored variables");
    println!("    exit / .exit / Ctrl+D                    Exit REPL");
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
