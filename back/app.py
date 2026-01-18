"""
M3 Movement - Product Scraper Dashboard
A Streamlit frontend for the Rust-based product scraper
"""

import streamlit as st
import json
import os
import pandas as pd
from datetime import datetime
import time

# Page config
st.set_page_config(
    page_title="M3 Movement - Deal Finder",
    page_icon="💰",
    layout="wide",
    initial_sidebar_state="expanded"
)

# Custom CSS
st.markdown("""
<style>
    .big-font {
        font-size: 24px !important;
        font-weight: bold;
    }
    .profit-positive {
        color: #00ff00;
        font-weight: bold;
    }
    .profit-high {
        color: #00ff00;
        background-color: #003300;
        padding: 2px 8px;
        border-radius: 4px;
    }
    .source-newegg {
        background-color: #ff6600;
        color: white;
        padding: 2px 8px;
        border-radius: 4px;
    }
    .source-swappa {
        background-color: #00a651;
        color: white;
        padding: 2px 8px;
        border-radius: 4px;
    }
    .source-ebay {
        background-color: #e53238;
        color: white;
        padding: 2px 8px;
        border-radius: 4px;
    }
    .stMetric {
        background-color: #1e1e1e;
        padding: 15px;
        border-radius: 10px;
    }
</style>
""", unsafe_allow_html=True)

# Get the directory where the app.py file is located
APP_DIR = os.path.dirname(os.path.abspath(__file__))
DATA_FILE = os.path.join(APP_DIR, "scraper_data.json")

def load_data():
    """Load scraper data from JSON file"""
    if os.path.exists(DATA_FILE):
        try:
            with open(DATA_FILE, 'r') as f:
                return json.load(f)
        except json.JSONDecodeError:
            return None
    return None

def format_price(price_str):
    """Extract numeric price from string"""
    if isinstance(price_str, (int, float)):
        return f"${price_str:.2f}"
    if not price_str:
        return "N/A"
    # Remove $ and other characters
    cleaned = ''.join(c for c in str(price_str) if c.isdigit() or c == '.')
    try:
        return f"${float(cleaned):.2f}"
    except:
        return price_str

def get_source_badge(source):
    """Return colored badge for source"""
    colors = {
        "Newegg": "#ff6600",
        "Swappa": "#00a651",
        "eBay": "#e53238"
    }
    color = colors.get(source, "#666666")
    return f'<span style="background-color: {color}; color: white; padding: 2px 8px; border-radius: 4px; font-size: 12px;">{source}</span>'

