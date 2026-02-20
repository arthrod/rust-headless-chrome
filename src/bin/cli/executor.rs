use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Result, anyhow, bail};

use headless_chrome::browser::tab::point::Point;
use headless_chrome::protocol::cdp::{Emulation, Page};
use headless_chrome::types::PrintToPdfOptions;
use headless_chrome::{Browser, Tab};

use super::parser::Command;

/// Holds the runtime state for command execution.
pub struct ExecutionContext {
    pub browser: Browser,
    /// Current active tab (the "page")
    pub page: Option<Arc<Tab>>,
    /// Named variables for element handles and values
    pub variables: HashMap<String, Variable>,
    /// Whether to print verbose output
    pub verbose: bool,
}

#[derive(Debug, Clone)]
pub enum Variable {
    /// A CSS selector used to re-locate an element
    ElementSelector(String),
    /// A string value (result of evaluate, getInnerText, etc.)
    StringValue(String),
    /// A JSON value
    JsonValue(serde_json::Value),
}

impl ExecutionContext {
    pub fn new(browser: Browser, verbose: bool) -> Self {
        Self {
            browser,
            page: None,
            variables: HashMap::new(),
            verbose,
        }
    }

    fn tab(&self) -> Result<&Arc<Tab>> {
        self.page
            .as_ref()
            .ok_or_else(|| anyhow!("No page open. Use `browser.newPage()` first."))
    }

