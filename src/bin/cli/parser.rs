/// Parser for Puppeteer-like command syntax.
///
/// Supports commands like:
///   browser.newPage()
///   page.goto('https://example.com')
///   page.setViewport({width: 1080, height: 1024})
///   page.keyboard.press('Enter')
///   page.locator('.selector').click()
///   page.locator('.selector').fill('text')
///   page.locator('::-p-text(some text)').waitHandle()
///   page.waitForSelector('.selector')
///   page.type('.selector', 'text')
///   page.click('.selector')
///   page.evaluate(() => document.title)
///   page.screenshot({path: 'file.png'})
///   page.content()
///   page.title()
///   page.url()
///   page.pdf({path: 'file.pdf'})
///   console.log('message')
///   browser.close()
///
/// Also supports variable assignment:
///   const page = await browser.newPage();
///   await page.goto('url');

#[derive(Debug, Clone)]
pub enum Command {
    /// browser.newPage()
    BrowserNewPage {
        /// Optional variable name to assign the page to (e.g., `const page = ...`)
        assign_to: Option<String>,
    },
    /// browser.close()
    BrowserClose,
    /// page.goto(url)
    PageGoto {
        url: String,
    },
    /// page.setViewport({width, height})
    PageSetViewport {
        width: u32,
        height: u32,
    },
    /// page.keyboard.press(key)
    PageKeyboardPress {
        key: String,
    },
    /// page.keyboard.type(text)
    PageKeyboardType {
        text: String,
    },
    /// page.locator(selector).click()
    PageLocatorClick {
        selector: String,
    },
    /// page.locator(selector).fill(text)
    PageLocatorFill {
        selector: String,
        text: String,
    },
    /// page.locator(selector).waitHandle() - optionally with evaluate
    PageLocatorWaitHandle {
        selector: String,
        assign_to: Option<String>,
    },
    /// page.waitForSelector(selector)
    PageWaitForSelector {
        selector: String,
        assign_to: Option<String>,
    },
    /// page.type(selector, text)
    PageType {
        selector: String,
        text: String,
    },
    /// page.click(selector)
    PageClick {
        selector: String,
    },
    /// page.evaluate(expression)
    PageEvaluate {
        expression: String,
        assign_to: Option<String>,
    },
    /// element.evaluate(fn) -- e.g., textSelector?.evaluate(el => el.textContent)
    ElementEvaluate {
        variable: String,
        js_fn: String,
        assign_to: Option<String>,
    },
    /// page.screenshot({path: 'file.png'})
    PageScreenshot {
        path: Option<String>,
    },
    /// page.content()
    PageContent {
        assign_to: Option<String>,
    },
    /// page.title()
    PageTitle {
        assign_to: Option<String>,
    },
    /// page.url()
    PageUrl {
        assign_to: Option<String>,
    },
    /// page.pdf({path: 'file.pdf'})
    PagePdf {
        path: Option<String>,
    },
    /// console.log(expression)
    ConsoleLog {
        expression: String,
    },
    /// page.mouse.click(x, y)
    PageMouseClick {
        x: f64,
        y: f64,
    },
    /// page.mouse.move(x, y)
    PageMouseMove {
        x: f64,
        y: f64,
    },
    /// page.setUserAgent(ua)
    PageSetUserAgent {
        user_agent: String,
    },
    /// page.goBack()
    PageGoBack,
    /// page.goForward()
    PageGoForward,
    /// page.reload()
    PageReload,
    /// page.setCookie(...cookies)
    PageSetCookie {
        cookies_json: String,
    },
    /// page.cookies()
    PageCookies {
        assign_to: Option<String>,
    },
    /// Empty line or comment
    Noop,
}

pub fn parse_line(line: &str) -> Result<Command, String> {
    let line = line.trim();

    // Skip empty lines and comments
    if line.is_empty() || line.starts_with("//") || line.starts_with('#') {
        return Ok(Command::Noop);
    }

    // Strip trailing semicolons
    let line = line.trim_end_matches(';').trim();

    // Check for variable assignment: const/let/var name = await ...
    let (assign_to, line) = parse_assignment(line);

    // Strip leading `await` keyword
    let line = strip_await(line);

    // Now parse the actual command
    parse_command(line, assign_to)
}

