"""
M3 Movement - Product Scraper Dashboard
Displays scraped products from Newegg, Swappa, and eBay SOLD listings
with historical run tracking and per-product eBay comparison
"""

import streamlit as st
import pandas as pd
import json
import os
from datetime import datetime
import time

# Page configuration
st.set_page_config(
    page_title="M3 Movement - Product Scraper",
    page_icon="📱",
    layout="wide"
)

# Custom CSS
st.markdown("""
<style>
    .profit-positive { color: #00ff00; font-weight: bold; }
    .profit-negative { color: #ff4444; font-weight: bold; }
    .stMetric { background-color: #1e1e1e; padding: 10px; border-radius: 5px; }
    .run-card { 
        background-color: #262626; 
        padding: 15px; 
        border-radius: 10px; 
        margin: 10px 0;
        border-left: 4px solid #4CAF50;
    }
    .product-comparison {
        background-color: #1a1a2e;
        padding: 10px;
        border-radius: 8px;
        margin: 5px 0;
    }
</style>
""", unsafe_allow_html=True)

def load_data():
    """Load scraper data from JSON file"""
    data_file = os.path.join(os.path.dirname(__file__), 'scraper_data.json')
    if os.path.exists(data_file):
        try:
            with open(data_file, 'r') as f:
                return json.load(f)
        except json.JSONDecodeError:
            return None
    return None

def format_currency(value):
    """Format a number as currency"""
    if value is None:
        return "N/A"
    return f"${value:,.2f}"

def calculate_profit_margin(buy_price, sell_price):
    """Calculate profit margin percentage"""
    if buy_price and sell_price and buy_price > 0:
        return ((sell_price - buy_price) / buy_price) * 100
    return None

