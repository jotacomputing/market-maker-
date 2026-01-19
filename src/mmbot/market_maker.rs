use market_maker_rs::{Decimal, dec, market_state::volatility::VolatilityEstimator, prelude::{InventoryPosition, MarketState, PnL}, strategy::avellaneda_stoikov::calculate_optimal_quotes};
use rustc_hash::FxHashMap;
use std::{collections::VecDeque, time::{Duration, Instant}};
use crate::{mmbot::{types::SymbolOrders, rolling_price::RollingPrice, types::QuotingMode}, shm::{feed_queue_mm::MarketMakerFeedQueue, fill_queue_mm::MarketMakerFillQueue, order_queue_mm::{MarketMakerOrderQueue, MmOrder}, response_queue_mm::MessageFromApiQueue}};
use rust_decimal::prelude::ToPrimitive;


//use std::time::Instant;
const MAX_SYMBOLS : usize = 100;
const SAMPLE_GAP : Duration = Duration::from_millis(50);
const VOLITILTY_CALC_GAP  : Duration = Duration::from_millis(100);
const QUOTING_GAP : Duration = Duration::from_millis(200);
const TARGET_INVENTORY : Decimal = dec!(0); 
const MAX_SIZE : Decimal = dec!(50) ; 
const INVENTORY_CAP :Decimal = dec!(1000);
const MAX_BOOK_MULT : Decimal = dec!(2);


#[derive(Debug)]
pub struct SymbolState{
    // inputs 
    pub symbol : u32,
    pub ipo_price: Decimal,

    // Market data
    pub best_bid: Decimal,
    pub best_ask: Decimal,
    pub best_bid_qty: u32,
    pub best_ask_qty: u32,

    // market state for storing volatility and mid price for each symbol 
    pub market_state : MarketState,

    // rolling price history for volatility calculation , each symbol 
    pub rolling_prices: RollingPrice, 

    // Inventory for each symbol long , short how much 
    pub inventory : InventoryPosition,
    pub pnl       : PnL,

    // Timing
   // pub last_quoted: Instant, // last quoting for this symbol 
    pub last_volatility_calc: Instant, // last volatility calculation
    pub last_sample_time: Instant, // when did we add the mid price to the rolling prices array last 
  
    // AS model constants 
    pub risk_aversion: Decimal,       
    pub time_to_terminal : u64,        
    pub liquidity_k: Decimal,            // order intensity 

    // keeping model constants per symbol , an auto adjusting formula needs to be developed to modify these 
    // according to market conditions

}