/// Extract variable assignment prefix: `const foo = `, `let bar = `, `var baz = `
fn parse_assignment(line: &str) -> (Option<String>, &str) {
    for keyword in &["const ", "let ", "var "] {
        if let Some(rest) = line.strip_prefix(keyword) {
            if let Some(eq_pos) = rest.find('=') {
                let var_name = rest[..eq_pos].trim().to_string();
                let rest = rest[eq_pos + 1..].trim();
                return (Some(var_name), rest);
            }
        }
    }
    (None, line)
}

fn strip_await(line: &str) -> &str {
    line.strip_prefix("await ")
        .unwrap_or(line)
        .trim()
}

fn parse_command(line: &str, assign_to: Option<String>) -> Result<Command, String> {
    // console.log(...)
    if let Some(args) = strip_method_call(line, "console.log") {
        return Ok(Command::ConsoleLog {
            expression: args.to_string(),
        });
    }

    // browser.newPage()
    if line == "browser.newPage()" {
        return Ok(Command::BrowserNewPage { assign_to });
    }

    // browser.close()
    if line == "browser.close()" {
        return Ok(Command::BrowserClose);
    }

    // page.goto(url)
    if let Some(args) = strip_method_call(line, "page.goto") {
        let url = extract_string_arg(args)?;
        return Ok(Command::PageGoto { url });
    }

    // page.setViewport({width: W, height: H})
    if let Some(args) = strip_method_call(line, "page.setViewport") {
        let (width, height) = parse_viewport_args(args)?;
        return Ok(Command::PageSetViewport { width, height });
    }

    // page.keyboard.press(key)
    if let Some(args) = strip_method_call(line, "page.keyboard.press") {
        let key = extract_string_arg(args)?;
        return Ok(Command::PageKeyboardPress { key });
    }

    // page.keyboard.type(text)
    if let Some(args) = strip_method_call(line, "page.keyboard.type") {
        let text = extract_string_arg(args)?;
        return Ok(Command::PageKeyboardType { text });
    }

    // page.locator(selector).fill(text)
    if let Some((selector, chain)) = parse_locator_chain(line) {
        if let Some(args) = strip_method_call(chain, ".fill") {
            let text = extract_string_arg(args)?;
            return Ok(Command::PageLocatorFill {
                selector,
                text,
            });
        }
        if chain == ".click()" {
            return Ok(Command::PageLocatorClick { selector });
        }
        if chain == ".waitHandle()" {
            return Ok(Command::PageLocatorWaitHandle {
                selector,
                assign_to,
            });
        }
    }

    // page.waitForSelector(selector)
    if let Some(args) = strip_method_call(line, "page.waitForSelector") {
        let selector = extract_string_arg(args)?;
        return Ok(Command::PageWaitForSelector {
            selector,
            assign_to,
        });
    }

    // page.type(selector, text)
    if let Some(args) = strip_method_call(line, "page.type") {
        let (selector, text) = extract_two_string_args(args)?;
        return Ok(Command::PageType { selector, text });
    }

    // page.click(selector)
    if let Some(args) = strip_method_call(line, "page.click") {
        let selector = extract_string_arg(args)?;
        return Ok(Command::PageClick { selector });
    }

    // page.evaluate(...)
    if let Some(args) = strip_method_call(line, "page.evaluate") {
        return Ok(Command::PageEvaluate {
            expression: args.to_string(),
            assign_to,
        });
    }

    // page.screenshot({path: 'file.png'})
    if let Some(args) = strip_method_call(line, "page.screenshot") {
        let path = parse_path_from_options(args);
        return Ok(Command::PageScreenshot { path });
    }

    // page.content()
    if line == "page.content()" {
        return Ok(Command::PageContent { assign_to });
    }

    // page.title()
    if line == "page.title()" {
        return Ok(Command::PageTitle { assign_to });
    }

    // page.url()
    if line == "page.url()" {
        return Ok(Command::PageUrl { assign_to });
    }

    // page.pdf({path: 'file.pdf'})
    if let Some(args) = strip_method_call(line, "page.pdf") {
        let path = parse_path_from_options(args);
        return Ok(Command::PagePdf { path });
    }

    // page.mouse.click(x, y)
    if let Some(args) = strip_method_call(line, "page.mouse.click") {
        let (x, y) = parse_two_numbers(args)?;
        return Ok(Command::PageMouseClick { x, y });
    }

    // page.mouse.move(x, y)
    if let Some(args) = strip_method_call(line, "page.mouse.move") {
        let (x, y) = parse_two_numbers(args)?;
        return Ok(Command::PageMouseMove { x, y });
    }

    // page.setUserAgent(ua)
    if let Some(args) = strip_method_call(line, "page.setUserAgent") {
        let user_agent = extract_string_arg(args)?;
        return Ok(Command::PageSetUserAgent { user_agent });
    }

    // page.goBack()
    if line == "page.goBack()" {
        return Ok(Command::PageGoBack);
    }

    // page.goForward()
    if line == "page.goForward()" {
        return Ok(Command::PageGoForward);
    }

    // page.reload()
    if line == "page.reload()" {
        return Ok(Command::PageReload);
    }

    // page.setCookie(...)
    if let Some(args) = strip_method_call(line, "page.setCookie") {
        return Ok(Command::PageSetCookie {
            cookies_json: args.to_string(),
        });
    }

    // page.cookies()
    if line == "page.cookies()" {
        return Ok(Command::PageCookies { assign_to });
    }

    // <variable>?.evaluate(fn) or <variable>.evaluate(fn)
    // e.g.: textSelector?.evaluate(el => el.textContent)
    //       fullTitle?.evaluate(el => el.textContent)
    if let Some(cmd) = try_parse_element_evaluate(line, assign_to.clone()) {
        return Ok(cmd);
    }

    Err(format!("Unknown command: {line}"))
}

