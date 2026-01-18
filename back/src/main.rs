use reqwest::header::USER_AGENT;
use scraper::{Html, Selector};
use std::time::Duration;
use std::fs;
use std::collections::HashSet;
use tokio::time::sleep;
use thirtyfour::prelude::*;
use serde::{Serialize, Deserialize};
use chrono::{Local, DateTime};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Product {
    name: String,
    price: String,
    url: String,
    source: String,
}

// Structure for arbitrage data export
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ArbitrageOpportunity {
    buy_product_name: String,
    buy_source: String,
    buy_price: f64,
    buy_url: String,
    ebay_avg_sold_price: f64,
    ebay_sold_count: usize,
    ebay_price_range: String,
    potential_profit: f64,
    margin_percent: f64,
    sample_ebay_urls: Vec<String>,
}

// Structure for individual product with eBay comparison
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProductWithComparison {
    name: String,
    price: String,
    price_numeric: f64,
    url: String,
    source: String,
    ebay_avg_sold: Option<f64>,
    ebay_sold_count: Option<usize>,
    ebay_price_range: Option<String>,
    potential_profit: Option<f64>,
    margin_percent: Option<f64>,
}

// Structure for a single run snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RunSnapshot {
    run_id: u32,
    timestamp: String,
    swappa_products: Vec<ProductWithComparison>,
    newegg_products: Vec<ProductWithComparison>,
    ebay_sold_products: Vec<Product>,
    arbitrage_opportunities: Vec<ArbitrageOpportunity>,
    total_swappa: usize,
    total_newegg: usize,
    total_ebay_sold: usize,
    best_opportunity: Option<ArbitrageOpportunity>,
}

// Structure for frontend data export with history
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ScraperData {
    last_updated: String,
    run_count: u32,
    newegg_products: Vec<Product>,
    swappa_products: Vec<Product>,
    ebay_products: Vec<Product>,
    arbitrage_opportunities: Vec<ArbitrageOpportunity>,
    total_tracked: usize,
    // New: Run history
    run_history: Vec<RunSnapshot>,
}

// File paths
const SEEN_PRODUCTS_FILE: &str = "seen_products.json";
const FRONTEND_DATA_FILE: &str = "scraper_data.json";
const MAX_HISTORY_RUNS: usize = 20; // Keep last 20 runs

// Load existing frontend data (for history)
fn load_frontend_data() -> Option<ScraperData> {
    match fs::read_to_string(FRONTEND_DATA_FILE) {
        Ok(content) => serde_json::from_str(&content).ok(),
        Err(_) => None,
    }
}

// Save data for frontend
fn save_frontend_data(data: &ScraperData) {
    if let Ok(json) = serde_json::to_string_pretty(data) {
        if let Err(e) = fs::write(FRONTEND_DATA_FILE, json) {
            eprintln!("Failed to write frontend data: {}", e);
        } else {
            println!("📁 Frontend data saved to {}", FRONTEND_DATA_FILE);
        }
    }
}

// Create products with eBay comparison data
fn create_products_with_comparison(
    swappa_products: &[Product],
    ebay_sold: &[Product],
) -> Vec<ProductWithComparison> {
    let mut products_with_comp = Vec::new();
    
    for product in swappa_products {
        let price_numeric = parse_price(&product.price).unwrap_or(0.0);
        
        // Find similar eBay sold items
        let mut similar_sold: Vec<f64> = Vec::new();
        for sold in ebay_sold {
            let score = similarity_score(product, sold);
            if score >= 40.0 {
                if let Some(sold_price) = parse_price(&sold.price) {
                    if sold_price > 50.0 {
                        similar_sold.push(sold_price);
                    }
                }
            }
        }
        
        let (ebay_avg, ebay_count, ebay_range, profit, margin) = if similar_sold.len() >= 2 {
            let avg = similar_sold.iter().sum::<f64>() / similar_sold.len() as f64;
            let min = similar_sold.iter().cloned().fold(f64::INFINITY, f64::min);
            let max = similar_sold.iter().cloned().fold(0.0, f64::max);
            let profit = avg - price_numeric;
            let margin = if price_numeric > 0.0 { (profit / price_numeric) * 100.0 } else { 0.0 };
            (
                Some(avg),
                Some(similar_sold.len()),
                Some(format!("${:.2} - ${:.2}", min, max)),
                Some(profit),
                Some(margin),
            )
        } else {
            (None, None, None, None, None)
        };
        
        products_with_comp.push(ProductWithComparison {
            name: product.name.clone(),
            price: product.price.clone(),
            price_numeric,
            url: product.url.clone(),
            source: product.source.clone(),
            ebay_avg_sold: ebay_avg,
            ebay_sold_count: ebay_count,
            ebay_price_range: ebay_range,
            potential_profit: profit,
            margin_percent: margin,
        });
    }
    
    products_with_comp
}

