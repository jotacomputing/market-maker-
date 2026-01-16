use market_maker_rs::{Decimal, dec, market_state::volatility::VolatilityEstimator, prelude::{InventoryPosition, MarketState}, strategy::avellaneda_stoikov::calculate_optimal_quotes};
use std::{time::{Duration, Instant}};
use crate::{mmbot::rolling_price::RollingPrice, shm::{feed_queue_mm::MarketMakerFeedQueue, fill_queue_mm::MarketMakerFillQueue, send_order_queue_mm::{MarketMakerOrderQueue, Order}}};
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

#[derive(Debug , Clone)]
pub struct SymbolState{
    pub symbol : u32,
    pub inventory: InventoryPosition,          // current inventory 
    pub risk_aversion: Decimal,       
    pub time_to_terminal : u64,        
    pub liquidity_k: Decimal,            // order intensity 
    pub market_state : MarketState,
    pub best_bid_qty: u32,
    pub best_ask_qty: u32,
    pub prices : RollingPrice ,
    pub last_quoted : Instant ,
    pub last_volitlitly_calc : Instant ,
    pub last_sampled : Instant
}

// each symbol state shud have a defualt inventory for init 
impl SymbolState{
    pub fn new(ipo_price : Decimal , symbol:u32)->Self{
        Self { 
            symbol,
            inventory: InventoryPosition::new(),
            risk_aversion: dec!(1), 
            time_to_terminal: 0, 
            liquidity_k: dec!(0), 
            market_state: MarketState::new(dec!(0), dec!(0), 0) ,
            prices : RollingPrice::new(50, ipo_price),
            last_quoted : Instant::now() ,
            last_volitlitly_calc : Instant::now() , 
            last_sampled : Instant::now() , 
            best_ask_qty : 10 ,
            best_bid_qty : 10
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
    
}

pub struct MarketMaker{
    pub fill_queue  : MarketMakerFillQueue,
    pub feed_queue  : MarketMakerFeedQueue,
    pub order_queue : MarketMakerOrderQueue,
    pub symbol_detials : [Option<SymbolState> ; MAX_SYMBOLS],
    pub volitality_estimator : VolatilityEstimator ,
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
        Self { 
            order_queue : order_queue.unwrap(),
            fill_queue : fill_queue.unwrap(),
            feed_queue : feed_queue.unwrap(),
            symbol_detials: std::array::from_fn(|_| None), 
            volitality_estimator: VolatilityEstimator::new() , 
        }
    }

    pub fn add_symbol(&mut self , symbol : u32 , ipo_price : Decimal){
       self.symbol_detials[symbol as usize] = Some(SymbolState::new(ipo_price , symbol));
    }

    pub fn run_market_maker(&mut self){
        loop {
            while let Ok(Some(fill)) = self.fill_queue.dequeue(){
                let symbol = fill.symbol;
                if let Some(symbol_state) = self.symbol_detials[symbol as usize].as_mut(){
                    symbol_state.inventory.last_update = fill.timestamp;
                    match fill.side_of_mm_order {
                        0 =>{
                            // it was a buy order so we bought shares , add to the inventory 
                            symbol_state.inventory.quantity = symbol_state.inventory.quantity.saturating_add(Decimal::from(fill.fill_quantity));
                        }
                        1 =>{
                            // it was a sell order so we sold inventory 
                            symbol_state.inventory.quantity = symbol_state.inventory.quantity.saturating_sub(Decimal::from(fill.fill_quantity));
                        }
                        _=>{

                        }
                    }
                }
                
            }

            while let Ok(Some(market_feed)) = self.feed_queue.dequeue(){
                let symbol = market_feed.symbol;
                if let Some( symbol_state) = self.symbol_detials[symbol as usize].as_mut(){
                    let mid_price = (market_feed.best_bid + market_feed.best_ask)/2;
                    // set in the market state 
                    symbol_state.market_state.mid_price = Decimal::from(mid_price);   
                    symbol_state.best_ask_qty = market_feed.best_ask_qty; 
                    symbol_state.best_bid_qty = market_feed.best_bid_qty;
                }
                // find the mid price 
            }

            for   symbol_state in self.symbol_detials.iter_mut(){
                match symbol_state{
                    Some(state)=>{

                        if state.last_sampled.elapsed() >= SAMPLE_GAP{
                            state.prices.push(state.market_state.mid_price);
                            state.last_sampled = Instant::now();
                        }
                        
                        if state.last_quoted.elapsed() >= QUOTING_GAP{
                            if let Ok(best_quote_prices) = calculate_optimal_quotes(
                                state.market_state.mid_price,
                                 state.inventory.quantity, 
                                 state.risk_aversion, 
                                 state.market_state.volatility, 
                                 state.time_to_terminal,
                                 state.liquidity_k 
                            ){
                                let bid_price = best_quote_prices.0;
                                let ask_price = best_quote_prices.1;

                                let (bid_size , ask_size) = state.compute_quote_sizes();
        
                                // need to find sizes , form quotes , cancel all pending orders 
                                // SEND CANCEL ORDER Commands for pending orders 
                                let _ = self.order_queue.enqueue(Order{
                                    price : ask_price.to_u64().unwrap() ,
                                    timestamp : 0 , 
                                    shares_qty : ask_size as u32 ,
                                    symbol : state.symbol ,
                                    side : 1 ,
                                    status : 0 
                                });

                                let _ = self.order_queue.enqueue(Order{
                                    price : bid_price.to_u64().unwrap() ,
                                    timestamp : 0 , 
                                    shares_qty : bid_size as u32 ,
                                    symbol : state.symbol ,
                                    side : 0 ,
                                    status : 0 
                                });

                            }
                        }

                        if state.last_volitlitly_calc.elapsed() >= VOLITILTY_CALC_GAP{
                            // we need to calculate volitility again 
                           if let Ok(volatility) = self.volitality_estimator.calculate_simple(state.prices.as_slice_for_volatility()){
                                state.market_state.volatility = volatility;
                                state.last_volitlitly_calc = Instant::now();
                           }
                        }
                    }
                    None => {
                       
                    }
                }
            }
            // isssue -> market state has a time stamp for  both vilitility and mid price  clocks need to be configured 

            

            
        }
    }
}



