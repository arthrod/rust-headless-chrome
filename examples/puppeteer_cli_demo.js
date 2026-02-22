// Puppeteer-compatible script for rust-headless-chrome CLI
// This mirrors the Puppeteer example from the docs.
//
// Run with: cargo run --bin puppeteer-script -- examples/puppeteer_cli_demo.js -v

const page = await browser.newPage();

// Navigate to the Chrome developer site.
await page.goto('https://developer.chrome.com/');

// Set screen size.
await page.setViewport({width: 1080, height: 1024});

// Take a screenshot
await page.screenshot({path: 'chrome_dev.png'});

// Print the page title
const title = await page.title();
console.log('Page title: %s', title);

// Close the browser
await browser.close();
