use market_maker_rs::{Decimal, dec, market_state::volatility::VolatilityEstimator, prelude::{InventoryPosition, MarketState}, strategy::avellaneda_stoikov::calculate_optimal_quotes};
use std::{mem::MaybeUninit, time::{Duration, Instant}};
use crate::{mmbot::rolling_price::RollingPrice, shm::{feed_queue_mm::MarketMakerFeedQueue, fill_queue_mm::MarketMakerFillQueue}};
//use std::time::Instant;
const MAX_SYMBOLS : usize = 100;
const QUOTING_GAP : Duration = Duration::from_millis(200);
const SAMPLE_GAP : Duration = Duration::from_millis(100);
const VOLITILTY_CALC_GAP  : Duration = Duration::from_millis(500);


#[derive(Debug , Clone)]
pub struct SymbolState{
    pub inventory: InventoryPosition,          // current inventory 
    pub risk_aversion: Decimal,       
    pub time_to_terminal : u64,        
    pub liquidity_k: Decimal,            // order intensity 
    pub market_state : MarketState,
    pub prices : RollingPrice ,
    pub last_quoted : Instant ,
    pub last_volitlitly_calc : Instant ,
    pub last_sampled : Instant
}

// each symbol state shud have a defualt inventory for init 
impl SymbolState{
    pub fn new(ipo_price : Decimal)->Self{
        Self { 
            inventory: InventoryPosition::new(),
            risk_aversion: dec!(1), 
            time_to_terminal: 0, 
            liquidity_k: dec!(0), 
            market_state: MarketState::new(dec!(0), dec!(0), 0) ,
            prices : RollingPrice::new(50, ipo_price),
            last_quoted : Instant::now() ,
            last_volitlitly_calc : Instant::now() , 
            last_sampled : Instant::now()
        }
    }
}

pub struct MarketMaker{
    pub fill_queue : MarketMakerFillQueue,
    pub feed_queue : MarketMakerFeedQueue,
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
        Self { 
            fill_queue : fill_queue.unwrap(),
            feed_queue : feed_queue.unwrap(),
            symbol_detials: std::array::from_fn(|_| None), 
            volitality_estimator: VolatilityEstimator::new() , 
        }
    }

    pub fn add_symbol(&mut self , symbol : u32 , ipo_price : Decimal){
       self.symbol_detials[symbol as usize] = Some(SymbolState::new(ipo_price));
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
                }
                // find the mid price 
            }

            for symbol_state in self.symbol_detials.iter_mut(){
                match symbol_state{
                    Some(state)=>{

                        if state.last_sampled.elapsed() >= SAMPLE_GAP{
                            state.prices.push(state.market_state.mid_price);
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
        
                                // need to find sizes , form quotes , cancel all pending orders 
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
                        println!("No symbol has been inititliased ")
                    }
                }
            }
            // isssue -> market state has a time stamp for  both vilitility and mid price  clocks need to be configured 

            

            
        }
    }
}