/// Try to parse `varName?.evaluate(fn)` or `varName.evaluate(fn)`
fn try_parse_element_evaluate(line: &str, assign_to: Option<String>) -> Option<Command> {
    // Match patterns like: variable?.evaluate(...) or variable.evaluate(...)
    let eval_patterns = ["?.evaluate(", ".evaluate("];

    for pat in &eval_patterns {
        if let Some(pos) = line.find(pat) {
            let variable = line[..pos].trim().to_string();
            // variable must be a simple identifier
            if variable.chars().all(|c| c.is_alphanumeric() || c == '_') && !variable.is_empty() {
                let rest = &line[pos + pat.len()..];
                // rest should end with )
                if let Some(js_fn) = rest.strip_suffix(')') {
                    return Some(Command::ElementEvaluate {
                        variable,
                        js_fn: js_fn.to_string(),
                        assign_to,
                    });
                }
            }
        }
    }
    None
}

/// Strip `method_name(` prefix and `)` suffix, returning the inner arguments
fn strip_method_call<'a>(line: &'a str, method: &str) -> Option<&'a str> {
    let prefix = format!("{method}(");
    if let Some(rest) = line.strip_prefix(&prefix) {
        if let Some(args) = rest.strip_suffix(')') {
            return Some(args);
        }
    }
    None
}

/// Parse `page.locator(selector)` chain: returns (selector, remaining_chain)
fn parse_locator_chain(line: &str) -> Option<(String, &str)> {
    let prefix = "page.locator(";
    if !line.starts_with(prefix) {
        // Also try: page\n  .locator(
        return None;
    }
    let rest = &line[prefix.len()..];

    // Find matching closing paren for the selector argument
    // The selector is a string arg, so find its end
    let selector_end = find_string_arg_end(rest)?;
    let selector = extract_string_from(rest, selector_end)?;
    let remaining = rest[selector_end..].trim_start();
    let remaining = remaining.strip_prefix(')')?;
    let remaining = remaining.trim_start();
    Some((selector, remaining))
}

