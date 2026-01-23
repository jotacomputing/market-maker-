use std::time::Instant;

use rust_decimal::Decimal;

// level are basically price levels  how deep to quote 

#[derive(Debug, Clone, PartialEq , Copy)]
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
        side: InventorySatus,     // Only bid or only ask
        levels: usize,           // 10 levels
    },

    Emergency

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


impl SymbolOrders{
    pub fn new(symbol : u32)->Self{
        Self { 
            symbol, 
            pending_orders: Vec::new(), 
            next_client_id: 1, 
            last_quote_time: Instant::now() 
        }
    }
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
    pub created_at : Instant
}



#[derive(Debug)]
pub enum MmError{
    SymbolNotFound ,
    ClienIdNotFound
}


#[derive(Debug, Clone, PartialEq , Copy)]
pub enum InventorySatus{
    Long ,
    Short
}

pub enum ApiMessageType{
    AddSymbolMessage = 0 , 
    OrderAcceptedAck = 1 ,
    OrderCancelledAck = 2 ,
}





pub struct DepthUpdate{
    pub old_best_bid : Decimal ,
    pub old_best_ask : Decimal ,
    pub new_best_bid : Decimal ,
    pub new_best_ask : Decimal, 
}



#[derive(Debug , Clone, Copy)]
pub struct CancelData{
    pub symbol : u32 , 
    pub client_id : u64 , 
    pub order_id  : Option<u64> 

}