    pub fn execute(&mut self, cmd: &Command) -> Result<Option<String>> {
        match cmd {
            Command::BrowserNewPage { assign_to } => {
                let tab = self.browser.new_tab()?;
                self.page = Some(tab);
                if let Some(name) = assign_to {
                    if self.verbose {
                        return Ok(Some(format!("Page created → {name}")));
                    }
                }
                Ok(Some("New page created.".to_string()))
            }

            Command::BrowserClose => {
                // Drop the page first, then drop browser will close the process
                self.page = None;
                // We can't really "close" the browser in a clean way through the API,
                // but dropping it will kill the process.
                // For the CLI, we signal that we should exit.
                Ok(Some("Browser closed.".to_string()))
            }

            Command::PageGoto { url } => {
                let tab = self.tab()?;
                tab.navigate_to(url)?;
                tab.wait_until_navigated()?;
                if self.verbose {
                    Ok(Some(format!("Navigated to {url}")))
                } else {
                    Ok(None)
                }
            }

            Command::PageSetViewport { width, height } => {
                let tab = self.tab()?;
                // Use Emulation.setDeviceMetricsOverride to set viewport size
                // This is what Puppeteer does under the hood
                tab.call_method(Emulation::SetDeviceMetricsOverride {
                    width: *width,
                    height: *height,
                    device_scale_factor: 1.0,
                    mobile: false,
                    scale: None,
                    screen_width: None,
                    screen_height: None,
                    position_x: None,
                    position_y: None,
                    dont_set_visible_size: None,
                    screen_orientation: None,
                    viewport: None,
                    display_feature: None,
                    device_posture: None,
                })?;
                if self.verbose {
                    Ok(Some(format!("Viewport set to {width}x{height}")))
                } else {
                    Ok(None)
                }
            }

            Command::PageKeyboardPress { key } => {
                let tab = self.tab()?;
                tab.press_key(key)?;
                if self.verbose {
                    Ok(Some(format!("Pressed key: {key}")))
                } else {
                    Ok(None)
                }
            }

            Command::PageKeyboardType { text } => {
                let tab = self.tab()?;
                tab.type_str(text)?;
                if self.verbose {
                    Ok(Some(format!("Typed: {text}")))
                } else {
                    Ok(None)
                }
            }

            Command::PageLocatorClick { selector } => {
                let tab = self.tab()?;
                let selector = resolve_selector(selector);
                tab.wait_for_element(&selector)?.click()?;
                if self.verbose {
                    Ok(Some(format!("Clicked: {selector}")))
                } else {
                    Ok(None)
                }
            }

            Command::PageLocatorFill { selector, text } => {
                let tab = self.tab()?;
                let selector = resolve_selector(selector);
                let el = tab.wait_for_element(&selector)?;
                el.click()?;
                tab.type_str(text)?;
                if self.verbose {
                    Ok(Some(format!("Filled '{selector}' with: {text}")))
                } else {
                    Ok(None)
                }
            }

            Command::PageLocatorWaitHandle { selector, assign_to } => {
                let tab = self.tab()?;
                let selector = resolve_selector(selector);
                let _el = tab.wait_for_element(&selector)?;
                if let Some(name) = assign_to {
                    self.variables
                        .insert(name.clone(), Variable::ElementSelector(selector.clone()));
                    if self.verbose {
                        return Ok(Some(format!("Element '{selector}' found → {name}")));
                    }
                }
                Ok(Some(format!("Element found: {selector}")))
            }

            Command::PageWaitForSelector { selector, assign_to } => {
                let tab = self.tab()?;
                let _el = tab.wait_for_element(selector)?;
                if let Some(name) = assign_to {
                    self.variables
                        .insert(name.clone(), Variable::ElementSelector(selector.clone()));
                    if self.verbose {
                        return Ok(Some(format!("Element '{selector}' found → {name}")));
                    }
                }
                Ok(Some(format!("Element found: {selector}")))
            }

            Command::PageType { selector, text } => {
                let tab = self.tab()?;
                let el = tab.wait_for_element(selector)?;
                el.click()?;
                tab.type_str(text)?;
                if self.verbose {
                    Ok(Some(format!("Typed into '{selector}': {text}")))
                } else {
                    Ok(None)
                }
            }

            Command::PageClick { selector } => {
                let tab = self.tab()?;
                tab.wait_for_element(selector)?.click()?;
                if self.verbose {
                    Ok(Some(format!("Clicked: {selector}")))
                } else {
                    Ok(None)
                }
            }

            Command::PageEvaluate { expression, assign_to } => {
                let tab = self.tab()?;
                let js = normalize_evaluate_expression(expression);
                let result = tab.evaluate(&js, true)?;
                let value = format_remote_object_value(&result.value);
                if let Some(name) = assign_to {
                    if let Some(ref val) = result.value {
                        self.variables
                            .insert(name.clone(), Variable::JsonValue(val.clone()));
                    } else {
                        self.variables
                            .insert(name.clone(), Variable::StringValue(value.clone()));
                    }
                }
                Ok(Some(value))
            }

            Command::ElementEvaluate { variable, js_fn, assign_to } => {
                let tab = self.tab()?;
                // Resolve the element from the stored selector
                let selector = match self.variables.get(variable) {
                    Some(Variable::ElementSelector(sel)) => sel.clone(),
                    Some(_) => bail!("Variable '{variable}' is not an element handle"),
                    None => bail!("Unknown variable: {variable}"),
                };
                let el = tab.wait_for_element(&selector)?;

                // Convert arrow function to function expression
                let js_code = arrow_fn_to_function(js_fn);
                let result = el.call_js_fn(&js_code, vec![], true)?;
                let value = format_remote_object_value(&result.value);

                if let Some(name) = assign_to {
                    if let Some(ref val) = result.value {
                        self.variables
                            .insert(name.clone(), Variable::JsonValue(val.clone()));
                    } else {
                        self.variables
                            .insert(name.clone(), Variable::StringValue(value.clone()));
                    }
                }
                Ok(Some(value))
            }

            Command::PageScreenshot { path } => {
                let tab = self.tab()?;
                let data = tab.capture_screenshot(
                    Page::CaptureScreenshotFormatOption::Png,
                    None,
                    None,
                    true,
                )?;
                let path = path.as_deref().unwrap_or("screenshot.png");
                std::fs::write(path, &data)?;
                Ok(Some(format!("Screenshot saved to {path} ({} bytes)", data.len())))
            }

            Command::PageContent { assign_to } => {
                let tab = self.tab()?;
                let content = tab.get_content()?;
                if let Some(name) = assign_to {
                    self.variables
                        .insert(name.clone(), Variable::StringValue(content.clone()));
                    if self.verbose {
                        return Ok(Some(format!("Content stored in {name} ({} chars)", content.len())));
                    }
                }
                // For content, just show length to avoid flooding the terminal
                Ok(Some(format!("Page content: {} chars", content.len())))
            }

            Command::PageTitle { assign_to } => {
                let tab = self.tab()?;
                let title = tab.get_title()?;
                if let Some(name) = assign_to {
                    self.variables
                        .insert(name.clone(), Variable::StringValue(title.clone()));
                }
                Ok(Some(title))
            }

            Command::PageUrl { assign_to } => {
                let tab = self.tab()?;
                let url = tab.get_url();
                if let Some(name) = assign_to {
                    self.variables
                        .insert(name.clone(), Variable::StringValue(url.clone()));
                }
                Ok(Some(url))
            }

            Command::PagePdf { path } => {
                let tab = self.tab()?;
                let data = tab.print_to_pdf(Some(PrintToPdfOptions::default()))?;
                let path = path.as_deref().unwrap_or("page.pdf");
                std::fs::write(path, &data)?;
                Ok(Some(format!("PDF saved to {path} ({} bytes)", data.len())))
            }

            Command::ConsoleLog { expression } => {
                let output = self.resolve_console_log(expression);
                Ok(Some(output))
            }

            Command::PageMouseClick { x, y } => {
                let tab = self.tab()?;
                tab.click_point(Point { x: *x, y: *y })?;
                if self.verbose {
                    Ok(Some(format!("Mouse clicked at ({x}, {y})")))
                } else {
                    Ok(None)
                }
            }

            Command::PageMouseMove { x, y } => {
                let tab = self.tab()?;
                tab.move_mouse_to_point(Point { x: *x, y: *y })?;
                if self.verbose {
                    Ok(Some(format!("Mouse moved to ({x}, {y})")))
                } else {
                    Ok(None)
                }
            }

            Command::PageSetUserAgent { user_agent } => {
                let tab = self.tab()?;
                tab.set_user_agent(user_agent, None, None)?;
                if self.verbose {
                    Ok(Some(format!("User agent set to: {user_agent}")))
                } else {
                    Ok(None)
                }
            }

            Command::PageGoBack => {
                let tab = self.tab()?;
                let js = "window.history.back()";
                tab.evaluate(js, false)?;
                std::thread::sleep(std::time::Duration::from_millis(500));
                if self.verbose {
                    Ok(Some("Navigated back.".to_string()))
                } else {
                    Ok(None)
                }
            }

            Command::PageGoForward => {
                let tab = self.tab()?;
                let js = "window.history.forward()";
                tab.evaluate(js, false)?;
                std::thread::sleep(std::time::Duration::from_millis(500));
                if self.verbose {
                    Ok(Some("Navigated forward.".to_string()))
                } else {
                    Ok(None)
                }
            }

            Command::PageReload => {
                let tab = self.tab()?;
                tab.reload(true, None)?;
                if self.verbose {
                    Ok(Some("Page reloaded.".to_string()))
                } else {
                    Ok(None)
                }
            }

            Command::PageSetCookie { cookies_json } => {
                let tab = self.tab()?;
                // Parse the cookies JSON
                let cookies: Vec<headless_chrome::protocol::cdp::Network::CookieParam> =
                    serde_json::from_str(&format!("[{cookies_json}]"))
                        .map_err(|e| anyhow!("Invalid cookie JSON: {e}"))?;
                tab.set_cookies(cookies)?;
                Ok(Some("Cookies set.".to_string()))
            }

            Command::PageCookies { assign_to } => {
                let tab = self.tab()?;
                let cookies = tab.get_cookies()?;
                let json = serde_json::to_string_pretty(&cookies)?;
                if let Some(name) = assign_to {
                    self.variables
                        .insert(name.clone(), Variable::StringValue(json.clone()));
                }
                Ok(Some(json))
            }

            Command::Noop => Ok(None),
        }
    }

