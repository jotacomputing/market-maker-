use std::time::Instant;

use rust_decimal::Decimal;
use rustc_hash::FxHashMap;
use crate::shm::order_queue_mm::MarketMakerOrderQueue;



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
pub struct OrderManager {
    pub order_queue: MarketMakerOrderQueue,
    pub symbols: FxHashMap<u32, SymbolOrders>,
}