/// Find the end of a string argument (including the closing quote)
fn find_string_arg_end(s: &str) -> Option<usize> {
    let s = s.trim();
    let quote = s.chars().next()?;
    if quote != '\'' && quote != '"' && quote != '`' {
        return None;
    }
    let mut i = 1;
    let bytes = s.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2;
            continue;
        }
        if bytes[i] == quote as u8 {
            return Some(i + 1);
        }
        i += 1;
    }
    None
}

/// Extract the string content from the beginning of `s` up to `end`
fn extract_string_from(s: &str, end: usize) -> Option<String> {
    let s = s.trim();
    if end < 2 {
        return None;
    }
    Some(s[1..end - 1].to_string())
}

/// Extract a single string argument from something like `'hello'` or `"hello"`.
/// Handles escaped quotes (e.g., `'it\'s'`).
fn extract_string_arg(args: &str) -> Result<String, String> {
    let args = args.trim();
    if args.is_empty() {
        return Err("Expected a string argument".to_string());
    }

    let quote = args.chars().next().unwrap();
    if quote != '\'' && quote != '"' && quote != '`' {
        return Err(format!("Expected string argument, got: {args}"));
    }

    let end = find_string_arg_end(args)
        .ok_or_else(|| format!("Unterminated string: {args}"))?;

    Ok(args[1..end - 1].to_string())
}

/// Extract two string arguments: ('arg1', 'arg2')
fn extract_two_string_args(args: &str) -> Result<(String, String), String> {
    let args = args.trim();
    let first = extract_string_arg(args)?;
    // Use escape-aware end finding to skip past the first string
    let after_first = find_string_arg_end(args)
        .ok_or_else(|| format!("Unterminated string: {args}"))?;
    let rest = args[after_first..].trim();
    let rest = rest.strip_prefix(',').ok_or("Expected comma between arguments")?;
    let second = extract_string_arg(rest.trim())?;
    Ok((first, second))
}

/// Parse {width: W, height: H} style viewport arguments
fn parse_viewport_args(args: &str) -> Result<(u32, u32), String> {
    let args = args.trim();
    // Strip outer braces
    let inner = args
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
        .ok_or_else(|| format!("Expected {{width: N, height: N}}, got: {args}"))?;

    let mut width = None;
    let mut height = None;

    for part in inner.split(',') {
        let part = part.trim();
        if let Some(val) = part.strip_prefix("width:").or_else(|| part.strip_prefix("width :")) {
            width = Some(
                val.trim()
                    .parse::<u32>()
                    .map_err(|e| format!("Invalid width: {e}"))?,
            );
        } else if let Some(val) = part.strip_prefix("height:").or_else(|| part.strip_prefix("height :")) {
            height = Some(
                val.trim()
                    .parse::<u32>()
                    .map_err(|e| format!("Invalid height: {e}"))?,
            );
        }
    }

    Ok((
        width.ok_or("Missing width in viewport")?,
        height.ok_or("Missing height in viewport")?,
    ))
}

/// Parse {path: 'file.png'} style options
fn parse_path_from_options(args: &str) -> Option<String> {
    let args = args.trim();
    if args.is_empty() {
        return None;
    }
    let inner = args.strip_prefix('{')?.strip_suffix('}')?;
    for part in inner.split(',') {
        let part = part.trim();
        if let Some(val) = part.strip_prefix("path:").or_else(|| part.strip_prefix("path :")) {
            return extract_string_arg(val.trim()).ok();
        }
    }
    None
}