    /// Resolve a console.log expression, substituting variables
    fn resolve_console_log(&self, expression: &str) -> String {
        let expression = expression.trim();

        // Parse format string and arguments: 'format %s', var1, var2
        let parts = split_console_log_args(expression);

        if parts.is_empty() {
            return String::new();
        }

        // First part is the format string
        let format_str = unquote_string(&parts[0]);
        let args: Vec<String> = parts[1..]
            .iter()
            .map(|arg| {
                let arg = arg.trim();
                // Try to resolve as variable
                match self.variables.get(arg) {
                    Some(Variable::StringValue(s)) => s.clone(),
                    Some(Variable::JsonValue(v)) => match v {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    },
                    Some(Variable::ElementSelector(s)) => format!("[Element: {s}]"),
                    None => arg.to_string(),
                }
            })
            .collect();

        // Replace %s placeholders with arguments
        let mut result = format_str;
        for arg in &args {
            if let Some(pos) = result.find("%s") {
                result = format!("{}{}{}", &result[..pos], arg, &result[pos + 2..]);
            }
        }

        result
    }
}

/// Resolve Puppeteer pseudo-selectors to standard CSS.
/// Puppeteer uses `::-p-text(...)`, `::-p-aria(...)` etc. which are not standard CSS.
/// We convert these to XPath or JS-based lookups where possible, but for now
/// we convert `::-p-text(X)` to an XPath-like text search.
fn resolve_selector(selector: &str) -> String {
    // For standard CSS selectors, pass through
    if !selector.starts_with("::-p-") {
        return selector.to_string();
    }

    // ::-p-text(text) → we'll use a CSS fallback - look for elements containing text
    // This can't be done purely in CSS, so we'll keep it as-is and handle in the
    // element finding logic. For now, strip it and use body as fallback.
    // In practice, the Tab API's wait_for_element doesn't support these pseudo-selectors.

    // Best effort: just return the selector and let it fail gracefully
    // The executor should handle this by falling back to evaluate()
    selector.to_string()
}