def main():
    # Header
    st.title("💰 M3 Movement - Deal Finder")
    st.markdown("*Real-time product price comparison across Newegg, Swappa, and eBay*")
    
    # Sidebar
    st.sidebar.header("⚙️ Settings")
    auto_refresh = st.sidebar.checkbox("Auto-refresh (30s)", value=False)
    min_profit = st.sidebar.slider("Minimum Profit ($)", 0, 500, 10)
    min_margin = st.sidebar.slider("Minimum Margin (%)", 0, 100, 5)
    
    if st.sidebar.button("🔄 Refresh Now"):
        st.rerun()
    
    # Load data
    data = load_data()
    
    if not data:
        st.warning("⚠️ No data available. Make sure the Rust scraper is running.")
        st.info("""
        **To start the scraper:**
        1. Open a terminal in the `back` folder
        2. Make sure ChromeDriver is running: `chromedriver --port=9515`
        3. Run: `cargo run`
        
        The dashboard will automatically update once data is available.
        """)
        
        if auto_refresh:
            time.sleep(5)
            st.rerun()
        return
    
    # Status bar
    col1, col2, col3, col4, col5 = st.columns(5)
    with col1:
        st.metric("Last Updated", data.get("last_updated", "N/A"))
    with col2:
        st.metric("Run #", data.get("run_count", 0))
    with col3:
        st.metric("Products Tracked", data.get("total_tracked", 0))
    with col4:
        opportunities = data.get("arbitrage_opportunities", [])
        profitable = [o for o in opportunities if o.get("profit", 0) >= min_profit and o.get("margin_percent", 0) >= min_margin]
        st.metric("Opportunities", len(profitable))
    with col5:
        if opportunities:
            best_profit = max(o.get("profit", 0) for o in opportunities)
            st.metric("Best Profit", f"${best_profit:.2f}")
    
    st.divider()
    
    # Tabs
    tab1, tab2, tab3, tab4, tab5 = st.tabs(["🎯 Arbitrage Opportunities", "🟠 Newegg", "🟢 Swappa", "🔴 eBay", "📊 Analytics"])
    
    # Arbitrage Opportunities Tab
    with tab1:
        st.header("🎯 Arbitrage Opportunities")
        st.caption("Swappa prices compared to eBay SOLD averages")
        
        opportunities = data.get("arbitrage_opportunities", [])
        
        if not opportunities:
            st.info("No arbitrage opportunities found yet. Keep the scraper running to find deals!")
        else:
            # Filter opportunities - use potential_profit for new format, profit for old
            filtered = [
                o for o in opportunities 
                if o.get("potential_profit", o.get("profit", 0)) >= min_profit 
                and o.get("margin_percent", 0) >= min_margin
            ]
            
            if not filtered:
                st.warning(f"No opportunities match your criteria (min ${min_profit} profit, {min_margin}% margin)")
            else:
                st.success(f"Found **{len(filtered)}** opportunities matching your criteria!")
                
                for i, opp in enumerate(filtered[:50]):  # Limit to top 50
                    profit = opp.get("potential_profit", opp.get("profit", 0))
                    margin = opp.get("margin_percent", 0)
                    
                    # Color based on profit
                    if profit >= 100:
                        profit_color = "🟢"
                    elif profit >= 50:
                        profit_color = "🟡"
                    else:
                        profit_color = "🟠"
                    
                    buy_name = opp.get('buy_product_name', 'Unknown')
                    
                    with st.expander(f"{profit_color} **${profit:.2f}** potential profit ({margin:.1f}%) - {buy_name[:50]}"):
                        col1, col2 = st.columns(2)
                        
                        with col1:
                            st.markdown("### 🛒 BUY ON SWAPPA")
                            st.markdown(f"**Product:** {buy_name}")
                            st.markdown(f"**Price:** ${opp.get('buy_price', 0):.2f}")
                            st.markdown(f"[🔗 View on Swappa]({opp.get('buy_url', '#')})")
                        
                        with col2:
                            st.markdown("### 📊 EBAY SOLD DATA")
                            ebay_avg = opp.get('ebay_avg_sold_price', 0)
                            sold_count = opp.get('ebay_sold_count', 0)
                            price_range = opp.get('ebay_price_range', 'N/A')
                            
                            st.markdown(f"**Avg Sold Price:** ${ebay_avg:.2f}")
                            st.markdown(f"**Based on:** {sold_count} recent sales")
                            st.markdown(f"**Price Range:** {price_range}")
                            
                            # Show sample eBay URLs
                            sample_urls = opp.get('sample_ebay_urls', [])
                            if sample_urls:
                                st.markdown("**Sample Sold Listings:**")
                                for url in sample_urls[:3]:
                                    st.markdown(f"- [View]({url})")
                        
                        st.divider()
                        col1, col2, col3 = st.columns(3)
                        col1.metric("Potential Profit", f"${profit:.2f}")
                        col2.metric("Margin", f"{margin:.1f}%")
                        col3.metric("ROI", f"{(profit / max(opp.get('buy_price', 1), 1) * 100):.1f}%")
    
    # Newegg Tab
    with tab2:
        st.header("🟠 Newegg Products")
        products = data.get("newegg_products", [])
        st.caption(f"{len(products)} products found")
        
        if products:
            # Convert to DataFrame for better display
            df = pd.DataFrame(products)
            df = df[["name", "price", "url"]]
            df.columns = ["Product Name", "Price", "URL"]
            
            # Search filter
            search = st.text_input("🔍 Search Newegg products", key="newegg_search")
            if search:
                df = df[df["Product Name"].str.contains(search, case=False, na=False)]
            
            for _, row in df.iterrows():
                with st.container():
                    col1, col2, col3 = st.columns([3, 1, 1])
                    col1.write(row["Product Name"][:80])
                    col2.write(row["Price"])
                    col3.markdown(f"[View]({row['URL']})")
        else:
            st.info("No Newegg products scraped yet")
    
    # Swappa Tab
    with tab3:
        st.header("🟢 Swappa Products")
        products = data.get("swappa_products", [])
        st.caption(f"{len(products)} products found")
        
        if products:
            df = pd.DataFrame(products)
            df = df[["name", "price", "url"]]
            df.columns = ["Product Name", "Price", "URL"]
            
            search = st.text_input("🔍 Search Swappa products", key="swappa_search")
            if search:
                df = df[df["Product Name"].str.contains(search, case=False, na=False)]
            
            for _, row in df.iterrows():
                with st.container():
                    col1, col2, col3 = st.columns([3, 1, 1])
                    col1.write(row["Product Name"][:80])
                    col2.write(row["Price"])
                    col3.markdown(f"[View]({row['URL']})")
        else:
            st.info("No Swappa products scraped yet")
    
    # eBay Tab
    with tab4:
        st.header("🔴 eBay Products")
        products = data.get("ebay_products", [])
        st.caption(f"{len(products)} products found")
        
        if products:
            df = pd.DataFrame(products)
            df = df[["name", "price", "url"]]
            df.columns = ["Product Name", "Price", "URL"]
            
            search = st.text_input("🔍 Search eBay products", key="ebay_search")
            if search:
                df = df[df["Product Name"].str.contains(search, case=False, na=False)]
            
            for _, row in df.iterrows():
                with st.container():
                    col1, col2, col3 = st.columns([3, 1, 1])
                    col1.write(row["Product Name"][:80])
                    col2.write(row["Price"])
                    col3.markdown(f"[View]({row['URL']})")
        else:
            st.info("No eBay products scraped yet")
    
    # Analytics Tab
    with tab5:
        st.header("📊 Analytics")
        
        col1, col2 = st.columns(2)
        
        with col1:
            st.subheader("Products by Source")
            source_data = {
                "Newegg": len(data.get("newegg_products", [])),
                "Swappa": len(data.get("swappa_products", [])),
                "eBay": len(data.get("ebay_products", []))
            }
            st.bar_chart(source_data)
        
        with col2:
            st.subheader("Opportunity Distribution")
            opportunities = data.get("arbitrage_opportunities", [])
            if opportunities:
                profit_ranges = {
                    "$0-25": len([o for o in opportunities if 0 <= o.get("profit", 0) < 25]),
                    "$25-50": len([o for o in opportunities if 25 <= o.get("profit", 0) < 50]),
                    "$50-100": len([o for o in opportunities if 50 <= o.get("profit", 0) < 100]),
                    "$100-200": len([o for o in opportunities if 100 <= o.get("profit", 0) < 200]),
                    "$200+": len([o for o in opportunities if o.get("profit", 0) >= 200])
                }
                st.bar_chart(profit_ranges)
            else:
                st.info("No opportunities to analyze yet")
        
        # Summary stats
        st.subheader("Summary Statistics")
        opportunities = data.get("arbitrage_opportunities", [])
        if opportunities:
            profits = [o.get("profit", 0) for o in opportunities]
            margins = [o.get("margin_percent", 0) for o in opportunities]
            
            col1, col2, col3, col4 = st.columns(4)
            col1.metric("Avg Profit", f"${sum(profits)/len(profits):.2f}")
            col2.metric("Max Profit", f"${max(profits):.2f}")
            col3.metric("Avg Margin", f"{sum(margins)/len(margins):.1f}%")
            col4.metric("Max Margin", f"{max(margins):.1f}%")
    
    # Auto-refresh
    if auto_refresh:
        time.sleep(30)
        st.rerun()

if __name__ == "__main__":
    main()