// Convert PriceComparison to ArbitrageOpportunity for frontend export
fn convert_to_arbitrage_opportunities(comparisons: &[PriceComparison]) -> Vec<ArbitrageOpportunity> {
    let mut opportunities = Vec::new();
    
    for comparison in comparisons {
        opportunities.push(ArbitrageOpportunity {
            buy_product_name: comparison.source_product.name.clone(),
            buy_source: comparison.source_product.source.clone(),
            buy_price: comparison.source_price,
            buy_url: comparison.source_product.url.clone(),
            ebay_avg_sold_price: comparison.ebay_avg_sold,
            ebay_sold_count: comparison.ebay_sold_count,
            ebay_price_range: format!("${:.2} - ${:.2}", comparison.ebay_min_price, comparison.ebay_max_price),
            potential_profit: comparison.profit,
            margin_percent: comparison.margin_percent,
            sample_ebay_urls: comparison.sample_ebay_urls.clone(),
        });
    }
    
    // Sort by profit descending
    opportunities.sort_by(|a, b| b.potential_profit.partial_cmp(&a.potential_profit).unwrap_or(std::cmp::Ordering::Equal));
    opportunities
}

// Generate a unique key for a product (using URL as primary key for deduplication)
fn product_key(product: &Product) -> String {
    // Use URL as the primary key - this ensures same listing isn't duplicated
    // Strip query params for cleaner comparison
    let url_clean = product.url.split('?').next().unwrap_or(&product.url);
    format!("{}|{}", product.source, url_clean)
}

// Deduplicate products by URL
fn deduplicate_products(products: Vec<Product>) -> Vec<Product> {
    let mut seen_urls: HashSet<String> = HashSet::new();
    let mut unique_products = Vec::new();
    
    for product in products {
        let url_clean = product.url.split('?').next().unwrap_or(&product.url).to_string();
        if !seen_urls.contains(&url_clean) {
            seen_urls.insert(url_clean);
            unique_products.push(product);
        }
    }
    
    unique_products
}

// Load seen products from JSON file
fn load_seen_products() -> HashSet<String> {
    match fs::read_to_string(SEEN_PRODUCTS_FILE) {
        Ok(content) => {
            serde_json::from_str(&content).unwrap_or_else(|_| HashSet::new())
        }
        Err(_) => HashSet::new()
    }
}

// Save seen products to JSON file
fn save_seen_products(seen: &HashSet<String>) {
    if let Ok(json) = serde_json::to_string_pretty(seen) {
        let _ = fs::write(SEEN_PRODUCTS_FILE, json);
    }
}

// Filter products to only return new ones and update seen set
fn filter_new_products(products: Vec<Product>, seen: &mut HashSet<String>) -> Vec<Product> {
    let mut new_products = Vec::new();
    
    for product in products {
        let key = product_key(&product);
        if !seen.contains(&key) {
            seen.insert(key);
            new_products.push(product);
        }
    }
    
    new_products
}

// Parse price string to f64
fn parse_price(price_str: &str) -> Option<f64> {
    // Remove currency symbols, commas, and extra whitespace
    let cleaned: String = price_str
        .replace('$', "")
        .replace(',', "")
        .replace(" ", "")
        .trim()
        .chars()
        .take_while(|c| c.is_digit(10) || *c == '.')
        .collect();
    
    cleaned.parse::<f64>().ok()
}

// Extract key product identifiers from name (model numbers, brand, etc.)
fn extract_keywords(name: &str) -> Vec<String> {
    let name_lower = name.to_lowercase();
    
    // Common phone models and keywords to match
    let keywords: Vec<&str> = vec![
        // iPhones
        "iphone 16 pro max", "iphone 16 pro", "iphone 16", "iphone 16e",
        "iphone 15 pro max", "iphone 15 pro", "iphone 15 plus", "iphone 15",
        "iphone 14 pro max", "iphone 14 pro", "iphone 14 plus", "iphone 14",
        "iphone 13 pro max", "iphone 13 pro", "iphone 13 mini", "iphone 13",
        "iphone 12 pro max", "iphone 12 pro", "iphone 12 mini", "iphone 12",
        "iphone se",
        // Samsung
        "galaxy s24 ultra", "galaxy s24+", "galaxy s24",
        "galaxy s23 ultra", "galaxy s23+", "galaxy s23",
        "galaxy z fold", "galaxy z flip",
        "galaxy a54", "galaxy a34", "galaxy a14",
        // Google Pixel
        "pixel 9 pro xl", "pixel 9 pro", "pixel 9",
        "pixel 8 pro", "pixel 8a", "pixel 8",
        "pixel 7 pro", "pixel 7a", "pixel 7",
        // Storage sizes
        "128gb", "256gb", "512gb", "1tb",
        // Conditions
        "unlocked",
    ];
    
    let mut found_keywords = Vec::new();
    for kw in keywords {
        if name_lower.contains(kw) {
            found_keywords.push(kw.to_string());
        }
    }
    
    found_keywords
}