// each symbol state shud have a defualt inventory for init 
impl SymbolState{
    pub fn new(ipo_price : Decimal , symbol:u32)->Self{
        Self { 
            symbol ,
            ipo_price , 
            best_ask : ipo_price,
            best_bid : ipo_price ,
            best_ask_qty : 0 , 
            best_bid_qty : 0 ,
            market_state : MarketState { mid_price: ipo_price, volatility: dec!(0), timestamp: 0 } ,
            rolling_prices : RollingPrice { deque: VecDeque::new(), capacity: 0 } ,
            inventory : InventoryPosition::new() ,
            pnl : PnL::new(),
            last_sample_time : Instant::now() ,
            last_volatility_calc : Instant::now() ,
            risk_aversion :dec!(0) , // decide ,,
            time_to_terminal : 0 , // decide 
            liquidity_k : dec!(0) , // decide , 
        }
        // find the sollutiton for the best bid and the best ask value at cold start 
    }
    pub fn compute_quote_sizes(
        &self,
    ) -> (u64, u64) {
       
        if INVENTORY_CAP <= dec!(0) || MAX_SIZE == dec!(0) {
            return (0, 0);
        }

        let inv = self.inventory.quantity;
        let dev = inv - TARGET_INVENTORY; 
        let abs_dev = dev.abs();

        
        let vol = self.market_state.volatility.max(dec!(0));
        let vol_factor = dec!(1) / (dec!(1) + vol); // in (0,1]

        let inv_ratio = (abs_dev / INVENTORY_CAP).min(dec!(1));

        
        let inv_ratio_f = inv_ratio.to_f64().unwrap_or(1.0);
        let vol_factor_f = vol_factor.to_f64().unwrap_or(0.1);

       
        let mut base = (MAX_SIZE.to_f64().unwrap_or(50.0) * vol_factor_f).round() as i64;
        base = base.max(1);

        
        let risky_mult = (1.0 - inv_ratio_f).clamp(0.0, 1.0);
        let safe_mult  = (1.0 + inv_ratio_f).clamp(1.0, 2.0);

        let (mut bid_size, mut ask_size) = if dev >= dec!(0) {
            // too long => don't buy more, sell more
            ((base as f64 * risky_mult).round() as i64,
             (base as f64 * safe_mult).round() as i64)
        } else {
            // too short => buy more, don't sell more
            ((base as f64 * safe_mult).round() as i64,
             (base as f64 * risky_mult).round() as i64)
        };

        
        if abs_dev >= INVENTORY_CAP {
          
            if dev > dec!(0) {
              
                bid_size = 0;
            } else if dev < dec!(0) {
              
                ask_size = 0;
            } else {
               
            }
        }

      
        let max_size_i64 = MAX_SIZE.to_i64().unwrap_or(50);
        bid_size = bid_size.clamp(0, max_size_i64);
        ask_size = ask_size.clamp(0, max_size_i64);

        let best_bid_qty = self.best_bid_qty as u64;
        let best_ask_qty = self.best_ask_qty as u64;
        let max_book_mult_u64 = MAX_BOOK_MULT.to_u64().unwrap_or(2);

        if best_bid_qty > 0 {
            let cap = best_bid_qty.saturating_mul(max_book_mult_u64).max(1);
            bid_size = bid_size.min(cap as i64);
        }
        if best_ask_qty > 0 {
            let cap = best_ask_qty.saturating_mul(max_book_mult_u64).max(1);
            ask_size = ask_size.min(cap as i64);
        }

        (bid_size as u64, ask_size as u64)
    }

    //pub fn generate_client_id(&mut self) -> u64 {
    //    let id = self.next_client_id;
    //    self.next_client_id += 1;
    //    id
    //}
    
}



pub struct MarketMaker{

    // Data manager for all symbols 
    pub symbol_states: FxHashMap<u32, SymbolState>,
    pub message_queue : MessageFromApiQueue,
    pub fill_queue    : MarketMakerFillQueue,
    pub feed_queue    : MarketMakerFeedQueue,
    pub volitality_estimator : VolatilityEstimator ,



    //ORDER MANAGER 
    pub order_queue   : MarketMakerOrderQueue,
    pub symbol_orders: FxHashMap<u32, SymbolOrders>,


    // Bootstrap tracking
    pub total_volume: u64,
    pub is_bootstrapped: bool,

   

    


    // quoting engine 
    pub current_mode : QuotingMode
   
}

impl MarketMaker{
    pub fn new()->Self{
        let fill_queue = MarketMakerFillQueue::open("/tmp/MarketMakerFills");
        if fill_queue.is_err(){
            eprintln!("failed to open the fill queue");
        }
        let feed_queue = MarketMakerFeedQueue::open("/tmp/MarketMakerFeed");
        if feed_queue.is_err(){
            eprint!("failed to open feed queue");
        }
        let order_queue = MarketMakerOrderQueue::open("/tmp/MarketMakerOrders");
        if order_queue.is_err(){
            eprint!("failed to open order queue");
        }
        let message_from_api_queueu = MessageFromApiQueue::open("/tmp/MessageFromApiToMM");
        if message_from_api_queueu.is_err(){
            eprint!("fai;ed to open message queue");
        }
        Self { 
            symbol_orders : FxHashMap::with_capacity_and_hasher(1_000_000, Default::default()),
            order_queue : order_queue.unwrap(),
            fill_queue : fill_queue.unwrap(),
            feed_queue : feed_queue.unwrap(),
            message_queue : message_from_api_queueu.unwrap(),
            volitality_estimator: VolatilityEstimator::new() , 
            is_bootstrapped : false , 
            total_volume : 0 , 
            symbol_states : FxHashMap::with_capacity_and_hasher(1_000_000, Default::default()),
            current_mode : QuotingMode::Bootstrap { spread_pct: dec!(0), levels: 0 }
          
        }
    }

    
}



