use std::time::Instant;

use rust_decimal::Decimal;

#[derive(Debug, Clone, PartialEq)]
pub enum QuotingMode {
    Bootstrap {
        spread_pct: Decimal,     // 2% wide spread
        levels: usize,           // 5 levels
    },
    Normal {
        levels: usize,           // 10 levels
        size_decay: f64,         // 0.85 exponential decay
    },
    Stressed {
        spread_mult: Decimal,    // 2x wider spreads
        levels: usize,           // 5 levels (reduce liquidity)
    },
    InventoryCapped {
        side: Side,     // Only bid or only ask
        levels: usize,           // 10 levels
    },
}

pub struct QuoteLadder {
    pub bids: Vec<QuoteLevel>,
    pub asks: Vec<QuoteLevel>,
}

pub struct QuoteLevel {
    pub price: Decimal,
    pub size: u64,
    pub level_number: usize,  // 0 = closest to mid, 9 = farthest
}


#[derive(Debug)]
pub struct SymbolOrders {
    pub symbol: u32,
    pub pending_orders: Vec<PendingOrder>,
    pub next_client_id: u64,
    pub last_quote_time: Instant,
}


#[derive(Debug, Clone, PartialEq)]
pub enum Side {
    BID = 0 ,
    ASK = 1 
}

#[derive(Debug, Clone, PartialEq)]
pub enum OrderState {
    PendingNew,
    Active,
    PendingCancel,
    PartiallyFilled,
}


#[derive(Debug, Clone)]
pub struct PendingOrder{
    pub client_id: u64,
    pub exchange_order_id: Option<u64>,
    pub side: Side,
    pub price: Decimal,
    pub original_size: u32,
    pub remaining_size: u32,
    pub state: OrderState,
    pub level_number: usize,  // Which level in the ladder (0-9)
}

