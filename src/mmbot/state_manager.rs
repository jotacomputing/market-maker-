// data manager + inventory manager 
use std::time::Instant;
use market_maker_rs::{market_state::volatility::VolatilityEstimator, prelude::{InventoryPosition, MarketState, PnL}};
use rust_decimal::Decimal;
use rustc_hash::FxHashMap;
use crate::{mmbot::rolling_price::RollingPrice, shm::{feed_queue_mm::MarketMakerFeedQueue, fill_queue_mm::MarketMakerFillQueue, response_queue_mm::MessageFromApiQueue}};

pub struct SymbolState{
    pub symbol: u32,
    pub ipo_price: Decimal,
    
    // Market data
    pub best_bid: Decimal,
    pub best_ask: Decimal,
    pub best_bid_qty: u32,
    pub best_ask_qty: u32,

    // market state , volatility + mid price 
    pub market_state : MarketState,
     
    // historical data
    pub rolling_prices: RollingPrice,
    
    // Inventory 
    pub inventory : InventoryPosition,
    pub pnl       : PnL,
    
    // Bootstrap tracking
    pub total_volume: u64,
    pub is_bootstrapped: bool,
    
    // Timing
    pub last_feed_time: Instant,
    pub last_volatility_calc: Instant,
    pub last_sample_time: Instant,
}

pub struct StateManager{
    pub symbol_states        : FxHashMap<u32 , SymbolState>, // hash map for now , will move to array 
    pub fill_queue           : MarketMakerFillQueue,
    pub feed_queue           : MarketMakerFeedQueue,
    pub response_queue       : MessageFromApiQueue ,
    pub volitality_estimator : VolatilityEstimator ,
}