// Calculate similarity score between two products
fn similarity_score(p1: &Product, p2: &Product) -> f64 {
    let kw1 = extract_keywords(&p1.name);
    let kw2 = extract_keywords(&p2.name);
    
    if kw1.is_empty() || kw2.is_empty() {
        return 0.0;
    }
    
    let mut matches = 0;
    for k in &kw1 {
        if kw2.contains(k) {
            matches += 1;
        }
    }
    
    // Higher weight for phone model matches
    let phone_models = ["iphone", "galaxy", "pixel"];
    let mut model_match = false;
    for model in phone_models {
        let p1_has = p1.name.to_lowercase().contains(model);
        let p2_has = p2.name.to_lowercase().contains(model);
        if p1_has && p2_has {
            model_match = true;
            break;
        }
    }
    
    if !model_match {
        return 0.0;
    }
    
    // Calculate score based on keyword matches
    let max_keywords = kw1.len().max(kw2.len()) as f64;
    (matches as f64 / max_keywords) * 100.0
}

#[derive(Debug, Clone)]
struct PriceComparison {
    product_name: String,
    source_product: Product,
    source_price: f64,
    ebay_avg_sold: f64,
    ebay_sold_count: usize,
    ebay_min_price: f64,
    ebay_max_price: f64,
    sample_ebay_urls: Vec<String>,
    profit: f64,
    margin_percent: f64,
}

// Find arbitrage opportunities by comparing Swappa prices to eBay SOLD averages
fn find_arbitrage_opportunities(
    _newegg: &[Product],  // Not using Newegg for comparison anymore
    swappa: &[Product],
    ebay_sold: &[Product],
) -> Vec<PriceComparison> {
    let mut opportunities = Vec::new();
    
    // Only use Swappa as buy source
    for buy_product in swappa {
        if let Some(buy_price) = parse_price(&buy_product.price) {
            if buy_price < 50.0 {
                continue; // Skip very low priced items
            }
            
            // Find similar eBay SOLD items and calculate average
            let mut similar_sold: Vec<(f64, String)> = Vec::new();
            
            for sold_product in ebay_sold {
                let score = similarity_score(buy_product, sold_product);
                if score >= 40.0 {  // Lower threshold since we're matching sold items
                    if let Some(sold_price) = parse_price(&sold_product.price) {
                        if sold_price > 50.0 {  // Filter out accessories/parts
                            similar_sold.push((sold_price, sold_product.url.clone()));
                        }
                    }
                }
            }
            
            // Need at least 2 sold items to calculate meaningful average
            if similar_sold.len() >= 2 {
                let prices: Vec<f64> = similar_sold.iter().map(|(p, _)| *p).collect();
                let avg_sold = prices.iter().sum::<f64>() / prices.len() as f64;
                let min_price = prices.iter().cloned().fold(f64::INFINITY, f64::min);
                let max_price = prices.iter().cloned().fold(0.0, f64::max);
                
                // Calculate profit based on average sold price
                let profit = avg_sold - buy_price;
                let margin_percent = (profit / buy_price) * 100.0;
                
                // Only include if there's meaningful profit (> 10%)
                if margin_percent > 10.0 && profit > 20.0 {
                    let sample_urls: Vec<String> = similar_sold.iter()
                        .take(3)
                        .map(|(_, url)| url.clone())
                        .collect();
                    
                    opportunities.push(PriceComparison {
                        product_name: buy_product.name.clone(),
                        source_product: buy_product.clone(),
                        source_price: buy_price,
                        ebay_avg_sold: avg_sold,
                        ebay_sold_count: similar_sold.len(),
                        ebay_min_price: min_price,
                        ebay_max_price: max_price,
                        sample_ebay_urls: sample_urls,
                        profit,
                        margin_percent,
                    });
                }
            }
        }
    }
    
    // Sort opportunities by profit descending
    opportunities.sort_by(|a, b| {
        b.profit.partial_cmp(&a.profit).unwrap_or(std::cmp::Ordering::Equal)
    });
    
    opportunities
}

