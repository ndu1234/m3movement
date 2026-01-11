use reqwest::header::USER_AGENT;
use scraper::{Html, Selector};
use std::time::Duration;
use std::fs;
use tokio::time::sleep;
use thirtyfour::prelude::*;

#[derive(Debug, Clone)]
struct Product {
    name: String,
    price: String,
    url: String,
    source: String,
}

#[derive(Debug, Clone)]
struct ProductDetails {
    name: String,
    price: String,
    url: String,
    source: String,
    description: String,
    specs: Vec<String>,
    images: Vec<String>,
    condition: String,
    seller: String,
}

async fn fetch_html(client: &reqwest::Client, url: &str) -> Option<String> {
    let response = client
        .get(url)
        .header(USER_AGENT, "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .send()
        .await;

    match response {
        Ok(resp) => match resp.text().await {
            Ok(text) => Some(text),
            Err(e) => {
                eprintln!("Failed to read response from {}: {}", url, e);
                None
            }
        },
        Err(e) => {
            eprintln!("Failed to fetch {}: {}", url, e);
            None
        }
    }
}

fn scrape_newegg_products(html: &str, base_url: &str) -> Vec<Product> {
    let document = Html::parse_document(html);
    let mut products = Vec::new();

    // Newegg product items - try multiple selectors
    let item_selectors = [
        ".item-cell",           // Main product grid
        ".item-container",      // Alternative container
        ".item-info",           // Product info blocks
        "[class*='product']",   // Any product class
    ];

    for selector_str in &item_selectors {
        if let Ok(item_selector) = Selector::parse(selector_str) {
            for item in document.select(&item_selector) {
                let item_html = Html::parse_fragment(&item.html());
                
                // Try to get product name
                let name = get_text_from_selectors(&item_html, &[
                    ".item-title",
                    ".item-name", 
                    "a.item-title",
                    "[class*='title']",
                ]);

                // Try to get price
                let price = get_text_from_selectors(&item_html, &[
                    ".price-current",
                    ".price",
                    "[class*='price']",
                    "li.price-current",
                ]);

                // Try to get URL
                let url = get_href_from_selectors(&item_html, &[
                    "a.item-title",
                    "a[href*='/p/']",
                    "a",
                ]);

                if !name.is_empty() && name.len() > 5 {
                    let full_url = if url.starts_with("http") {
                        url
                    } else if url.starts_with("//") {
                        format!("https:{}", url)
                    } else if url.starts_with('/') {
                        format!("{}{}", base_url, url)
                    } else {
                        url
                    };

                    products.push(Product {
                        name: name.trim().to_string(),
                        price: if price.is_empty() { "Price not found".to_string() } else { price.trim().to_string() },
                        url: full_url,
                        source: "Newegg".to_string(),
                    });
                }
            }
        }

        if !products.is_empty() {
            break;
        }
    }

    // Deduplicate by name
    products.sort_by(|a, b| a.name.cmp(&b.name));
    products.dedup_by(|a, b| a.name == b.name);
    products
}

fn scrape_swappa_products(html: &str, base_url: &str) -> Vec<Product> {
    let document = Html::parse_document(html);
    let mut products = Vec::new();

    // Swappa listing items
    let item_selectors = [
        ".listing_row",
        ".listing-card",
        "[class*='listing']",
        ".product-card",
        ".item",
    ];

    for selector_str in &item_selectors {
        if let Ok(item_selector) = Selector::parse(selector_str) {
            for item in document.select(&item_selector) {
                let item_html = Html::parse_fragment(&item.html());
                
                // Get product name
                let name = get_text_from_selectors(&item_html, &[
                    ".listing_row_title",
                    ".listing-title",
                    ".title",
                    "h3",
                    "h4",
                    "[class*='title']",
                ]);

                // Get price
                let price = get_text_from_selectors(&item_html, &[
                    ".listing_row_price",
                    ".price",
                    "[class*='price']",
                ]);

                // Get URL - first check if the item itself is a link
                let mut url = if let Some(href) = item.value().attr("href") {
                    href.to_string()
                } else {
                    // Otherwise look for child links
                    get_href_from_selectors(&item_html, &[
                        "a[href*='/listing/']",
                        "a[href*='/buy/']",
                        "a",
                    ])
                };

                if !name.is_empty() && name.len() > 3 {
                    let full_url = if url.starts_with("http") {
                        url
                    } else if url.starts_with('/') {
                        format!("{}{}", base_url, url)
                    } else {
                        url
                    };

                    products.push(Product {
                        name: name.trim().to_string(),
                        price: if price.is_empty() { "Price not found".to_string() } else { price.trim().to_string() },
                        url: full_url,
                        source: "Swappa".to_string(),
                    });
                }
            }
        }

        if !products.is_empty() {
            break;
        }
    }

    products.sort_by(|a, b| a.name.cmp(&b.name));
    products.dedup_by(|a, b| a.name == b.name);
    products
}

fn get_text_from_selectors(html: &Html, selectors: &[&str]) -> String {
    for sel_str in selectors {
        if let Ok(selector) = Selector::parse(sel_str) {
            if let Some(element) = html.select(&selector).next() {
                let text: String = element.text().collect::<Vec<_>>().join(" ");
                let cleaned = text.split_whitespace().collect::<Vec<_>>().join(" ");
                if !cleaned.is_empty() {
                    return cleaned;
                }
            }
        }
    }
    String::new()
}

fn get_href_from_selectors(html: &Html, selectors: &[&str]) -> String {
    for sel_str in selectors {
        if let Ok(selector) = Selector::parse(sel_str) {
            if let Some(element) = html.select(&selector).next() {
                if let Some(href) = element.value().attr("href") {
                    return href.to_string();
                }
            }
        }
    }
    String::new()
}

// Parse detailed info from a Newegg product page
fn parse_newegg_product_page(html: &str, url: &str) -> ProductDetails {
    let document = Html::parse_document(html);
    
    // Get product name
    let name = get_text_from_selectors(&document, &[
        "h1.product-title",
        ".product-title",
        "h1[class*='title']",
        "h1",
    ]);
    
    // Get price
    let price = get_text_from_selectors(&document, &[
        ".price-current",
        ".product-price .price-current",
        "[class*='price'] strong",
        ".price",
    ]);
    
    // Get description
    let description = get_text_from_selectors(&document, &[
        ".product-bullets",
        ".product-description",
        "#product-details",
        "[class*='description']",
    ]);
    
    // Get specs
    let mut specs = Vec::new();
    let spec_selectors = [
        ".tab-pane table tr",
        ".product-specs tr",
        ".spec-table tr",
    ];
    for selector_str in &spec_selectors {
        if let Ok(selector) = Selector::parse(selector_str) {
            for row in document.select(&selector) {
                let text: String = row.text().collect::<Vec<_>>().join(" ");
                let cleaned = text.split_whitespace().collect::<Vec<_>>().join(" ");
                if !cleaned.is_empty() && cleaned.len() > 3 {
                    specs.push(cleaned);
                }
            }
        }
        if !specs.is_empty() {
            break;
        }
    }
    
    // Get images
    let mut images = Vec::new();
    let img_selectors = [
        ".product-view-gallery img",
        ".swiper-slide img",
        ".product-image img",
        "img[src*='productImage']",
    ];
    for selector_str in &img_selectors {
        if let Ok(selector) = Selector::parse(selector_str) {
            for img in document.select(&selector) {
                if let Some(src) = img.value().attr("src").or_else(|| img.value().attr("data-src")) {
                    let img_url = if src.starts_with("//") {
                        format!("https:{}", src)
                    } else {
                        src.to_string()
                    };
                    if !images.contains(&img_url) {
                        images.push(img_url);
                    }
                }
            }
        }
        if !images.is_empty() {
            break;
        }
    }
    
    // Get seller info
    let seller = get_text_from_selectors(&document, &[
        ".product-seller",
        ".seller-name",
        "[class*='seller']",
    ]);
    
    ProductDetails {
        name: if name.is_empty() { "Unknown".to_string() } else { name.trim().to_string() },
        price: if price.is_empty() { "Price not found".to_string() } else { price.trim().to_string() },
        url: url.to_string(),
        source: "Newegg".to_string(),
        description: description.trim().to_string(),
        specs: specs.into_iter().take(10).collect(), // Limit specs
        images: images.into_iter().take(5).collect(), // Limit images
        condition: "New".to_string(),
        seller: if seller.is_empty() { "Unknown".to_string() } else { seller.trim().to_string() },
    }
}

// Parse detailed info from a Swappa product page
fn parse_swappa_product_page(html: &str, url: &str) -> ProductDetails {
    let document = Html::parse_document(html);
    
    // Get product name
    let name = get_text_from_selectors(&document, &[
        "h1.listing-title",
        ".listing-title",
        "h1[class*='title']",
        "h1",
    ]);
    
    // Get price
    let price = get_text_from_selectors(&document, &[
        ".listing-price",
        ".price-tag",
        "[class*='price']",
    ]);
    
    // Get description
    let description = get_text_from_selectors(&document, &[
        ".listing-description",
        ".description-text",
        "[class*='description']",
    ]);
    
    // Get condition
    let condition = get_text_from_selectors(&document, &[
        ".listing-condition",
        ".condition-badge",
        "[class*='condition']",
    ]);
    
    // Get specs/details
    let mut specs = Vec::new();
    let spec_selectors = [
        ".listing-specs li",
        ".device-specs li",
        ".spec-list li",
        ".listing-details li",
    ];
    for selector_str in &spec_selectors {
        if let Ok(selector) = Selector::parse(selector_str) {
            for item in document.select(&selector) {
                let text: String = item.text().collect::<Vec<_>>().join(" ");
                let cleaned = text.split_whitespace().collect::<Vec<_>>().join(" ");
                if !cleaned.is_empty() && cleaned.len() > 2 {
                    specs.push(cleaned);
                }
            }
        }
        if !specs.is_empty() {
            break;
        }
    }
    
    // Get images
    let mut images = Vec::new();
    let img_selectors = [
        ".listing-gallery img",
        ".listing-images img",
        ".carousel img",
        "img[class*='listing']",
    ];
    for selector_str in &img_selectors {
        if let Ok(selector) = Selector::parse(selector_str) {
            for img in document.select(&selector) {
                if let Some(src) = img.value().attr("src").or_else(|| img.value().attr("data-src")) {
                    if !images.contains(&src.to_string()) {
                        images.push(src.to_string());
                    }
                }
            }
        }
        if !images.is_empty() {
            break;
        }
    }
    
    // Get seller
    let seller = get_text_from_selectors(&document, &[
        ".seller-name",
        ".listing-seller",
        "[class*='seller'] a",
    ]);
    
    ProductDetails {
        name: if name.is_empty() { "Unknown".to_string() } else { name.trim().to_string() },
        price: if price.is_empty() { "Price not found".to_string() } else { price.trim().to_string() },
        url: url.to_string(),
        source: "Swappa".to_string(),
        description: description.trim().to_string(),
        specs: specs.into_iter().take(10).collect(),
        images: images.into_iter().take(5).collect(),
        condition: if condition.is_empty() { "Unknown".to_string() } else { condition.trim().to_string() },
        seller: if seller.is_empty() { "Unknown".to_string() } else { seller.trim().to_string() },
    }
}

// Fetch detailed info for a list of products by visiting each product page
async fn fetch_product_details(client: &reqwest::Client, products: &[Product], max_items: usize) -> Vec<ProductDetails> {
    let mut details = Vec::new();
    
    let products_to_fetch: Vec<_> = products.iter()
        .filter(|p| !p.url.is_empty() && p.url.starts_with("http"))
        .take(max_items)
        .collect();
    
    println!("\n  📋 Fetching detailed info for {} products...\n", products_to_fetch.len());
    
    for (i, product) in products_to_fetch.iter().enumerate() {
        println!("    [{}/{}] Fetching details: {}", i + 1, products_to_fetch.len(), 
            if product.name.len() > 50 { &product.name[..50] } else { &product.name });
        
        if let Some(html) = fetch_html(client, &product.url).await {
            let detail = match product.source.as_str() {
                "Newegg" => parse_newegg_product_page(&html, &product.url),
                "Swappa" => parse_swappa_product_page(&html, &product.url),
                _ => continue,
            };
            details.push(detail);
        }
        
        // Rate limiting - be respectful to servers
        sleep(Duration::from_millis(2000)).await;
    }
    
    details
}

fn extract_newegg_categories(html: &str, base_url: &str) -> Vec<String> {
    let document = Html::parse_document(html);
    let mut categories = Vec::new();
    
    // Look for category links in Newegg's navigation
    let category_selectors = [
        "a[href*='/Category/']",
        "a[href*='/SubCategory/']",
        ".nav-category a",
        ".menu-list a",
        "[class*='category'] a",
    ];
    
    for selector_str in &category_selectors {
        if let Ok(selector) = Selector::parse(selector_str) {
            for element in document.select(&selector) {
                if let Some(href) = element.value().attr("href") {
                    let full_url = if href.starts_with("http") {
                        href.to_string()
                    } else if href.starts_with("//") {
                        format!("https:{}", href)
                    } else if href.starts_with('/') {
                        format!("{}{}", base_url, href)
                    } else {
                        continue;
                    };
                    
                    // Only add Newegg category URLs
                    if full_url.contains("newegg.com") && 
                       (full_url.contains("/Category/") || full_url.contains("/SubCategory/")) {
                        if !categories.contains(&full_url) {
                            categories.push(full_url);
                        }
                    }
                }
            }
        }
    }
    
    categories
}

async fn scrape_newegg(client: &reqwest::Client) -> Vec<Product> {
    let mut all_products = Vec::new();
    let base_url = "https://www.newegg.com";
    
    // First, fetch the main page to get all category links
    println!("  Fetching main page to discover categories...");
    let categories = if let Some(html) = fetch_html(client, base_url).await {
        let cats = extract_newegg_categories(&html, base_url);
        println!("  Found {} categories", cats.len());
        cats
    } else {
        Vec::new()
    };
    
    sleep(Duration::from_millis(1000)).await;
    
    // Limit to first 10 categories to avoid overwhelming the server
    let max_categories = 10;
    let categories_to_scrape: Vec<_> = categories.into_iter().take(max_categories).collect();
    
    for (i, url) in categories_to_scrape.iter().enumerate() {
        println!("  [{}/{}] Fetching: {}", i + 1, categories_to_scrape.len(), url);
        if let Some(html) = fetch_html(client, url).await {
            let products = scrape_newegg_products(&html, base_url);
            println!("    Found {} products", products.len());
            all_products.extend(products);
        }
        sleep(Duration::from_millis(1500)).await;
    }

    all_products
}

fn extract_swappa_categories(html: &str, base_url: &str) -> Vec<String> {
    let document = Html::parse_document(html);
    let mut categories = Vec::new();
    
    // Look for category links in Swappa's navigation
    let category_selectors = [
        "a[href*='/buy/']",
        "a[href*='/sell/']",
        ".nav a",
        ".menu a",
        "[class*='category'] a",
        "[class*='nav'] a",
    ];
    
    for selector_str in &category_selectors {
        if let Ok(selector) = Selector::parse(selector_str) {
            for element in document.select(&selector) {
                if let Some(href) = element.value().attr("href") {
                    let full_url = if href.starts_with("http") {
                        href.to_string()
                    } else if href.starts_with('/') {
                        format!("{}{}", base_url, href)
                    } else {
                        continue;
                    };
                    
                    // Only add Swappa buy category URLs
                    if full_url.contains("swappa.com") && full_url.contains("/buy/") {
                        // Skip listing pages, only get category pages
                        if !full_url.contains("/listing/") && !categories.contains(&full_url) {
                            categories.push(full_url);
                        }
                    }
                }
            }
        }
    }
    
    categories
}

async fn scrape_swappa(_client: &reqwest::Client) -> Vec<Product> {
    let mut all_products = Vec::new();
    
    println!("  Starting Selenium WebDriver for Swappa...");
    
    // Set up Chrome options - headless mode to run without visible browser
    let mut caps = DesiredCapabilities::chrome();
    caps.add_arg("--headless=new").ok();
    caps.add_arg("--disable-gpu").ok();
    caps.add_arg("--no-sandbox").ok();
    caps.add_arg("--disable-dev-shm-usage").ok();
    caps.add_arg("--window-size=1920,1200").ok();
    caps.add_arg("--disable-blink-features=AutomationControlled").ok();
    caps.add_arg("--user-agent=Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36").ok();
    
    // Connect to ChromeDriver
    let driver = match WebDriver::new("http://localhost:9515", caps).await {
        Ok(d) => d,
        Err(e) => {
            eprintln!("  ❌ Failed to connect to ChromeDriver: {}", e);
            eprintln!("  💡 Make sure ChromeDriver is running: chromedriver --port=9515");
            return all_products;
        }
    };
    
    println!("  ✓ Connected to ChromeDriver");
    
    // Create screenshots directory
    let screenshot_dir = "/tmp/swappa_screenshots";
    let _ = fs::create_dir_all(screenshot_dir);
    
    // URLs to scrape - these are specific device pages with listings
    let urls: Vec<(&str, &str)> = vec![
        ("iPhone 15", "https://swappa.com/buy/apple-iphone-15"),
        ("iPhone 14", "https://swappa.com/buy/apple-iphone-14"),
        ("iPhone 13", "https://swappa.com/buy/apple-iphone-13"),
        ("Galaxy S24", "https://swappa.com/buy/samsung-galaxy-s24"),
        ("Pixel 8", "https://swappa.com/buy/google-pixel-8"),
    ];
    
    for (category, url) in urls.iter() {
        println!("  📱 Scraping {}: {}", category, url);
        
        if let Err(e) = driver.goto(*url).await {
            eprintln!("    ❌ Failed to navigate to {}: {}", url, e);
            continue;
        }
        
        // Wait for page to fully load
        sleep(Duration::from_secs(4)).await;
        
        // Scroll to load all content
        for i in 0..5 {
            let scroll_pos = (i + 1) * 600;
            let _ = driver.execute(&format!("window.scrollTo(0, {})", scroll_pos), vec![]).await;
            sleep(Duration::from_millis(800)).await;
        }
        
        // Take and save screenshot
        let screenshot_path = format!("{}/{}.png", screenshot_dir, category.replace(" ", "_"));
        if let Ok(png_data) = driver.screenshot_as_png().await {
            if fs::write(&screenshot_path, &png_data).is_ok() {
                println!("    📸 Screenshot saved: {}", screenshot_path);
            }
        }
        
        // Extract all product info directly from the page using comprehensive JS
        let products_result = driver.execute(
            r#"
            var products = [];
            
            // Try to find all href links first
            var allLinks = document.querySelectorAll('a[href]');
            var listingLinks = [];
            allLinks.forEach(function(a) {
                var href = a.href || '';
                if (href.includes('/listing/') || href.includes('/buy/') && href.includes('-')) {
                    listingLinks.push(href);
                }
            });
            
            // Get the page text and parse it
            var text = document.body.innerText;
            var lines = text.split('\n');
            
            for (var i = 0; i < lines.length; i++) {
                var line = lines[i].trim();
                
                // Look for price patterns
                if (line.match(/^\$\d+/) && line.length < 15) {
                    var price = line.split(' ')[0]; // Get just the price
                    
                    // Look backwards for product name
                    for (var j = 1; j <= 5 && i >= j; j++) {
                        var name = lines[i - j].trim();
                        if (name.length > 5 && !name.startsWith('$') && 
                            (name.includes('iPhone') || name.includes('Galaxy') || 
                             name.includes('Pixel') || name.includes('GB') ||
                             name.includes('Pro') || name.includes('Max') ||
                             name.includes('Plus') || name.includes('Ultra'))) {
                            
                            // Check if we already have this product
                            var exists = products.some(function(p) { return p.name === name; });
                            if (!exists) {
                                products.push({
                                    name: name,
                                    price: price,
                                    url: listingLinks.length > products.length ? listingLinks[products.length] : ''
                                });
                            }
                            break;
                        }
                    }
                }
            }
            
            return { products: products.slice(0, 20), links: listingLinks.slice(0, 10) };
            "#,
            vec![]
        ).await;
        
        if let Ok(result_value) = products_result {
            let json = result_value.json();
            
            // Get listing links for detailed fetching
            if let Some(links_arr) = json.get("links").and_then(|v| v.as_array()) {
                let link_count = links_arr.len();
                if link_count > 0 {
                    println!("    🔗 Found {} listing links", link_count);
                }
            }
            
            // Get products
            if let Some(products_arr) = json.get("products").and_then(|v| v.as_array()) {
                for product in products_arr {
                    let name = product.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let price = product.get("price").and_then(|v| v.as_str()).unwrap_or("");
                    let prod_url = product.get("url").and_then(|v| v.as_str()).unwrap_or("");
                    
                    if !name.is_empty() && !all_products.iter().any(|p: &Product| p.name == name) {
                        let final_url = if !prod_url.is_empty() && prod_url.contains("/listing/") {
                            prod_url.to_string()
                        } else {
                            url.to_string()
                        };
                        
                        all_products.push(Product {
                            name: name.to_string(),
                            price: price.to_string(),
                            url: final_url,
                            source: "Swappa".to_string(),
                        });
                        println!("      Found: {} - {}", name, price);
                    }
                }
            }
        }
        
        // Now click on individual listings to get their URLs
        let click_result = driver.execute(
            r#"
            var listingCards = document.querySelectorAll('[class*="listing"], [class*="item"], [class*="card"]');
            var urls = [];
            
            // Also try finding clickable elements with prices
            var allElements = document.querySelectorAll('a');
            for (var i = 0; i < Math.min(allElements.length, 100); i++) {
                var el = allElements[i];
                var href = el.href || '';
                var text = el.innerText || '';
                
                if (href.includes('swappa.com') && text.includes('$') && 
                    (text.includes('iPhone') || text.includes('Galaxy') || text.includes('Pixel') ||
                     text.includes('Good') || text.includes('Fair') || text.includes('Mint'))) {
                    urls.push({ url: href, text: text.substring(0, 100) });
                }
            }
            
            return urls.slice(0, 10);
            "#,
            vec![]
        ).await;
        
        if let Ok(urls_value) = click_result {
            if let Some(arr) = urls_value.json().as_array() {
                for item in arr.iter().take(3) {
                    let listing_url = item.get("url").and_then(|v| v.as_str()).unwrap_or("");
                    
                    if !listing_url.is_empty() && listing_url.contains("/listing/") {
                        // Visit this listing to get detailed info
                        if let Ok(_) = driver.goto(listing_url).await {
                            sleep(Duration::from_secs(3)).await;
                            
                            let detail_result = driver.execute(
                                r#"
                                var info = {};
                                info.name = (document.querySelector('h1') || {}).innerText || '';
                                info.price = (document.querySelector('[class*="price"]') || {}).innerText || '';
                                info.condition = (document.querySelector('[class*="condition"]') || {}).innerText || '';
                                info.seller = (document.querySelector('[class*="seller"], a[href*="/user/"]') || {}).innerText || '';
                                info.url = window.location.href;
                                return info;
                                "#,
                                vec![]
                            ).await;
                            
                            if let Ok(info_value) = detail_result {
                                let info = info_value.json();
                                let name = info.get("name").and_then(|v| v.as_str()).unwrap_or("");
                                let price = info.get("price").and_then(|v| v.as_str()).unwrap_or("");
                                let url = info.get("url").and_then(|v| v.as_str()).unwrap_or("");
                                
                                if !name.is_empty() && name.len() > 3 && !all_products.iter().any(|p| p.name == name) {
                                    all_products.push(Product {
                                        name: name.to_string(),
                                        price: price.to_string(),
                                        url: url.to_string(),
                                        source: "Swappa".to_string(),
                                    });
                                    println!("      📦 Listing detail: {} - {}", name, price);
                                }
                            }
                        }
                        
                        // Go back to the category page
                        let _ = driver.goto(*url).await;
                        sleep(Duration::from_secs(2)).await;
                    }
                }
            }
        }
        
        sleep(Duration::from_secs(1)).await;
    }
    
    // Close the browser
    if let Err(e) = driver.quit().await {
        eprintln!("  Warning: Failed to close browser: {}", e);
    }
    
    println!("  ✓ Swappa scraping complete. Found {} products", all_products.len());
    println!("  📁 Screenshots saved to: {}", screenshot_dir);
    
    all_products
}

#[tokio::main]
async fn main() {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("Failed to create HTTP client");

    println!("🛒 Product Scraper - Newegg & Swappa");
    println!("⏰ Running every 1 minute. Press Ctrl+C to stop.\n");
    
    let mut run_count = 0;
    
    loop {
        run_count += 1;
        let now = chrono::Local::now();
        
        println!("\n{}", "=".repeat(60));
        println!("🔄 SCRAPE RUN #{} - {}", run_count, now.format("%Y-%m-%d %H:%M:%S"));
        println!("{}", "=".repeat(60));

        // Scrape Newegg
        println!("\n📦 Scraping Newegg...\n");
        let newegg_products = scrape_newegg(&client).await;
        
        println!("\n{}", "-".repeat(60));
        println!("NEWEGG PRODUCTS ({})", newegg_products.len());
        println!("{}", "-".repeat(60));
        
        for (i, product) in newegg_products.iter().take(15).enumerate() {
            println!("\n{}. {}", i + 1, product.name);
            println!("   💰 Price: {}", product.price);
            println!("   🔗 {}", product.url);
        }

        // Fetch detailed info for Newegg products
        let newegg_details = fetch_product_details(&client, &newegg_products, 5).await;
        
        println!("\n{}", "=".repeat(60));
        println!("📦 NEWEGG DETAILED PRODUCTS ({})", newegg_details.len());
        println!("{}", "=".repeat(60));
        
        for (i, detail) in newegg_details.iter().enumerate() {
            println!("\n{}. {}", i + 1, detail.name);
            println!("   💰 Price: {}", detail.price);
            println!("   📝 Description: {}", if detail.description.len() > 100 { 
                format!("{}...", &detail.description[..100]) 
            } else { 
                detail.description.clone() 
            });
            println!("   🏷️  Condition: {}", detail.condition);
            println!("   👤 Seller: {}", detail.seller);
            if !detail.specs.is_empty() {
                println!("   📋 Specs ({}):", detail.specs.len());
                for spec in detail.specs.iter().take(3) {
                    println!("      - {}", if spec.len() > 60 { format!("{}...", &spec[..60]) } else { spec.clone() });
                }
            }
            if !detail.images.is_empty() {
                println!("   🖼️  Images: {}", detail.images.len());
            }
            println!("   🔗 {}", detail.url);
        }

        sleep(Duration::from_millis(2000)).await;

        // Scrape Swappa
        println!("\n\n📱 Scraping Swappa...\n");
        let swappa_products = scrape_swappa(&client).await;
        
        println!("\n{}", "-".repeat(60));
        println!("SWAPPA PRODUCTS ({})", swappa_products.len());
        println!("{}", "-".repeat(60));
        
        for (i, product) in swappa_products.iter().take(15).enumerate() {
            println!("\n{}. {}", i + 1, product.name);
            println!("   💰 Price: {}", product.price);
            println!("   🔗 {}", product.url);
        }

        // Fetch detailed info for Swappa products using Selenium
        let swappa_details = fetch_swappa_details_selenium(&swappa_products, 5).await;
        
        println!("\n{}", "=".repeat(60));
        println!("📱 SWAPPA DETAILED PRODUCTS ({})", swappa_details.len());
        println!("{}", "=".repeat(60));
        
        for (i, detail) in swappa_details.iter().enumerate() {
            println!("\n{}. {}", i + 1, detail.name);
            println!("   💰 Price: {}", detail.price);
            println!("   📝 Description: {}", if detail.description.len() > 100 { 
                format!("{}...", &detail.description[..100]) 
            } else { 
                detail.description.clone() 
            });
            println!("   🏷️  Condition: {}", detail.condition);
            println!("   👤 Seller: {}", detail.seller);
            if !detail.specs.is_empty() {
                println!("   📋 Specs ({}):", detail.specs.len());
                for spec in detail.specs.iter().take(3) {
                    println!("      - {}", if spec.len() > 60 { format!("{}...", &spec[..60]) } else { spec.clone() });
                }
            }
            if !detail.images.is_empty() {
                println!("   🖼️  Images: {}", detail.images.len());
            }
            println!("   🔗 {}", detail.url);
        }

        // Summary
        println!("\n\n{}", "=".repeat(60));
        println!("📊 SUMMARY - Run #{}", run_count);
        println!("{}", "=".repeat(60));
        println!("Newegg products found: {}", newegg_products.len());
        println!("Newegg detailed: {}", newegg_details.len());
        println!("Swappa products found: {}", swappa_products.len());
        println!("Swappa detailed: {}", swappa_details.len());
        println!("Total products: {}", newegg_products.len() + swappa_products.len());
        println!("Total detailed: {}", newegg_details.len() + swappa_details.len());
        
        // Wait 1 minute before next scrape
        println!("\n⏳ Next scrape in 60 seconds...");
        println!("   Press Ctrl+C to stop.");
        sleep(Duration::from_secs(60)).await;
    }
}

// Fetch Swappa product details using Selenium (since regular HTTP doesn't work)
async fn fetch_swappa_details_selenium(products: &[Product], max_items: usize) -> Vec<ProductDetails> {
    let mut details = Vec::new();
    
    // Only process products with actual listing URLs
    let products_to_fetch: Vec<_> = products.iter()
        .filter(|p| p.url.contains("/listing/"))
        .take(max_items)
        .collect();
    
    if products_to_fetch.is_empty() {
        println!("\n  📋 No individual Swappa listing URLs to fetch details from");
        return details;
    }
    
    println!("\n  📋 Fetching detailed info for {} Swappa products...\n", products_to_fetch.len());
    
    // Set up Chrome - headless mode
    let mut caps = DesiredCapabilities::chrome();
    caps.add_arg("--headless=new").ok();
    caps.add_arg("--disable-gpu").ok();
    caps.add_arg("--no-sandbox").ok();
    caps.add_arg("--disable-dev-shm-usage").ok();
    caps.add_arg("--window-size=1920,1200").ok();
    caps.add_arg("--disable-blink-features=AutomationControlled").ok();
    caps.add_arg("--user-agent=Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36").ok();
    
    let driver = match WebDriver::new("http://localhost:9515", caps).await {
        Ok(d) => d,
        Err(e) => {
            eprintln!("  ❌ Failed to connect to ChromeDriver: {}", e);
            return details;
        }
    };
    
    for (i, product) in products_to_fetch.iter().enumerate() {
        println!("    [{}/{}] Fetching: {}", i + 1, products_to_fetch.len(), 
            if product.name.len() > 50 { &product.name[..50] } else { &product.name });
        
        if let Err(e) = driver.goto(&product.url).await {
            eprintln!("      ❌ Failed to navigate: {}", e);
            continue;
        }
        
        sleep(Duration::from_secs(3)).await;
        
        // Extract detailed info using JavaScript
        let detail_result = driver.execute(
            r#"
            var info = {};
            
            // Get title
            var title = document.querySelector('h1, .listing-title, [class*="title"]');
            info.name = title ? title.innerText.trim() : '';
            
            // Get price
            var priceEl = document.querySelector('[class*="price"], .price, .listing-price');
            info.price = priceEl ? priceEl.innerText.trim() : '';
            
            // Get description
            var descEl = document.querySelector('[class*="description"], .listing-description, .description');
            info.description = descEl ? descEl.innerText.trim().substring(0, 500) : '';
            
            // Get condition
            var condEl = document.querySelector('[class*="condition"], .condition-badge, .listing-condition');
            info.condition = condEl ? condEl.innerText.trim() : '';
            
            // Get seller
            var sellerEl = document.querySelector('[class*="seller"], .seller-name, a[href*="/user/"]');
            info.seller = sellerEl ? sellerEl.innerText.trim() : '';
            
            // Get specs from page
            var specs = [];
            var specItems = document.querySelectorAll('[class*="spec"] li, .device-info li, .listing-details li');
            specItems.forEach(function(item) {
                var text = item.innerText.trim();
                if (text && text.length > 2) specs.push(text);
            });
            info.specs = specs.slice(0, 10);
            
            // Get images
            var images = [];
            var imgs = document.querySelectorAll('img[src*="swappa"], .listing-images img, .gallery img');
            imgs.forEach(function(img) {
                if (img.src && !images.includes(img.src)) images.push(img.src);
            });
            info.images = images.slice(0, 5);
            
            return info;
            "#,
            vec![]
        ).await;
        
        if let Ok(info_value) = detail_result {
            let json = info_value.json();
            
            let name = json.get("name").and_then(|v| v.as_str()).unwrap_or(&product.name).to_string();
            let price = json.get("price").and_then(|v| v.as_str()).unwrap_or(&product.price).to_string();
            let description = json.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let condition = json.get("condition").and_then(|v| v.as_str()).unwrap_or("Unknown").to_string();
            let seller = json.get("seller").and_then(|v| v.as_str()).unwrap_or("Unknown").to_string();
            
            let specs: Vec<String> = json.get("specs")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            
            let images: Vec<String> = json.get("images")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            
            details.push(ProductDetails {
                name,
                price,
                url: product.url.clone(),
                source: "Swappa".to_string(),
                description,
                specs,
                images,
                condition: if condition.is_empty() { "Unknown".to_string() } else { condition },
                seller: if seller.is_empty() { "Unknown".to_string() } else { seller },
            });
        }
        
        sleep(Duration::from_secs(2)).await;
    }
    
    let _ = driver.quit().await;
    
    details
}