/// Convert arrow function to regular function for call_js_fn
/// e.g., "el => el.textContent" → "function(el) { return el.textContent; }"
fn arrow_fn_to_function(arrow: &str) -> String {
    let arrow = arrow.trim();

    // Already a function expression?
    if arrow.starts_with("function") {
        return arrow.to_string();
    }

    // Parse arrow function: (args) => body  or  arg => body
    if let Some(arrow_pos) = arrow.find("=>") {
        let params = arrow[..arrow_pos].trim();
        let body = arrow[arrow_pos + 2..].trim();

        // Strip parens from params if present
        let params = params
            .strip_prefix('(')
            .and_then(|s| s.strip_suffix(')'))
            .unwrap_or(params);

        // If body is wrapped in braces, use as-is; otherwise wrap in return
        if body.starts_with('{') {
            format!("function({params}) {body}")
        } else {
            format!("function({params}) {{ return {body}; }}")
        }
    } else {
        // Fallback: wrap the whole thing
        format!("function() {{ return {arrow}; }}")
    }
}

/// Normalize an evaluate expression.
/// Handles: `() => expr`, `function() { ... }`, or plain JS expression.
fn normalize_evaluate_expression(expr: &str) -> String {
    let expr = expr.trim();

    // Arrow function: () => expr
    if let Some(arrow_pos) = expr.find("=>") {
        let body = expr[arrow_pos + 2..].trim();
        // If the body has braces, extract the content
        if body.starts_with('{') && body.ends_with('}') {
            // It's a block body, extract and wrap in IIFE
            return format!("(function() {body})()");
        }
        // Expression body
        return body.to_string();
    }

    // Already a function expression? Wrap in IIFE
    if expr.starts_with("function") {
        return format!("({expr})()");
    }

    // Plain expression
    expr.to_string()
}

fn format_remote_object_value(value: &Option<serde_json::Value>) -> String {
    match value {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Null) => "null".to_string(),
        Some(v) => v.to_string(),
        None => "undefined".to_string(),
    }
}

/// Split console.log arguments respecting string quotes
fn split_console_log_args(expr: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut quote_char = ' ';
    let mut chars = expr.chars().peekable();

    while let Some(c) = chars.next() {
        if in_string {
            current.push(c);
            if c == '\\' {
                if let Some(&next) = chars.peek() {
                    current.push(next);
                    chars.next();
                }
            } else if c == quote_char {
                in_string = false;
            }
        } else if c == '\'' || c == '"' || c == '`' {
            in_string = true;
            quote_char = c;
            current.push(c);
        } else if c == ',' {
            parts.push(current.trim().to_string());
            current = String::new();
        } else {
            current.push(c);
        }
    }

    if !current.trim().is_empty() {
        parts.push(current.trim().to_string());
    }

    parts
}

/// Remove surrounding quotes from a string
fn unquote_string(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 {
        let first = s.chars().next().unwrap();
        let last = s.chars().next_back().unwrap();
        if (first == '\'' || first == '"' || first == '`') && first == last {
            return s[1..s.len() - 1].to_string();
        }
    }
    s.to_string()
}