// Display arbitrage opportunities
fn display_arbitrage_opportunities(opportunities: &[PriceComparison]) {
    if opportunities.is_empty() {
        println!("\n  ℹ️  No arbitrage opportunities found this run");
        println!("     (Need similar items sold on eBay to compare prices)");
        return;
    }
    
    println!("\n📋 ARBITRAGE OPPORTUNITIES ({}):", opportunities.len());
    println!("   Comparing Swappa prices to eBay SOLD averages\n");
    
    for (i, opp) in opportunities.iter().take(15).enumerate() {
        println!("{}. {}", i + 1, truncate_string(&opp.product_name, 60));
        println!("   📥 BUY ON SWAPPA: ${:.2}", opp.source_price);
        println!("      🔗 {}", opp.source_product.url);
        println!("   📊 EBAY SOLD DATA ({} recent sales):", opp.ebay_sold_count);
        println!("      Average: ${:.2}", opp.ebay_avg_sold);
        println!("      Range: ${:.2} - ${:.2}", opp.ebay_min_price, opp.ebay_max_price);
        println!("   💵 POTENTIAL PROFIT: ${:.2} ({:.1}% margin)", opp.profit, opp.margin_percent);
        if !opp.sample_ebay_urls.is_empty() {
            println!("   🔗 Sample sold listings:");
            for url in &opp.sample_ebay_urls {
                println!("      {}", url);
            }
        }
        println!();
    }
}

fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() > max_len {
        format!("{}...", &s[..max_len])
    } else {
        s.to_string()
    }
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
        
        // Extract ALL individual listings from the page using text scanning
        let category_name = *category;
        let base_url = *url;
        let script = format!(r#"
            var products = [];
            var categoryName = "{}";
            var baseUrl = "{}";
            var seenKeys = new Set();
            var method = 'text-scan';
            var listingIndex = 0;
            
            // Find listing rows/cards with price information
            // Swappa typically shows listings as rows with price, condition, storage info
            var cards = document.querySelectorAll('[class*="listing"], [class*="item"], [class*="card"], [class*="row"], [class*="product"], article, [data-listing], tr, [role="row"]');
            
            for (var i = 0; i < cards.length && products.length < 30; i++) {{
                var card = cards[i];
                var text = card.innerText || '';
                var priceMatch = text.match(/\$(\d{{2,4}})/);
                
                if (priceMatch) {{
                    var priceNum = parseInt(priceMatch[1]);
                    // Filter to reasonable phone prices
                    if (priceNum >= 100 && priceNum <= 1500) {{
                        var price = '$' + priceMatch[1];
                        
                        // Find best link in card - prefer prices or listing links
                        var cardAnchors = card.querySelectorAll('a');
                        var href = '';
                        
                        for (var j = 0; j < cardAnchors.length; j++) {{
                            var linkHref = cardAnchors[j].href || '';
                            // Prefer prices page or listing-specific URLs
                            if (linkHref.includes('/prices/') || linkHref.includes('/listing/')) {{
                                href = linkHref;
                                break;
                            }}
                            // Fallback to guide page
                            if (!href && linkHref.includes('/guide/') && !linkHref.includes('/reviews')) {{
                                href = linkHref;
                            }}
                        }}
                        
                        // If no good link, use baseUrl with listing index for tracking
                        if (!href) {{
                            href = baseUrl;
                        }}
                        
                        // Extract condition
                        var condition = '';
                        if (text.includes('Mint')) condition = 'Mint';
                        else if (text.includes('Good')) condition = 'Good';
                        else if (text.includes('Fair')) condition = 'Fair';
                        
                        // Extract storage
                        var storage = '';
                        var storageMatch = text.match(/(\d{{2,3}})\s*GB/i);
                        if (storageMatch) storage = storageMatch[1] + 'GB';
                        
                        // Extract carrier/unlock status
                        var carrier = '';
                        if (text.includes('Unlocked')) carrier = 'Unlocked';
                        else if (text.includes('Verizon')) carrier = 'Verizon';
                        else if (text.includes('T-Mobile')) carrier = 'T-Mobile';
                        else if (text.includes('AT&T')) carrier = 'AT&T';
                        
                        // Create unique key based on card content to avoid duplicates
                        var key = price + '-' + storage + '-' + condition + '-' + i;
                        if (!seenKeys.has(key)) {{
                            seenKeys.add(key);
                            method = 'cards';
                            listingIndex++;
                            
                            // Build descriptive name
                            var name = categoryName;
                            if (storage) name += ' ' + storage;
                            if (carrier) name += ' ' + carrier;
                            if (condition) name += ' (' + condition + ')';
                            
                            products.push({{
                                name: name,
                                price: price,
                                condition: condition,
                                storage: storage,
                                carrier: carrier,
                                url: href,
                                listingNum: listingIndex
                            }});
                        }}
                    }}
                }}
            }}
            
            // Fallback: Text scanning if no cards found
            if (products.length == 0) {{
                var bodyText = document.body.innerText;
                var textLines = bodyText.split('\n');
                
                for (var i = 0; i < textLines.length && products.length < 30; i++) {{
                    var line = textLines[i].trim();
                    var priceMatch = line.match(/\$(\d{{2,4}})/);
                    
                    if (priceMatch) {{
                        var priceNum = parseInt(priceMatch[1]);
                        if (priceNum >= 100 && priceNum <= 1500) {{
                            listingIndex++;
                            var price = '$' + priceMatch[1];
                            
                            var condition = '';
                            var storage = '';
                            var contextText = textLines.slice(Math.max(0, i-3), i+3).join(' ');
                            
                            if (contextText.includes('Mint')) condition = 'Mint';
                            else if (contextText.includes('Good')) condition = 'Good';
                            else if (contextText.includes('Fair')) condition = 'Fair';
                            
                            var storageMatch = contextText.match(/(\d{{2,3}})\s*GB/i);
                            if (storageMatch) storage = storageMatch[1] + 'GB';
                            
                            var key = 'line-' + i + '-' + price;
                            if (!seenKeys.has(key)) {{
                                seenKeys.add(key);
                                
                                var name = categoryName;
                                if (storage) name += ' ' + storage;
                                if (condition) name += ' (' + condition + ')';
                                
                                products.push({{
                                    name: name,
                                    price: price,
                                    condition: condition,
                                    storage: storage,
                                    carrier: '',
                                    url: baseUrl,
                                    listingNum: listingIndex
                                }});
                            }}
                        }}
                    }}
                }}
            }}
            
            return {{ 
                products: products, 
                total: products.length, 
                method: method
            }};
            "#, category_name, base_url);
        
        let products_result = driver.execute(&script, vec![]).await;
        
        if let Ok(result_value) = products_result {
            let json = result_value.json();
            
            // Get total found
            let total = json.get("total").and_then(|v| v.as_u64()).unwrap_or(0);
            let method = json.get("method").and_then(|v| v.as_str()).unwrap_or("unknown");
            
            println!("    🔍 Found {} listings (via {})", total, method);
            
            // Get all products
            if let Some(products_arr) = json.get("products").and_then(|v| v.as_array()) {
                let mut added_count = 0;
                for product in products_arr {
                    let name = product.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let price = product.get("price").and_then(|v| v.as_str()).unwrap_or("");
                    let prod_url = product.get("url").and_then(|v| v.as_str()).unwrap_or("");
                    
                    if !name.is_empty() && !price.is_empty() {
                        let final_url = if !prod_url.is_empty() {
                            prod_url.to_string()
                        } else {
                            url.to_string()
                        };
                        
                        // Don't filter duplicates by name - allow same model with different conditions/prices
                        all_products.push(Product {
                            name: name.to_string(),
                            price: price.to_string(),
                            url: final_url,
                            source: "Swappa".to_string(),
                        });
                        added_count += 1;
                    }
                }
                if added_count > 0 {
                    println!("    ✅ Added {} listings from {}", added_count, category);
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

async fn scrape_ebay(_client: &reqwest::Client) -> Vec<Product> {
    let mut all_products = Vec::new();
    
    println!("  Starting Selenium WebDriver for eBay...");
    
    // Set up Chrome options - with extra measures to avoid detection
    let mut caps = DesiredCapabilities::chrome();
    caps.add_arg("--headless=new").ok();
    caps.add_arg("--disable-gpu").ok();
    caps.add_arg("--no-sandbox").ok();
    caps.add_arg("--disable-dev-shm-usage").ok();
    caps.add_arg("--window-size=1920,1200").ok();
    caps.add_arg("--disable-blink-features=AutomationControlled").ok();
    caps.add_arg("--disable-web-security").ok();
    caps.add_arg("--disable-features=VizDisplayCompositor").ok();
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
    let screenshot_dir = "/tmp/ebay_screenshots";
    let _ = fs::create_dir_all(screenshot_dir);
    
    // eBay SOLD listings URLs - LH_Complete=1&LH_Sold=1 shows recently sold items
    let urls: Vec<(&str, &str)> = vec![
        // Phones - SOLD listings
        ("iPhone 15", "https://www.ebay.com/sch/i.html?_nkw=iphone+15+unlocked&_sacat=9355&LH_Sold=1&LH_Complete=1&_sop=13"),
        ("iPhone 14", "https://www.ebay.com/sch/i.html?_nkw=iphone+14+unlocked&_sacat=9355&LH_Sold=1&LH_Complete=1&_sop=13"),
        ("iPhone 13", "https://www.ebay.com/sch/i.html?_nkw=iphone+13+unlocked&_sacat=9355&LH_Sold=1&LH_Complete=1&_sop=13"),
        ("Galaxy S24", "https://www.ebay.com/sch/i.html?_nkw=samsung+galaxy+s24+unlocked&_sacat=9355&LH_Sold=1&LH_Complete=1&_sop=13"),
        ("Galaxy S23", "https://www.ebay.com/sch/i.html?_nkw=samsung+galaxy+s23+unlocked&_sacat=9355&LH_Sold=1&LH_Complete=1&_sop=13"),
        ("Pixel 8", "https://www.ebay.com/sch/i.html?_nkw=google+pixel+8+unlocked&_sacat=9355&LH_Sold=1&LH_Complete=1&_sop=13"),
        ("Pixel 7", "https://www.ebay.com/sch/i.html?_nkw=google+pixel+7+unlocked&_sacat=9355&LH_Sold=1&LH_Complete=1&_sop=13"),
    ];
    
    for (category, url) in urls.iter() {
        println!("  🛍️ Scraping eBay {}: {}", category, url);
        
        if let Err(e) = driver.goto(*url).await {
            eprintln!("    ❌ Failed to navigate to {}: {}", url, e);
            continue;
        }
        
        // Wait for page to load
        sleep(Duration::from_secs(5)).await;
        
        // Scroll to load more content
        for i in 0..6 {
            let scroll_pos = (i + 1) * 800;
            let _ = driver.execute(&format!("window.scrollTo(0, {})", scroll_pos), vec![]).await;
            sleep(Duration::from_millis(600)).await;
        }
        
        // Scroll back up
        let _ = driver.execute("window.scrollTo(0, 0)", vec![]).await;
        sleep(Duration::from_secs(1)).await;
        
        // Take screenshot
        let screenshot_path = format!("{}/{}.png", screenshot_dir, category.replace(" ", "_"));
        if let Ok(png_data) = driver.screenshot_as_png().await {
            if fs::write(&screenshot_path, &png_data).is_ok() {
                println!("    📸 Screenshot saved: {}", screenshot_path);
            }
        }
        
        // Extract products using JavaScript - updated selectors for eBay 2026
        let script = r#"
            var products = [];
            var seenUrls = new Set();
            var debug = { selectors: [] };
            
            // Updated selector for eBay's new s-card structure
            var items = document.querySelectorAll('ul.srp-results li.s-card');
            debug.itemsChecked = items.length;
            debug.winningSelector = 'ul.srp-results li.s-card';
            
            for (var i = 0; i < items.length && products.length < 50; i++) {
                var item = items[i];
                
                // Get title from s-card__title
                var titleEl = item.querySelector('.s-card__title span');
                var name = titleEl ? titleEl.innerText.trim() : '';
                
                // Clean up title - remove "NEW LISTING" prefix
                name = name.replace(/^NEW LISTING/i, '').trim();
                
                // Skip invalid names
                if (!name || name.length < 10 || name.toLowerCase().includes('shop on ebay')) continue;
                
                // Get price from s-card__price
                var priceEl = item.querySelector('.s-card__price');
                var price = '';
                if (priceEl) {
                    var priceText = priceEl.innerText.trim();
                    var priceMatch = priceText.match(/\$[\d,]+\.?\d{0,2}/);
                    if (priceMatch) {
                        price = priceMatch[0];
                    }
                }
                
                // Get URL from s-card__link with /itm/
                var linkEl = item.querySelector('a.s-card__link[href*="/itm/"]');
                if (!linkEl) {
                    linkEl = item.querySelector('a[href*="/itm/"]');
                }
                var href = linkEl ? linkEl.href : '';
                
                // Validate and add product
                if (name && name.length > 5 && price && href && href.includes('/itm/')) {
                    // Clean up URL - remove tracking params
                    var cleanUrl = href.split('?')[0];
                    if (!seenUrls.has(cleanUrl)) {
                        seenUrls.add(cleanUrl);
                        products.push({
                            name: name.substring(0, 200),
                            price: price,
                            url: cleanUrl
                        });
                    }
                }
            }
            
            debug.productsFound = products.length;
            return { products: products, total: products.length, debug: debug };
        "#;
        
        let products_result = driver.execute(script, vec![]).await;
        
        if let Ok(result_value) = products_result {
            let json = result_value.json();
            let total = json.get("total").and_then(|v| v.as_u64()).unwrap_or(0);
            
            // Debug info
            if let Some(debug) = json.get("debug") {
                let items_checked = debug.get("itemsChecked").and_then(|v| v.as_u64()).unwrap_or(0);
                let winning_selector = debug.get("winningSelector").and_then(|v| v.as_str()).unwrap_or("none");
                println!("    🔍 Found {} products (checked {} items, selector: {})", total, items_checked, winning_selector);
            } else {
                println!("    🔍 Found {} products", total);
            }
            
            if let Some(products_arr) = json.get("products").and_then(|v| v.as_array()) {
                let mut added_count = 0;
                for product in products_arr {
                    let name = product.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let price = product.get("price").and_then(|v| v.as_str()).unwrap_or("");
                    let prod_url = product.get("url").and_then(|v| v.as_str()).unwrap_or("");
                    
                    if !name.is_empty() && !price.is_empty() && !prod_url.is_empty() {
                        all_products.push(Product {
                            name: name.to_string(),
                            price: price.to_string(),
                            url: prod_url.to_string(),
                            source: "eBay".to_string(),
                        });
                        added_count += 1;
                    }
                }
                if added_count > 0 {
                    println!("    ✅ Added {} products from {}", added_count, category);
                }
            }
        }
        
        sleep(Duration::from_secs(2)).await;
    }
    
    // Close browser
    if let Err(e) = driver.quit().await {
        eprintln!("  Warning: Failed to close browser: {}", e);
    }
    
    // Deduplicate
    all_products.sort_by(|a, b| a.name.cmp(&b.name));
    all_products.dedup_by(|a, b| a.name == b.name);
    
    println!("  ✓ eBay scraping complete. Found {} products", all_products.len());
    println!("  📁 Screenshots saved to: {}", screenshot_dir);
    
    all_products
}

#[tokio::main]
async fn main() {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("Failed to create HTTP client");

    println!("🛒 Product Scraper - Newegg, Swappa & eBay");
    println!("⏰ Running every 1 minute. Press Ctrl+C to stop.");
    println!("📁 Tracking seen products in: {}\n", SEEN_PRODUCTS_FILE);
    
    // Load previously seen products
    let mut seen_products = load_seen_products();
    println!("📊 Loaded {} previously seen products\n", seen_products.len());
    
    let mut run_count = 0;
    
    loop {
        run_count += 1;
        let now = chrono::Local::now();
        
        println!("\n{}", "=".repeat(60));
        println!("🔄 SCRAPE RUN #{} - {}", run_count, now.format("%Y-%m-%d %H:%M:%S"));
        println!("{}", "=".repeat(60));

        // Scrape Newegg
        println!("\n📦 Scraping Newegg...\n");
        let all_newegg_products = deduplicate_products(scrape_newegg(&client).await);
        let newegg_products = filter_new_products(all_newegg_products.clone(), &mut seen_products);
        
        println!("\n{}", "-".repeat(60));
        println!("NEWEGG: {} total, {} NEW", all_newegg_products.len(), newegg_products.len());
        println!("{}", "-".repeat(60));
        
        // Always show all scraped items with links
        if !all_newegg_products.is_empty() {
            println!("\n📋 ALL SCRAPED NEWEGG ITEMS ({}):", all_newegg_products.len());
            for (i, product) in all_newegg_products.iter().enumerate() {
                println!("\n{}. {}", i + 1, product.name);
                println!("   💰 Price: {}", product.price);
                println!("   🔗 {}", product.url);
            }
        }
        
        if newegg_products.is_empty() {
            println!("\n  ℹ️  No new Newegg products found this run");
        } else {
            println!("\n🆕 NEW NEWEGG PRODUCTS:");
            for (i, product) in newegg_products.iter().take(15).enumerate() {
                println!("\n{}. {}", i + 1, product.name);
                println!("   💰 Price: {}", product.price);
                println!("   🔗 {}", product.url);
            }
        }

        // Fetch detailed info for new Newegg products
        let newegg_details = if !newegg_products.is_empty() {
            fetch_product_details(&client, &newegg_products, 5).await
        } else {
            Vec::new()
        };
        
        if !newegg_details.is_empty() {
            println!("\n{}", "=".repeat(60));
            println!("📦 NEW NEWEGG DETAILED PRODUCTS ({})", newegg_details.len());
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
        }

        sleep(Duration::from_millis(2000)).await;

        // Scrape Swappa
        println!("\n\n📱 Scraping Swappa...\n");
        let all_swappa_products = deduplicate_products(scrape_swappa(&client).await);
        let swappa_products = filter_new_products(all_swappa_products.clone(), &mut seen_products);
        
        println!("\n{}", "-".repeat(60));
        println!("SWAPPA: {} total, {} NEW", all_swappa_products.len(), swappa_products.len());
        println!("{}", "-".repeat(60));
        
        // Always show all scraped items with links
        if !all_swappa_products.is_empty() {
            println!("\n📋 ALL SCRAPED SWAPPA ITEMS ({}):", all_swappa_products.len());
            for (i, product) in all_swappa_products.iter().enumerate() {
                println!("\n{}. {}", i + 1, product.name);
                println!("   💰 Price: {}", product.price);
                println!("   🔗 {}", product.url);
            }
        }
        
        if swappa_products.is_empty() {
            println!("\n  ℹ️  No new Swappa products found this run");
        } else {
            println!("\n🆕 NEW SWAPPA PRODUCTS:");
            for (i, product) in swappa_products.iter().take(15).enumerate() {
                println!("\n{}. {}", i + 1, product.name);
                println!("   💰 Price: {}", product.price);
                println!("   🔗 {}", product.url);
            }
        }

        // Fetch detailed info for new Swappa products using Selenium
        let swappa_details = if !swappa_products.is_empty() {
            fetch_swappa_details_selenium(&swappa_products, 5).await
        } else {
            Vec::new()
        };
        
        if !swappa_details.is_empty() {
            println!("\n{}", "=".repeat(60));
            println!("📱 NEW SWAPPA DETAILED PRODUCTS ({})", swappa_details.len());
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
        }

        sleep(Duration::from_millis(2000)).await;

        // Scrape eBay
        println!("\n\n🛍️ Scraping eBay...\n");
        let all_ebay_products = deduplicate_products(scrape_ebay(&client).await);
        let ebay_products = filter_new_products(all_ebay_products.clone(), &mut seen_products);
        
        println!("\n{}", "-".repeat(60));
        println!("EBAY: {} total, {} NEW", all_ebay_products.len(), ebay_products.len());
        println!("{}", "-".repeat(60));
        
        // Always show all scraped items with links
        if !all_ebay_products.is_empty() {
            println!("\n📋 ALL SCRAPED EBAY ITEMS ({}):", all_ebay_products.len());
            for (i, product) in all_ebay_products.iter().enumerate() {
                println!("\n{}. {}", i + 1, product.name);
                println!("   💰 Price: {}", product.price);
                println!("   🔗 {}", product.url);
            }
        }
        
        if ebay_products.is_empty() {
            println!("\n  ℹ️  No new eBay products found this run");
        } else {
            println!("\n🆕 NEW EBAY PRODUCTS:");
            for (i, product) in ebay_products.iter().take(15).enumerate() {
                println!("\n{}. {}", i + 1, product.name);
                println!("   💰 Price: {}", product.price);
                println!("   🔗 {}", product.url);
            }
        }

        // Price Comparison & Arbitrage Analysis
        println!("\n\n{}", "=".repeat(60));
        println!("💰 PRICE COMPARISON & PROFIT MARGINS");
        println!("{}", "=".repeat(60));
        
        let arbitrage_opportunities = find_arbitrage_opportunities(
            &all_newegg_products,
            &all_swappa_products,
            &all_ebay_products,
        );
        
        display_arbitrage_opportunities(&arbitrage_opportunities);
        
        // Show best deals summary
        if !arbitrage_opportunities.is_empty() {
            println!("\n🏆 TOP 5 BEST PROFIT OPPORTUNITIES:");
            for (i, opp) in arbitrage_opportunities.iter().take(5).enumerate() {
                println!("   {}. ${:.2} potential profit ({:.1}%) - {}", 
                    i + 1, opp.profit, opp.margin_percent, truncate_string(&opp.product_name, 40));
            }
        }

        // Save seen products after each run
        save_seen_products(&seen_products);

        // Save data for frontend with run history
        let frontend_arbitrage = convert_to_arbitrage_opportunities(&arbitrage_opportunities);
        let swappa_with_comparison = create_products_with_comparison(&all_swappa_products, &all_ebay_products);
        let newegg_with_comparison = create_products_with_comparison(&all_newegg_products, &all_ebay_products);
        
        // Create current run snapshot
        let current_run = RunSnapshot {
            run_id: run_count,
            timestamp: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            swappa_products: swappa_with_comparison,
            newegg_products: newegg_with_comparison,
            ebay_sold_products: all_ebay_products.clone(),
            arbitrage_opportunities: frontend_arbitrage.clone(),
            total_swappa: all_swappa_products.len(),
            total_newegg: all_newegg_products.len(),
            total_ebay_sold: all_ebay_products.len(),
            best_opportunity: frontend_arbitrage.first().cloned(),
        };
        
        // Load existing history and append
        let mut run_history = if let Some(existing) = load_frontend_data() {
            existing.run_history
        } else {
            Vec::new()
        };
        
        run_history.push(current_run);
        
        // Keep only last MAX_HISTORY_RUNS
        if run_history.len() > MAX_HISTORY_RUNS {
            let skip_count = run_history.len() - MAX_HISTORY_RUNS;
            run_history = run_history.into_iter().skip(skip_count).collect();
        }
        
        let frontend_data = ScraperData {
            last_updated: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            run_count,
            newegg_products: all_newegg_products.clone(),
            swappa_products: all_swappa_products.clone(),
            ebay_products: all_ebay_products.clone(),
            arbitrage_opportunities: frontend_arbitrage,
            total_tracked: seen_products.len(),
            run_history,
        };
        save_frontend_data(&frontend_data);

        // Summary
        println!("\n\n{}", "=".repeat(60));
        println!("📊 SUMMARY - Run #{}", run_count);
        println!("{}", "=".repeat(60));
        println!("Newegg: {} total scraped, {} NEW", all_newegg_products.len(), newegg_products.len());
        println!("Swappa: {} total scraped, {} NEW", all_swappa_products.len(), swappa_products.len());
        println!("eBay: {} total scraped, {} NEW", all_ebay_products.len(), ebay_products.len());
        println!("Total NEW this run: {}", newegg_products.len() + swappa_products.len() + ebay_products.len());
        println!("Total products tracked: {}", seen_products.len());
        
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