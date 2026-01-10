use reqwest::header::USER_AGENT;
use scraper::{Html, Selector};
use std::time::Duration;
use tokio::time::sleep;

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

async fn scrape_swappa(client: &reqwest::Client) -> Vec<Product> {
    let mut all_products = Vec::new();
    let base_url = "https://swappa.com";
    
    // First, fetch the main page to get all category links
    println!("  Fetching main page to discover categories...");
    let categories = if let Some(html) = fetch_html(client, base_url).await {
        let cats = extract_swappa_categories(&html, base_url);
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
            let products = scrape_swappa_products(&html, base_url);
            println!("    Found {} products", products.len());
            all_products.extend(products);
        }
        sleep(Duration::from_millis(1500)).await;
    }

    all_products
}

#[tokio::main]
async fn main() {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("Failed to create HTTP client");

    println!("🛒 Product Scraper - Newegg & Swappa\n");
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

    // Fetch detailed info for Swappa products
    let swappa_details = fetch_product_details(&client, &swappa_products, 5).await;
    
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
    println!("📊 SUMMARY");
    println!("{}", "=".repeat(60));
    println!("Newegg products found: {}", newegg_products.len());
    println!("Newegg detailed: {}", newegg_details.len());
    println!("Swappa products found: {}", swappa_products.len());
    println!("Swappa detailed: {}", swappa_details.len());
    println!("Total products: {}", newegg_products.len() + swappa_products.len());
    println!("Total detailed: {}", newegg_details.len() + swappa_details.len());
}