def main():
    st.title("📱 M3 Movement - Product Scraper Dashboard")
    
    # Load data
    data = load_data()
    
    if not data:
        st.warning("⏳ Waiting for scraper data... Make sure the Rust backend is running.")
        st.info("Run the backend with: `cd back && cargo run`")
        
        # Auto-refresh
        if st.button("🔄 Refresh Data"):
            st.rerun()
        
        time.sleep(5)
        st.rerun()
        return
    
    # Sidebar with run history
    st.sidebar.title("📊 Run History")
    
    run_history = data.get('run_history', [])
    
    if run_history:
        st.sidebar.success(f"📜 {len(run_history)} runs recorded")
        
        # Run selector
        run_options = [f"Run {i+1}: {run.get('timestamp', 'Unknown')[:16]}" 
                      for i, run in enumerate(run_history)]
        
        selected_run_idx = st.sidebar.selectbox(
            "Select a run to view:",
            range(len(run_history)),
            format_func=lambda x: run_options[x],
            index=len(run_history) - 1  # Default to latest
        )
    else:
        st.sidebar.info("No historical runs yet")
        selected_run_idx = None
    
    # Display metrics
    col1, col2, col3, col4 = st.columns(4)
    
    with col1:
        st.metric("🛒 Newegg Products", len(data.get('newegg_products', [])))
    with col2:
        st.metric("📱 Swappa Products", len(data.get('swappa_products', [])))
    with col3:
        st.metric("🛍️ eBay Sold Items", len(data.get('ebay_products', [])))
    with col4:
        opportunities = data.get('arbitrage_opportunities', [])
        st.metric("💰 Arbitrage Opportunities", len(opportunities))
    
    # Last updated
    last_updated = data.get('last_updated', 'Unknown')
    st.caption(f"Last updated: {last_updated}")
    
    # Tabs
    tab1, tab2, tab3, tab4, tab5 = st.tabs([
        "🎯 Arbitrage Opportunities", 
        "📱 All Products vs eBay",
        "📜 Run History", 
        "📊 Compare Runs",
        "📈 Analytics"
    ])
    
    # Tab 1: Arbitrage Opportunities
    with tab1:
        st.header("💰 Arbitrage Opportunities")
        st.markdown("*Swappa products compared to eBay SOLD listing averages*")
        
        opportunities = data.get('arbitrage_opportunities', [])
        
        if opportunities:
            # Sort by profit margin
            opportunities_sorted = sorted(
                opportunities, 
                key=lambda x: x.get('margin_percent', 0) or 0, 
                reverse=True
            )
            
            for opp in opportunities_sorted:
                profit_margin = opp.get('margin_percent', 0) or 0
                profit_class = "profit-positive" if profit_margin > 0 else "profit-negative"
                
                with st.expander(
                    f"📱 {opp.get('buy_product_name', 'Unknown')} - "
                    f"{'🟢' if profit_margin > 0 else '🔴'} {profit_margin:.1f}% margin",
                    expanded=profit_margin > 15
                ):
                    col1, col2, col3 = st.columns(3)
                    
                    with col1:
                        st.markdown("### 💰 Buy on Swappa")
                        st.metric("Price", format_currency(opp.get('buy_price')))
                        swappa_url = opp.get('buy_url', '#')
                        st.markdown(f"[View on Swappa]({swappa_url})")
                    
                    with col2:
                        st.markdown("### 📊 eBay Sold Average")
                        st.metric("Average Sold Price", format_currency(opp.get('ebay_avg_sold_price')))
                        ebay_urls = opp.get('sample_ebay_urls', [])
                        if ebay_urls:
                            st.markdown(f"[View Sample Sale]({ebay_urls[0]})")
                        st.caption(f"Based on {opp.get('ebay_sold_count', 0)} sold listings")
                        st.caption(f"Range: {opp.get('ebay_price_range', 'N/A')}")
                    
                    with col3:
                        st.markdown("### 💵 Potential Profit")
                        potential_profit = opp.get('potential_profit', 0)
                        st.metric("Profit", format_currency(potential_profit))
                        st.markdown(f"<span class='{profit_class}'>Margin: {profit_margin:.1f}%</span>", 
                                   unsafe_allow_html=True)
        else:
            st.info("No arbitrage opportunities found yet. The scraper needs to find similar products on both Swappa and eBay SOLD listings.")
    
    # Tab 2: All Products vs eBay
    with tab2:
        st.header("📱 All Products vs eBay Averages")
        st.markdown("*Every Swappa & Newegg product compared to average eBay SOLD prices*")
        
        # Get products with comparison from run history
        if run_history and selected_run_idx is not None:
            selected_run = run_history[selected_run_idx]
            swappa_products = selected_run.get('swappa_products', [])
            newegg_products = selected_run.get('newegg_products', [])
            # Combine both sources
            products_with_comparison = swappa_products + newegg_products
        else:
            products_with_comparison = []
        
        if products_with_comparison:
            # Summary stats
            col1, col2, col3, col4 = st.columns(4)
            
            profitable = [p for p in products_with_comparison if (p.get('margin_percent') or 0) > 0]
            avg_margin = sum(p.get('margin_percent', 0) or 0 for p in products_with_comparison) / len(products_with_comparison) if products_with_comparison else 0
            
            with col1:
                st.metric("Total Products", len(products_with_comparison))
            with col2:
                st.metric("Profitable", len(profitable))
            with col3:
                st.metric("Avg Margin", f"{avg_margin:.1f}%")
            with col4:
                swappa_count = len([p for p in products_with_comparison if 'swappa' in p.get('url', '').lower()])
                newegg_count = len([p for p in products_with_comparison if 'newegg' in p.get('url', '').lower()])
                st.metric("Sources", f"S:{swappa_count} N:{newegg_count}")
            
            # Filter options
            filter_col1, filter_col2, filter_col3 = st.columns(3)
            with filter_col1:
                show_only_profitable = st.checkbox("Show only profitable", value=False)
            with filter_col2:
                min_margin = st.slider("Minimum margin %", -100, 100, -100)
            with filter_col3:
                source_filter = st.selectbox("Source", ["All", "Swappa", "Newegg"])
            
            # Sort by margin
            sorted_products = sorted(
                products_with_comparison,
                key=lambda x: x.get('margin_percent', -999) or -999,
                reverse=True
            )
            
            # Filter
            if show_only_profitable:
                sorted_products = [p for p in sorted_products if (p.get('margin_percent') or 0) > 0]
            sorted_products = [p for p in sorted_products if (p.get('margin_percent') or -999) >= min_margin]
            
            # Source filter
            if source_filter == "Swappa":
                sorted_products = [p for p in sorted_products if 'swappa' in p.get('url', '').lower()]
            elif source_filter == "Newegg":
                sorted_products = [p for p in sorted_products if 'newegg' in p.get('url', '').lower()]
            
            # Display products
            for product in sorted_products:
                margin = product.get('margin_percent')
                ebay_avg = product.get('ebay_avg_sold')
                source = "Swappa" if 'swappa' in product.get('url', '').lower() else "Newegg"
                source_icon = "📱" if source == "Swappa" else "🛒"
                
                # Determine icon
                if margin is not None:
                    if margin > 20:
                        icon = "🟢"
                    elif margin > 0:
                        icon = "🟡"
                    else:
                        icon = "🔴"
                    margin_text = f"{margin:.1f}%"
                else:
                    icon = "⚪"
                    margin_text = "N/A"
                
                with st.expander(f"{icon} {source_icon} [{source}] {product.get('name', 'Unknown')[:50]} - ${product.get('price_numeric', 0):.0f} → Margin: {margin_text}"):
                    col1, col2, col3 = st.columns(3)
                    
                    with col1:
                        st.markdown(f"**{source_icon} {source} Listing**")
                        st.write(f"Price: ${product.get('price_numeric', 0):.2f}")
                        st.markdown(f"[View on {source}]({product.get('url', '#')})")
                    
                    with col2:
                        st.markdown("**eBay Sold Average**")
                        if ebay_avg:
                            st.write(f"Avg: ${ebay_avg:.2f}")
                            st.write(f"Based on {product.get('ebay_sold_count', 0)} sales")
                            st.write(f"Range: {product.get('ebay_price_range', 'N/A')}")
                        else:
                            st.write("No eBay data")
                    
                    with col3:
                        st.markdown("**Profit Analysis**")
                        if margin is not None:
                            potential = product.get('potential_profit', 0)
                            color = "green" if potential > 0 else "red"
                            st.markdown(f"Potential: <span style='color:{color}'>${potential:.2f}</span>", unsafe_allow_html=True)
                            st.markdown(f"Margin: <span style='color:{color}'>{margin:.1f}%</span>", unsafe_allow_html=True)
                        else:
                            st.write("Unable to calculate")
        else:
            st.info("No product comparison data available. Run the scraper to generate comparisons.")
            # Show current swappa products as fallback
            swappa = data.get('swappa_products', [])
            if swappa:
                st.markdown("### Current Swappa Products (without comparison)")
                df = pd.DataFrame(swappa)
                if not df.empty:
                    st.dataframe(df[['name', 'price', 'url']] if 'url' in df.columns else df, use_container_width=True)
    
    # Tab 3: Run History
    with tab3:
        st.header("📜 Run History")
        
        if run_history:
            for i, run in enumerate(reversed(run_history)):
                run_idx = len(run_history) - 1 - i
                
                with st.expander(f"Run #{run.get('run_id', run_idx + 1)} - {run.get('timestamp', 'Unknown')[:19]}", 
                               expanded=(i == 0)):
                    col1, col2, col3 = st.columns(3)
                    
                    with col1:
                        st.metric("Swappa", run.get('total_swappa', 0))
                    with col2:
                        st.metric("eBay Sold", run.get('total_ebay_sold', 0))
                    with col3:
                        opps = run.get('arbitrage_opportunities', [])
                        st.metric("Opportunities", len(opps))
                    
                    # Show best opportunity from this run
                    best = run.get('best_opportunity')
                    if best:
                        st.markdown("**Best Opportunity:**")
                        st.write(f"🏆 {best.get('buy_product_name', 'Unknown')}: {best.get('margin_percent', 0):.1f}% margin (${best.get('potential_profit', 0):.2f} profit)")
                    
                    # Products with comparison
                    products = run.get('swappa_products', [])
                    if products:
                        profitable = len([p for p in products if (p.get('margin_percent') or 0) > 0])
                        st.markdown(f"**Products analyzed:** {len(products)} ({profitable} profitable)")
        else:
            st.info("No run history available yet. The scraper will record history as it runs.")
    
    # Tab 4: Compare Runs
    with tab4:
        st.header("📊 Compare Runs")
        
        if len(run_history) >= 2:
            col1, col2 = st.columns(2)
            
            with col1:
                run1_idx = st.selectbox(
                    "Select Run 1:",
                    range(len(run_history)),
                    format_func=lambda x: f"Run {run_history[x].get('run_id', x+1)}: {run_history[x].get('timestamp', 'Unknown')[:16]}",
                    key="run1"
                )
            
            with col2:
                run2_idx = st.selectbox(
                    "Select Run 2:",
                    range(len(run_history)),
                    format_func=lambda x: f"Run {run_history[x].get('run_id', x+1)}: {run_history[x].get('timestamp', 'Unknown')[:16]}",
                    index=min(1, len(run_history)-1),
                    key="run2"
                )
            
            if run1_idx != run2_idx:
                run1 = run_history[run1_idx]
                run2 = run_history[run2_idx]
                
                st.markdown("---")
                
                # Compare metrics
                col1, col2, col3 = st.columns(3)
                
                with col1:
                    st.markdown("### Swappa Products")
                    delta = run2.get('total_swappa', 0) - run1.get('total_swappa', 0)
                    st.metric(f"Run {run1.get('run_id', run1_idx+1)}", run1.get('total_swappa', 0))
                    st.metric(f"Run {run2.get('run_id', run2_idx+1)}", run2.get('total_swappa', 0), delta=delta)
                
                with col2:
                    st.markdown("### eBay Sold")
                    delta = run2.get('total_ebay_sold', 0) - run1.get('total_ebay_sold', 0)
                    st.metric(f"Run {run1.get('run_id', run1_idx+1)}", run1.get('total_ebay_sold', 0))
                    st.metric(f"Run {run2.get('run_id', run2_idx+1)}", run2.get('total_ebay_sold', 0), delta=delta)
                
                with col3:
                    st.markdown("### Opportunities")
                    opps1 = len(run1.get('arbitrage_opportunities', []))
                    opps2 = len(run2.get('arbitrage_opportunities', []))
                    delta = opps2 - opps1
                    st.metric(f"Run {run1.get('run_id', run1_idx+1)}", opps1)
                    st.metric(f"Run {run2.get('run_id', run2_idx+1)}", opps2, delta=delta)
                
                # Compare opportunities
                st.markdown("### Arbitrage Comparison")
                
                run1_opps = {o.get('buy_product_name'): o for o in run1.get('arbitrage_opportunities', [])}
                run2_opps = {o.get('buy_product_name'): o for o in run2.get('arbitrage_opportunities', [])}
                
                all_products = set(run1_opps.keys()) | set(run2_opps.keys())
                
                if all_products:
                    comparison_data = []
                    for product in all_products:
                        opp1 = run1_opps.get(product, {})
                        opp2 = run2_opps.get(product, {})
                        comparison_data.append({
                            'Product': product,
                            f'Run {run1.get("run_id", run1_idx+1)} Margin': f"{opp1.get('margin_percent', 'N/A'):.1f}%" if opp1.get('margin_percent') else "N/A",
                            f'Run {run2.get("run_id", run2_idx+1)} Margin': f"{opp2.get('margin_percent', 'N/A'):.1f}%" if opp2.get('margin_percent') else "N/A",
                        })
                    
                    df = pd.DataFrame(comparison_data)
                    st.dataframe(df, use_container_width=True)
            else:
                st.warning("Please select two different runs to compare.")
        else:
            st.info("Need at least 2 runs to compare. Keep the scraper running to build history.")
    
    # Tab 5: Analytics
    with tab5:
        st.header("📈 Analytics")
        
        if len(run_history) >= 2:
            # Prepare data for charts
            timestamps = [run.get('timestamp', '')[:16] for run in run_history]
            swappa_counts = [run.get('total_swappa', 0) for run in run_history]
            ebay_counts = [run.get('total_ebay_sold', 0) for run in run_history]
            
            # Product counts over time
            st.subheader("Products Over Time")
            chart_data = pd.DataFrame({
                'Run': [run.get('run_id', i+1) for i, run in enumerate(run_history)],
                'Swappa': swappa_counts,
                'eBay Sold': ebay_counts
            })
            st.line_chart(chart_data.set_index('Run'))
            
            # Average margins over time
            st.subheader("Average Profit Margins Over Time")
            avg_margins = []
            for run in run_history:
                products = run.get('swappa_products', [])
                if products:
                    margins = [p.get('margin_percent', 0) or 0 for p in products if p.get('margin_percent') is not None]
                    avg_margins.append(sum(margins) / len(margins) if margins else 0)
                else:
                    avg_margins.append(0)
            
            margin_data = pd.DataFrame({
                'Run': [run.get('run_id', i+1) for i, run in enumerate(run_history)],
                'Avg Margin %': avg_margins
            })
            st.line_chart(margin_data.set_index('Run'))
            
            # Opportunity count over time
            st.subheader("Arbitrage Opportunities Over Time")
            opp_counts = [len(run.get('arbitrage_opportunities', [])) for run in run_history]
            opp_data = pd.DataFrame({
                'Run': [run.get('run_id', i+1) for i, run in enumerate(run_history)],
                'Opportunities': opp_counts
            })
            st.bar_chart(opp_data.set_index('Run'))
        else:
            st.info("Need more runs to show analytics. Keep the scraper running!")
    
    # Auto-refresh option
    st.sidebar.markdown("---")
    auto_refresh = st.sidebar.checkbox("🔄 Auto-refresh (30s)", value=False)
    
    if auto_refresh:
        time.sleep(30)
        st.rerun()
    
    if st.sidebar.button("🔄 Manual Refresh"):
        st.rerun()

if __name__ == "__main__":
    main()