/// Parse two numeric arguments: (x, y)
fn parse_two_numbers(args: &str) -> Result<(f64, f64), String> {
    let parts: Vec<&str> = args.split(',').map(|s| s.trim()).collect();
    if parts.len() != 2 {
        return Err(format!("Expected two numbers, got: {args}"));
    }
    let x = parts[0]
        .parse::<f64>()
        .map_err(|e| format!("Invalid x: {e}"))?;
    let y = parts[1]
        .parse::<f64>()
        .map_err(|e| format!("Invalid y: {e}"))?;
    Ok((x, y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_goto() {
        match parse_line("await page.goto('https://example.com');").unwrap() {
            Command::PageGoto { url } => assert_eq!(url, "https://example.com"),
            other => panic!("Expected PageGoto, got: {other:?}"),
        }
    }

    #[test]
    fn test_parse_viewport() {
        match parse_line("await page.setViewport({width: 1080, height: 1024});").unwrap() {
            Command::PageSetViewport { width, height } => {
                assert_eq!(width, 1080);
                assert_eq!(height, 1024);
            }
            other => panic!("Expected PageSetViewport, got: {other:?}"),
        }
    }

    #[test]
    fn test_parse_keyboard_press() {
        match parse_line("await page.keyboard.press('/');").unwrap() {
            Command::PageKeyboardPress { key } => assert_eq!(key, "/"),
            other => panic!("Expected PageKeyboardPress, got: {other:?}"),
        }
    }

    #[test]
    fn test_parse_locator_fill() {
        match parse_line("await page.locator('::-p-aria(Search)').fill('automate beyond recorder');").unwrap() {
            Command::PageLocatorFill { selector, text } => {
                assert_eq!(selector, "::-p-aria(Search)");
                assert_eq!(text, "automate beyond recorder");
            }
            other => panic!("Expected PageLocatorFill, got: {other:?}"),
        }
    }

    #[test]
    fn test_parse_locator_click() {
        match parse_line("await page.locator('.devsite-result-item-link').click();").unwrap() {
            Command::PageLocatorClick { selector } => {
                assert_eq!(selector, ".devsite-result-item-link");
            }
            other => panic!("Expected PageLocatorClick, got: {other:?}"),
        }
    }

    #[test]
    fn test_parse_assignment() {
        match parse_line("const page = await browser.newPage();").unwrap() {
            Command::BrowserNewPage { assign_to } => {
                assert_eq!(assign_to, Some("page".to_string()));
            }
            other => panic!("Expected BrowserNewPage, got: {other:?}"),
        }
    }

    #[test]
    fn test_parse_locator_wait_handle() {
        let line = "const textSelector = await page.locator('::-p-text(Customize and automate)').waitHandle();";
        match parse_line(line).unwrap() {
            Command::PageLocatorWaitHandle { selector, assign_to } => {
                assert_eq!(selector, "::-p-text(Customize and automate)");
                assert_eq!(assign_to, Some("textSelector".to_string()));
            }
            other => panic!("Expected PageLocatorWaitHandle, got: {other:?}"),
        }
    }

    #[test]
    fn test_parse_element_evaluate() {
        let line = "const fullTitle = await textSelector?.evaluate(el => el.textContent);";
        match parse_line(line).unwrap() {
            Command::ElementEvaluate { variable, js_fn, assign_to } => {
                assert_eq!(variable, "textSelector");
                assert_eq!(js_fn, "el => el.textContent");
                assert_eq!(assign_to, Some("fullTitle".to_string()));
            }
            other => panic!("Expected ElementEvaluate, got: {other:?}"),
        }
    }

    #[test]
    fn test_parse_console_log() {
        match parse_line(r#"console.log('The title of this blog post is "%s".', fullTitle);"#).unwrap() {
            Command::ConsoleLog { expression } => {
                assert!(expression.contains("fullTitle"));
            }
            other => panic!("Expected ConsoleLog, got: {other:?}"),
        }
    }

    #[test]
    fn test_parse_screenshot() {
        match parse_line("await page.screenshot({path: 'screenshot.png'});").unwrap() {
            Command::PageScreenshot { path } => {
                assert_eq!(path, Some("screenshot.png".to_string()));
            }
            other => panic!("Expected PageScreenshot, got: {other:?}"),
        }
    }

    #[test]
    fn test_parse_noop() {
        assert!(matches!(parse_line("").unwrap(), Command::Noop));
        assert!(matches!(parse_line("// comment").unwrap(), Command::Noop));
        assert!(matches!(parse_line("  # comment").unwrap(), Command::Noop));
    }

    #[test]
    fn test_parse_browser_close() {
        assert!(matches!(
            parse_line("await browser.close();").unwrap(),
            Command::BrowserClose
        ));
    }
}
