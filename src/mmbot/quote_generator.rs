use rust_decimal::Decimal;
use crate::mmbot::types::Side;

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

pub struct QuotingEngine {
    pub current_mode: QuotingMode,
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



