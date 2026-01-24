use market_maker_rs::{Decimal, dec, market_state::volatility::VolatilityEstimator, prelude::{InventoryPosition, MMError, MarketState, PenaltyFunction, PnL}, strategy::{avellaneda_stoikov::calculate_optimal_quotes, interface::AvellanedaStoikov, quote}};
use rustc_hash::FxHashMap;
use serde::de;
use std::{collections::VecDeque, os::macos::raw::stat, time::{Duration, Instant}};
use crate::{mmbot::{rolling_price::RollingPrice, types::{CancelData, DepthUpdate, InventorySatus, MmError, PostData, QuotingMode, SymbolOrders, TargetLadder, TargetQuotes}}, shm::{feed_queue_mm::{MarketMakerFeed, MarketMakerFeedQueue}, fill_queue_mm::{MarketMakerFill, MarketMakerFillQueue}, order_queue_mm::{MarketMakerOrderQueue, MmOrder, QueueError}, response_queue_mm::{MessageFromApi, MessageFromApiQueue}}};
use rust_decimal::prelude::ToPrimitive;
use crate::mmbot::types::{OrderState , ApiMessageType , Side , PendingOrder};


//use std::time::Instant;
const MAX_SYMBOLS : usize = 100;
const SAMPLE_GAP : Duration = Duration::from_millis(50);
const VOLITILTY_CALC_GAP  : Duration = Duration::from_millis(100);
const QUOTING_GAP : Duration = Duration::from_millis(200);
const CYCLE_GAP : Duration = Duration::from_millis(250);
const TARGET_INVENTORY : Decimal = dec!(0); 
const MAX_SIZE_FOR_ORDER : Decimal = dec!(100) ; 
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

    // prev market data 
    pub prev_best_bid: Decimal,
    pub prev_best_ask: Decimal,
    pub prev_best_bid_qty: u32,
    pub prev_best_ask_qty: u32,
    pub prev_mid_price: Decimal,

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
    pub last_management_cycle_time : Instant,
  
    // AS model constants 
    pub risk_aversion: Decimal,       
    pub time_to_terminal : u64,        
    pub liquidity_k: Decimal,            // order intensity 

    // keeping model constants per symbol , an auto adjusting formula needs to be developed to modify these 
    // according to market conditions

    // add certain paramteres to seeee the boot strapppingggggg 

    // Bootstrap tracking
    pub total_trades: u64,
    pub total_volume: u64,
    pub is_bootstrapped: bool,




    pub current_mode : QuotingMode,
    pub prev_mode: QuotingMode,
}
// each symbol state shud have a defualt inventory for init (ik)
impl SymbolState{
    pub fn new(ipo_price : Decimal , symbol:u32)->Self{
        Self { 
            symbol ,
            ipo_price , 
            best_ask : ipo_price,
            best_bid : ipo_price ,
            best_ask_qty : 0 , 
            best_bid_qty : 0 ,
            prev_best_ask : dec!(0),
            prev_best_bid : dec!(0) ,
            prev_best_bid_qty : 0 , 
            prev_best_ask_qty : 0 ,
            prev_mid_price : ipo_price,
            market_state : MarketState { mid_price: ipo_price, volatility: dec!(0), timestamp: 0 } ,
            rolling_prices : RollingPrice { deque: VecDeque::with_capacity(100), capacity: 100 } ,
            inventory : InventoryPosition::new() ,
            pnl : PnL::new(),
            last_sample_time : Instant::now() ,
            last_volatility_calc : Instant::now() ,
            last_management_cycle_time : Instant::now(),
            risk_aversion :dec!(0) , // decide ,,
            time_to_terminal : 0 , // decide 
            liquidity_k : dec!(0) , // decide , 
            total_trades : 0 , 
            total_volume : 0 , 
            is_bootstrapped : false , 
            current_mode : QuotingMode::Bootstrap { spread_pct: dec!(0), levels: 0 } ,
            prev_mode : QuotingMode::Bootstrap { spread_pct: dec!(0), levels: 0 }
        }
        // find the sollutiton for the best bid and the best ask value at cold start 
    }

    pub fn compute_quote_sizes(
        &self,
    ) -> (u64, u64) {
       
        if INVENTORY_CAP <= dec!(0) || MAX_SIZE_FOR_ORDER == dec!(0) {
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

       
        let mut base = (MAX_SIZE_FOR_ORDER.to_f64().unwrap_or(50.0) * vol_factor_f).round() as i64;
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

      
        let max_size_i64 = MAX_SIZE_FOR_ORDER.to_i64().unwrap_or(50);
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

    pub fn should_exit_bootstrap(&mut self)->bool{
        let min_trades = self.total_trades >= 20;
        let min_volume = self.total_volume >= 1000;
        
        // tweak params 
        //not enough activity 
        if !min_trades || !min_volume {
            return false;  
        }

        //not enough data for volatility calc
        let enough_samples = self.rolling_prices.len() >= 20;
        if !enough_samples {
            return false;  // Can't calculate volatility yet
        }

        // greater spread pct than 2 -> non volatile market 
        let current_spread = self.best_ask - self.best_bid;
        let spread_pct = if self.market_state.mid_price > dec!(0) {
            (current_spread / self.market_state.mid_price).to_f64().unwrap_or(1.0)
        } else {
            1.0
        };

        let spread_tight = spread_pct < 0.02;  // <2% spread
        
        if !spread_tight {
            return false;  
        }


        // some exist 
        let depth_exists = self.best_bid_qty > 0 && self.best_ask_qty > 0;
        
        if !depth_exists {
            return false;  // No liquidity yet
        }

        true
    }


    pub fn determine_mode(&mut self)->QuotingMode{
        // emergency mode check 
        if self.pnl.total < dec!(-2000.0) || self.pnl.realized < dec!(-1500.0) {
            // Cancel ALL active orders
            //self.cancel_all_orders(symbol);
            self.prev_mode = self.current_mode;
            self.current_mode = QuotingMode::Emergency;
            return QuotingMode::Emergency;
        }


        // inventory cap mode check 
        let inv_abs = self.inventory.quantity.abs();
        let inv_ratio = (inv_abs / INVENTORY_CAP).to_f64().unwrap_or(0.0);
        if inv_abs >= INVENTORY_CAP {  // Hard cap hit
            let side = if self.inventory.quantity > dec!(0) {
                // Cancel all BUY orders (don't buy more)
                //self.cancel_side(symbol, 0);
                InventorySatus::Long
            } else {
                // Cancel all SELL orders (don't sell more)
                //self.cancel_side(symbol, 1);
                InventorySatus::Short
            };
            self.prev_mode = self.current_mode;
            self.current_mode = QuotingMode::InventoryCapped { side, levels: 10 };
            return QuotingMode::InventoryCapped { side, levels: 10 };
        }


        if !self.is_bootstrapped {
            //  if we should exit bootstrap
            if self.should_exit_bootstrap() {
                self.is_bootstrapped = true;
                // Continue to check other modes
            } else {
                // Stay in bootstrap
                self.prev_mode = self.current_mode;
                self.current_mode = QuotingMode::Bootstrap {
                    spread_pct: dec!(0.04),  // 4% wide spread
                    levels: 5,
                };
                return QuotingMode::Bootstrap {
                    spread_pct: dec!(0.04),  // 4% wide spread
                    levels: 5,
                };
            }
        }


        // stressed more in terms of high volatility 
        let vol_pct = (self.market_state.volatility * dec!(100)).to_f64().unwrap_or(0.0);
        let is_high_volatility = self.market_state.volatility > dec!(0.06);  // 6%
        let is_inventory_warning = inv_ratio >= 0.80;  // 80% of cap
        
        if is_high_volatility || is_inventory_warning {
            self.prev_mode = self.current_mode;
            self.current_mode = QuotingMode::Stressed {
                spread_mult: dec!(2.0),  // 2x wider spreads
                levels: 5,               // Fewer levels
            };
            return QuotingMode::Stressed {
                spread_mult: dec!(2.0),  // 2x wider spreads
                levels: 5,               // Fewer levels
            };
        }


        self.prev_mode = self.current_mode;
        self.current_mode = QuotingMode::Normal {
            levels: 10,
            size_decay: 0.85,
        };
        

        QuotingMode::Normal {
            levels: 10,
            size_decay: 0.85,
        }
    }

    pub fn should_cancel_unprofitable_order(& self , order : &PendingOrder , current_mid : Decimal , current_spread:Decimal)->bool{
        let distance_from_mid = (order.price - current_mid).abs();

        if order.side == Side::BID && order.price > current_mid {
            return true; 
        }
        if order.side == Side::ASK && order.price < current_mid {
            return true; 
        }
        
      
        let max_distance = current_spread * dec!(5.0);  
        if distance_from_mid > max_distance {
            return true;  
        }
        
      
        let min_profitable_spread = dec!(0.001); 
        if current_spread < min_profitable_spread {
            return true;  
        }
        
        false  
    }

    pub fn should_cancel_due_to_inventory(
        &self,
        order: &PendingOrder,
        inventory: Decimal,
    ) -> bool {
        let inv_ratio = inventory.abs() / INVENTORY_CAP;

        if inv_ratio >= dec!(0.85) {
            // dont buy more , cancel them 
            if inventory > dec!(0) && order.side == Side::BID {
                return true;  
            }
            // dont sell more 
            if inventory < dec!(0) && order.side == Side::ASK {
                return true; 
            }
        }
        
        false
    }

    
    
}

pub struct SymbolContext{
    pub state :  SymbolState , 
    pub orders : SymbolOrders
}


impl SymbolContext{
    pub fn new(ipo_price : Decimal , symbol : u32)->Self{
        Self{
            state : SymbolState::new(ipo_price, symbol) , 
            orders : SymbolOrders::new(symbol)
        }
    }

    pub fn check_if_time_caused_cancellation( &mut self,
        symbol: u32, cancel_batch: &mut Vec<CancelData>, ){
        const MAX_ORDER_AGE: Duration = Duration::from_secs(60); 

        for order in &mut self.orders.pending_orders {
            if order.state != OrderState::Active {
                continue;
            }

            let age = order.created_at.elapsed();
            if age > MAX_ORDER_AGE {
                if let Some(order_id) = order.exchange_order_id {
                    // sen directly to the order cancell queue , expose a function 
                    cancel_batch.push(CancelData { symbol  , client_id: order.client_id, order_id: Some(order_id) });
                    order.state = OrderState::PendingCancel;
                }
            }
        }
    }

    pub fn should_requote(&self) -> bool {


        // dont quote again in emergency mode 
        if matches!(self.state.current_mode, QuotingMode::Emergency) {
            return false;
        }
        

        // nit enough time passed 
        if self.orders.last_quote_time.elapsed() < QUOTING_GAP {
            return false;
        }

        // mode chNged 
        if self.state.current_mode != self.state.prev_mode {
            return true;
        }
        
        // getting active orders
        let active_bids = self.orders.pending_orders.iter()
            .filter(|o| o.side == Side::BID && matches!(o.state, OrderState::Active))
            .count();
        
        let active_asks = self.orders.pending_orders.iter()
            .filter(|o| o.side == Side::ASK && matches!(o.state, OrderState::Active))
            .count();
        
        let total_active = active_bids + active_asks;
        
        
        if total_active == 0 {
            return true;
        }
        
       
        match self.state.current_mode {
            QuotingMode::Bootstrap { levels, .. } => {
                let expected = levels * 2;
                if total_active < expected / 2 {
                    return true;
                }
            }
            
            QuotingMode::Normal { levels, .. } => {
                let expected = levels * 2;
                // Missing one side
                if active_bids == 0 || active_asks == 0 {
                    return true;
                }
                // Too few orders
                if total_active < expected / 2 {
                    return true;
                }
            }
            
            QuotingMode::Stressed { levels, .. } => {
                let expected = levels * 2;
                if total_active < expected / 2 {
                    return true;
                }
            }
            
            QuotingMode::InventoryCapped { side, levels } => {
                let expected = levels;
                let active_on_required_side = match side {
                    InventorySatus::Long => active_asks,   // Need asks to sell
                    InventorySatus::Short => active_bids,  // Need bids to buy
                };
                
                if active_on_required_side < expected / 2 {
                    return true;
                }
            }
            
            QuotingMode::Emergency => {
                return false;
            }
        }


        let mid_move = (self.state.market_state.mid_price - self.state.prev_mid_price).abs();

        // your tick-size logic can go here, for now do % threshold
        let mid_move_pct = if self.state.prev_mid_price != dec!(0) {
            (mid_move / self.state.prev_mid_price).to_f64().unwrap_or(0.0)
        } else {
            0.0
        };

         // 0.05% mid drift triggers requote (tune this)
        if mid_move_pct >= 0.0005 {
            return true;
        }

        // default dont 
        false
    }


    pub fn compute_target_ladder(&self)->Result<TargetLadder , MmError>{
        match self.state.current_mode{
            QuotingMode::Emergency =>{
                Ok(TargetLadder {
                    bids: Vec::new(),
                    asks: Vec::new(),
                })
            }

            QuotingMode::Bootstrap { spread_pct, levels } => {
                self.build_bootstrap_ladder(spread_pct, levels)
            }
            
            QuotingMode::Normal { levels, size_decay } => {
                self.build_normal_ladder( levels, size_decay)
            }
            
            QuotingMode::Stressed { spread_mult, levels } => {
                self.build_stressed_ladder(spread_mult, levels)
            }
            
            QuotingMode::InventoryCapped { side, levels } => {
                self.build_capped_ladder(side, levels)
            }
        }
       
    }


    pub fn build_bootstrap_ladder(&self , spread_pct : Decimal , levels:usize)->Result<TargetLadder , MmError>{
        const TICK_SIZE: Decimal = dec!(0.25); // configure 
        const BASE_SIZE: u64 = 100; // configure 
        
        let half_spread = self.state.ipo_price * spread_pct / dec!(2);
        let center_bid = self.state.ipo_price - half_spread;
        let center_ask = self.state.ipo_price + half_spread;
        
        let mut bids = Vec::with_capacity(levels);
        let mut asks = Vec::with_capacity(levels);
        
        for i in 0..levels {
            let offset = TICK_SIZE * Decimal::from(i);
            let size = (BASE_SIZE as f64 * 0.85_f64.powi(i as i32)) as u64;
            
            bids.push(TargetQuotes {
                price: center_bid - offset,
                qty: size.max(10) as u32,
                side: Side::BID,
                level : i 
                
            });
            
            asks.push(TargetQuotes {
                price: center_ask + offset,
                qty: size.max(10) as u32,
                side: Side::ASK,
                level : i 
            });
        }
        
        Ok(TargetLadder { bids, asks })
    }

    pub fn build_normal_ladder(&self , levels : usize , size_decay : f64)->Result<TargetLadder , MmError>{
        const TICK_SIZE: Decimal = dec!(0.25); 
        match calculate_optimal_quotes(
            self.state.market_state.mid_price, 
            self.state.inventory.quantity, 
            self.state.risk_aversion,
            self.state.market_state.volatility,
            self.state.time_to_terminal,
            self.state.liquidity_k
        ){
            Ok(quotes)=>{
                // optimal bid price and the optimal ask price 
                let (bid_size , ask_size) = self.state.compute_quote_sizes();

                let mut bids = Vec::with_capacity(levels);
                let mut asks = Vec::with_capacity(levels);


                for i in 0..levels {
                    let offset = TICK_SIZE * Decimal::from(i);
                    let bid_size = (bid_size as f64 * size_decay.powi(i as i32)) as u64;
                    let ask_size = (ask_size as f64 * size_decay.powi(i as i32)) as u64;

                    bids.push(TargetQuotes {
                        price: quotes.0 - offset,
                        qty: bid_size.max(10) as u32,
                        side: Side::BID,
                        level : i 

                    });

                    asks.push(TargetQuotes {
                        price: quotes.1 + offset,
                        qty: ask_size.max(10) as u32,
                        side: Side::ASK,
                        level : i
                    });
                }

                Ok(TargetLadder { bids, asks })
            }
            Err(_)=>{
                return Err(MmError::CouldNotCalculateQuotes);
            }
        }
        

        

            
    }

    pub fn build_stressed_ladder(&self , spread_mult : Decimal , levels : usize)->Result<TargetLadder , MmError>{
        const TICK_SIZE: Decimal = dec!(0.25);
        const BASE_SIZE: u64 = 50;

        match calculate_optimal_quotes(
            self.state.market_state.mid_price, 
            self.state.inventory.quantity, 
            self.state.risk_aversion,
            self.state.market_state.volatility,
            self.state.time_to_terminal,
            self.state.liquidity_k
        ){
            Ok(quotes)=>{
                let center_bid = quotes.0;
                let center_ask = quotes.1;

                let current_spread = center_ask - center_bid;
                let extra_spread = current_spread * (spread_mult - dec!(1)) / dec!(2);
                
                let new_bid = center_bid - extra_spread;
                let new_ask = center_ask + extra_spread;
                
                let mut bids = Vec::with_capacity(levels);
                let mut asks = Vec::with_capacity(levels);
                
                for i in 0..levels {
                    let offset = TICK_SIZE * Decimal::from(i);
                    let size = (BASE_SIZE as f64 * 0.80_f64.powi(i as i32)) as u64;
                    
                    bids.push(TargetQuotes {
                        price: new_bid - offset,
                        qty: size.max(10) as u32,
                        side: Side::BID,
                        level : i 
                    });
                    
                    
                    asks.push(TargetQuotes {
                        price: new_ask + offset,
                        qty: size.max(10) as u32,
                        side: Side::ASK,
                        level : i 
                    });
                }
                
                Ok(TargetLadder { bids, asks })
            }
            Err(_)=>{
                return Err(MmError::CouldNotCalculateQuotes);
            }
        }
    }

    pub fn build_capped_ladder(&self , side : InventorySatus ,levels : usize)->Result<TargetLadder , MmError>{
        const TICK_SIZE: Decimal = dec!(0.25);
        const BASE_SIZE: u64 = 150; 
        match calculate_optimal_quotes(
            self.state.market_state.mid_price, 
            self.state.inventory.quantity, 
            self.state.risk_aversion,
            self.state.market_state.volatility,
            self.state.time_to_terminal,
            self.state.liquidity_k
        ){
            Ok(quotes)=>{
                let center_bid = quotes.0;
                let center_ask = quotes.1;


                let mut bids = Vec::new();
                let mut asks = Vec::new();

                match side {
                    InventorySatus::Long => {
                        // Only asks (to sell)
                        for i in 0..levels {
                            let offset = TICK_SIZE * Decimal::from(i);
                            let size = (BASE_SIZE as f64 * 0.90_f64.powi(i as i32)) as u64;
                            
                            asks.push(TargetQuotes {
                                price: center_ask + offset,
                                qty: size.max(10) as u32,
                                side: Side::ASK,
                                level :  i 
                            });
                        }
                    }
                    
                    InventorySatus::Short => {
                        // Only bids (to buy)
                        for i in 0..levels {
                            let offset = TICK_SIZE * Decimal::from(i);
                            let size = (BASE_SIZE as f64 * 0.90_f64.powi(i as i32)) as u64;
                            
                            bids.push(TargetQuotes {
                                price: center_bid - offset,
                                qty: size.max(10) as u32,
                                side: Side::BID,
                                level : i 
                            });
                        }
                    }
                }

                Ok(TargetLadder { bids, asks })
        
            }

            Err(_)=>{
                return Err(MmError::CouldNotCalculateQuotes);
            }
        }
    }


    pub fn incremental_requote(&mut self ,  target_ladder : &mut TargetLadder , symbol : u32)->Result<(Vec<(u64 , u64)> , Vec<PostData>) , MmError>{
        const PRICE_TOLERANCE: Decimal = dec!(0.1);  // 10 cent / 10 paise 
        
     //   let mut orders_to_keep = Vec::new();
        let mut order_to_cancel = Vec::new();   
        let mut order_to_post = Vec::new();



        for order in &mut self.orders.pending_orders{
            // if state other than these two , we can skip 
            if !matches!(order.state, OrderState::Active | OrderState::PartiallyFilled) {
                continue;
            }

            let should_keep = match order.side {
                Side::BID =>{
                    // this is a bid order , 
                    target_ladder.bids.iter().any(
                        |target_quote|
                        target_quote.level == order.level && (order.price - target_quote.price).abs() <= PRICE_TOLERANCE
                    )
                }
                Side::ASK=>{
                    target_ladder.asks.iter().any(
                        |target_quote|
                        target_quote.level == order.level && (order.price - target_quote.price).abs() <= PRICE_TOLERANCE
                    )
                }
            };

            if should_keep {
                //orders_to_keep.push(order);
                // if it is not in cancel we obviously are keeping it 
            }
            else{
                // we send for canncelation 
                if let Some(order_id) = order.exchange_order_id{
                    order_to_cancel.push((order_id , order.client_id)); // push here for now can cancel in main loop
                }
            }
        }
          // identifiying the levels which are required to be posted 
        for target_quote in &mut target_ladder.asks{
            let already_have = self.orders.pending_orders.iter().any(
                |current_quote|
                target_quote.side == current_quote.side
                 && target_quote.level == current_quote.level 
                 && (target_quote.price-current_quote.price).abs() <= PRICE_TOLERANCE
            );

            if !already_have {
                // we need to post this order , either we would return all the orders to be posted , or just post from here 
                order_to_post.push(PostData{
                    price : target_quote.price ,
                    qty : target_quote.qty , 
                    side : target_quote.side,
                    symbol
                });
            }
        }

        for target_quote in &mut target_ladder.bids{
            let already_have = self.orders.pending_orders.iter().any(
                |current_quote|
                target_quote.side == current_quote.side
                 && target_quote.level == current_quote.level 
                 && (target_quote.price-current_quote.price).abs() <= PRICE_TOLERANCE
            );

            if !already_have {
                // we need to post this order , either we would return all the orders to be posted , or just post from here 
                order_to_post.push(PostData{
                    price : target_quote.price ,
                    qty : target_quote.qty , 
                    side : target_quote.side , 
                    symbol
                });
            }
        }

        self.orders.last_quote_time = Instant::now();

        Ok((order_to_cancel , order_to_post))
    }
}

pub struct MarketMaker{

    // Data manager for all symbols 
  //  pub symbol_states: FxHashMap<u32, SymbolState>,
    pub message_queue : MessageFromApiQueue,
    pub fill_queue    : MarketMakerFillQueue,
    pub feed_queue    : MarketMakerFeedQueue,
    pub volitality_estimator : VolatilityEstimator ,



    //ORDER MANAGER 
    pub order_queue   : MarketMakerOrderQueue,
  //  pub symbol_orders: FxHashMap<u32, SymbolOrders>,


    pub symbol_ctx  : FxHashMap<u32 , SymbolContext>,

    // quoting engine , currrent mode shud also be per symbol 


    pub cancel_batch : Vec<CancelData>,
    pub post_bacth   : Vec<PostData>
  
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
            //symbol_orders : FxHashMap::with_capacity_and_hasher(MAX_SYMBOLS, Default::default()),
            order_queue : order_queue.unwrap(),
            fill_queue : fill_queue.unwrap(),
            feed_queue : feed_queue.unwrap(),
            message_queue : message_from_api_queueu.unwrap(),
            volitality_estimator: VolatilityEstimator::new() , 
            symbol_ctx : FxHashMap::with_capacity_and_hasher(MAX_SYMBOLS, Default::default()),
            //symbol_states : FxHashMap::with_capacity_and_hasher(MAX_SYMBOLS, Default::default())
            cancel_batch : Vec::with_capacity(4096),
            post_bacth : Vec::with_capacity(4096),
        }
    }
    #[inline(always)]
    pub fn update_state_from_feed(&mut self , market_feed : MarketMakerFeed)->Result<() , MmError>{
        let symbol = market_feed.symbol;
        
        match self.symbol_ctx.get_mut(&symbol) {
            Some(ctx)=>{
                // store the prev best
                ctx.state.prev_best_bid = ctx.state.best_bid;
                ctx.state.prev_best_ask = ctx.state.best_ask;
                ctx.state.prev_best_bid_qty = ctx.state.best_bid_qty;
                ctx.state.prev_best_ask_qty = ctx.state.best_ask_qty;
                ctx.state.prev_mid_price = ctx.state.market_state.mid_price;

                ctx.state.best_ask = Decimal::from(market_feed.best_ask);
                ctx.state.best_bid = Decimal::from(market_feed.best_bid);
                ctx.state.best_ask_qty = market_feed.best_ask_qty;
                ctx.state.best_bid_qty = market_feed.best_bid_qty;


                if !ctx.state.is_bootstrapped {
                    // If price moved AND depth decreased, a trade likely happened
                    let bid_moved = ctx.state.best_bid != ctx.state.prev_best_bid;
                    let ask_moved = ctx.state.best_ask != ctx.state.prev_best_ask;
                    
                    let bid_depth_decreased = ctx.state.best_bid_qty < ctx.state.prev_best_bid_qty;
                    let ask_depth_decreased = ctx.state.best_ask_qty < ctx.state.prev_best_ask_qty;
                    
                    // Infer trade on bid side
                    if bid_moved || bid_depth_decreased {
                        let qty_change = ctx.state.prev_best_bid_qty.saturating_sub(ctx.state.best_bid_qty);
                        if qty_change > 0 {
                            ctx.state.total_volume += qty_change as u64;
                            ctx.state.total_trades += 1;
                        }
                    }
                    
                    // Infer trade on ask side
                    if ask_moved || ask_depth_decreased {
                        let qty_change = ctx.state.prev_best_ask_qty.saturating_sub(ctx.state.best_ask_qty);
                        if qty_change > 0 {
                            ctx.state.total_volume += qty_change as u64;
                            ctx.state.total_trades += 1;
                        }
                    }
                }




                ctx.state.market_state.mid_price = (ctx.state.best_ask + ctx.state.best_bid)/dec!(2);
                // mid price changed so the unrelaised pnl aslo changes 
                if ctx.state.inventory.quantity != dec!(0){
                    let new_unrealised = (ctx.state.market_state.mid_price - ctx.state.inventory.avg_entry_price)*ctx.state.inventory.quantity;
                    ctx.state.pnl.update(ctx.state.pnl.realized, new_unrealised);
                }

                // bootstrappijg per symbol 
                
            }
            None =>{
                return Err(MmError::SymbolNotFound);
            }
        }   
        Ok(())
    }
    #[inline(always)]
    pub fn update_inventory_from_fill(&mut self , market_fill : MarketMakerFill)->Result<() , MmError>{
        let symbol = market_fill.symbol;
        let fill_qty = Decimal::from(market_fill.fill_quantity);
        let fill_price = Decimal::from(market_fill.fill_price);
        match market_fill.side_of_mm_order{
            0 =>{
                 // market maker order was a buy (bid order)
                
                match self.symbol_ctx.get_mut(&symbol){
                    Some(ctx)=>{
                        let old_qty = ctx.state.inventory.quantity;
                        let old_avg = ctx.state.inventory.avg_entry_price;
                        ctx.state.inventory.quantity += fill_qty;

                        if ctx.state.inventory.quantity > dec!(0) {
                            ctx.state.inventory.avg_entry_price = 
                                (old_qty * old_avg + fill_qty * fill_price) 
                                / ctx.state.inventory.quantity;
                        }

                        //if it was a buy we got more shares , so the realised PNL wont change bcs we dint sold 
                        // unrealised PNL will 
                        let new_realised = ctx.state.pnl.realized;
                        let new_unrealised = (ctx.state.market_state.mid_price - ctx.state.inventory.avg_entry_price)*ctx.state.inventory.quantity;


                        ctx.state.pnl.update(new_realised, new_unrealised);
                    }
                    None =>{
                        return Err(MmError::SymbolNotFound);
                    }
                }
            }
            1 =>{
                // update PNL 
                match self.symbol_ctx.get_mut(&symbol){
                    Some(ctx)=>{
                       // let old_qty = symbol_state.inventory.quantity;
                        let old_realised = ctx.state.pnl.realized;
                     //   let old_unrealised = symbol_state.pnl.unrealized;

                        // actual PNL that took place from the bid ask spread 
                        let realized_pnl_from_sale = (fill_price - ctx.state.inventory.avg_entry_price) * fill_qty;

                        ctx.state.inventory.quantity -= fill_qty;

                        let new_realized = old_realised + realized_pnl_from_sale;

                        let new_unrealized = if ctx.state.inventory.quantity != dec!(0) {
                            (ctx.state.market_state.mid_price - ctx.state.inventory.avg_entry_price) * ctx.state.inventory.quantity
                        } else {
                            dec!(0)  // No position = no unrealized P&L
                        };

                        ctx.state.pnl.update(new_realized, new_unrealized);
                    
                    }
                    None=>{
                        return Err(MmError::SymbolNotFound);
                    }
                }
            }   
            _ =>{

            }
        }
        Ok(())
    }

    #[inline(always)]
    pub fn order_manager_update_after_fill(&mut self , market_fill : MarketMakerFill)->Result<() , MmError>{
        let symbol = market_fill.symbol; 
        match self.symbol_ctx.get_mut(&symbol){
            Some(ctx)=>{
                if let Some( mm_order) = ctx.orders.pending_orders.iter_mut().find(|pending_order| 
                    match pending_order.exchange_order_id{
                        Some(order_id)=>{
                            order_id == market_fill.order_id_mm_order
                        }
                        None =>{
                            false
                        }
                    }
                ){
                    mm_order.remaining_size = mm_order.remaining_size.saturating_sub(market_fill.fill_quantity);

                    if mm_order.remaining_size == 0{
                        mm_order.state = super::types::OrderState::Active
                    }else{
                        mm_order.state = super::types::OrderState::PartiallyFilled
                    }


                }
                // we can remove the orders which are fully matched 
                ctx.orders.pending_orders.retain(|order| order.remaining_size > 0 );
            }
            None=>{
                return Err(MmError::SymbolNotFound);
            }
        }
        Ok(())
    }

    pub fn get_inventory_status(&mut self , symbol : u32)->Result<InventorySatus , MmError>{
        match self.symbol_ctx.get_mut(&symbol){
            Some(ctx)=>{
                if ctx.state.inventory.quantity > dec!(0){
                    Ok(InventorySatus::Long)
                }
                else{
                    Ok(InventorySatus::Short)
                }
            }

            None=>{
                return Err(MmError::SymbolNotFound);
            }
        }
    }
    #[inline(always)]
    pub fn handle_order_acceptance_ack(&mut self  , api_response : MessageFromApi)->Result<() , MmError>{
        let symbol = api_response.symbol;
        match self.symbol_ctx.get_mut(&symbol){
            Some(ctx)=>{
                if let Some(order) = ctx.orders.pending_orders.iter_mut().find(
                    |pending_order|
                    pending_order.client_id == api_response.client_id

                ){
                    order.exchange_order_id = Some(api_response.order_id);
                    order.state = OrderState::Active;
                }
            }
            None=>{
                return Err(MmError::SymbolNotFound);
            }
        }
        Ok(())
    }
    #[inline(always)]
    pub fn handle_order_cancel_ack(&mut self, api_response : MessageFromApi)->Result<() , MmError>{
        let symbol = api_response.symbol;
        match self.symbol_ctx.get_mut(&symbol){
            Some(ctx)=>{
                ctx.orders.pending_orders.retain(|order| order.exchange_order_id != Some(api_response.order_id) );
            }
            None=>{
                return Err(MmError::SymbolNotFound);
            }
        }
        Ok(())
    }

    pub fn check_if_depth_update_causes_cancellation(&mut self , symbol : u32){
        let (
            mid_price,
            prev_mid_price,
            best_bid,
            best_ask,
            prev_best_bid_qty,
            prev_best_ask_qty,
            best_bid_qty,
            best_ask_qty,
        ) = {
            let symbol_context = match self.symbol_ctx.get(&symbol) {
                Some(ctx) => ctx,
                None => return,
            };
            (
                symbol_context.state.market_state.mid_price,
                symbol_context.state.prev_mid_price,
                symbol_context.state.best_bid,
                symbol_context.state.best_ask,
                symbol_context.state.prev_best_bid_qty,
                symbol_context.state.prev_best_ask_qty,
                symbol_context.state.best_bid_qty,
                symbol_context.state.best_ask_qty,
            )
        };


        let symbol_context = match self.symbol_ctx.get_mut(&symbol){
            Some(ctx)=>ctx ,
            None => return 
        };

        let price_move = (mid_price - prev_mid_price).abs();
        // percentage change in price 
        let price_move_pct = if prev_mid_price != dec!(0) {
            (price_move / prev_mid_price).to_f64().unwrap_or(0.0)
        } else {
            0.0
        };

        if price_move_pct > 0.001 {  // 0.1% move
            for order in &mut symbol_context.orders.pending_orders {
                if order.state != OrderState::Active {
                    continue;
                }
                
                let should_cancel = match order.side {
                    Side::BID => order.price > mid_price,  // Bid above mid
                    Side::ASK => order.price < mid_price,  // Ask below mid
                    _ => false,
                };
                
                if should_cancel {
                    if let Some(order_id) = order.exchange_order_id {
                        // send cancellation request 
                        self.cancel_batch.push(CancelData { symbol, client_id: order.client_id, order_id  : Some(order_id) });
                        order.state = OrderState::PendingCancel;
                    }
                }
            }
        }

        let current_spread = best_ask - best_bid;
        if current_spread < dec!(0.01) {  // $0.01 minimum
            
            for order in &mut symbol_context.orders.pending_orders {
                if order.state == OrderState::Active {
                    if let Some(order_id) = order.exchange_order_id {
                        self.cancel_batch.push(CancelData { symbol, client_id: order.client_id, order_id  : Some(order_id) });
                        order.state = OrderState::PendingCancel;
                    }
                }
            }
            return;  // No need to check other triggers
        }
        

        if prev_best_bid_qty > 0 {
            let bid_depth_ratio = best_bid_qty as f64 / prev_best_bid_qty as f64;
            
            if bid_depth_ratio < 0.3 {  // 70% of depth gone
                
                
                for order in &mut symbol_context.orders.pending_orders {
                    if order.side == Side::BID && order.state == OrderState::Active {
                        if let Some(order_id) = order.exchange_order_id {
                            self.cancel_batch.push(CancelData { symbol, client_id: order.client_id, order_id  : Some(order_id) });
                            order.state = OrderState::PendingCancel;
                        }
                    }
                }
            }
        }
        if prev_best_ask_qty > 0 {
            let ask_depth_ratio = best_ask_qty as f64 / prev_best_ask_qty as f64;
            
            if ask_depth_ratio < 0.3 {
                for order in &mut symbol_context.orders.pending_orders {
                    if order.side == Side::ASK && order.state == OrderState::Active {
                        if let Some(order_id) = order.exchange_order_id {
                           //self.send_cancel_request(symbol, order.client_id, order_id);
                            self.cancel_batch.push(CancelData { symbol, client_id: order.client_id, order_id  : Some(order_id) });
                            order.state = OrderState::PendingCancel;
                        }
                    }
                }
            }
        }
    }


    pub fn send_cancel_request(&mut self , symbol : u32 , client_id : u64 , order_id : u64 )->Result<() , QueueError>{
        match self.order_queue.enqueue(MmOrder { 
            order_id, 
            client_id, 
            price: 0, 
            timestamp: 0, 
            shares_qty: 0, 
            symbol, 
            side: 2, 
            order_type: 1, 
            status: 4
        }){
            Ok(_)=>{

            }
            Err(queue_error)=>{
                eprintln!(" enqueue erro {:?}" , queue_error);
            }
        }
        Ok(())
    }

    pub fn send_post_request(&mut self , symbol : u32 , price : Decimal , qty: u32, side : Side)->Result<() , QueueError>{
        match self.symbol_ctx.get_mut(&symbol){
            Some(ctx)=>{
                match self.order_queue.enqueue(MmOrder { 
                    order_id : 0 , 
                    client_id : ctx.orders.alloc_client_id() , 
                    price : price.to_u64().unwrap(), 
                    timestamp: 0, 
                    shares_qty: qty, 
                    symbol, 
                    side: match side {
                        Side::ASK => 1 ,
                        Side::BID => 0 
                    }, 
                    order_type: 0, 
                    status: 0
                }){
                    Ok(_)=>{
        
                    }
                    Err(queue_error)=>{
                        eprintln!(" enqueue erro {:?}" , queue_error);
                    }
                }
            }
            None =>{

            }
        }
        
        Ok(())
    }

    pub fn get_active_order_cnt(&self , symbol : u32)->Result<(usize , usize) , MmError>{
        let ctx = match self.symbol_ctx.get(&symbol){
            Some(context) => context , 
            None => {
                return Err(MmError::SymbolNotFound);
            }
        };

        let bids = ctx.orders.pending_orders.iter().filter(
            |order|
            order.side == Side::BID && order.state == OrderState::Active
        ).count();

        let asks = ctx.orders.pending_orders.iter().filter(
            |order|
            order.side == Side::ASK && order.state == OrderState::Active
        ).count();

        Ok((bids , asks ))
    }

    pub fn cancel_all_orders(&mut self , symbol : u32){

    }

    pub fn cancel_side(&mut self , symbol : u32){

    }
    pub fn run_market_maker(&mut self ){
        loop{

            self.cancel_batch.clear();
            self.post_bacth.clear();
            // first we co nsume the feed from the engine 
            while let  Ok(Some(feed)) = self.feed_queue.dequeue(){
                let symbol = feed.symbol;
                // update the feed for that symbol 
                match self.update_state_from_feed(feed){
                    Ok(_)=>{
                        self.check_if_depth_update_causes_cancellation(symbol);
                    }
                    Err(error)=>{
                        eprintln!(" feed update error {:?}" , error);
                    }
                }
            }

            // now needing to process fills 
            while let Ok(Some(fill)) = self.fill_queue.dequeue(){
                // state(inventory ) shud change , order manager change 
                let _= self.update_inventory_from_fill(fill);
                // order manager update 
                let _ = self.order_manager_update_after_fill(fill);
            }


            while let Ok(Some(api_message)) = self.message_queue.dequeue(){
                let symbol = api_message.symbol;
                match api_message.message_type{
                    0 =>{
                        // adding thr symbol , directly adding the context 
                        self.symbol_ctx.insert(symbol, SymbolContext::new(Decimal::from(api_message.ipo_price), symbol));
                    }
                    1 =>{
                        // order accepted ack
                        self.handle_order_acceptance_ack(api_message).expect("coulndt handle the order acceptance ");
                    }
                    2=>{
                        // cancale ordr ack
                        self.handle_order_cancel_ack(api_message).expect("coundt handle the order cancellation ack ")
                    }
                    _=>{

                    }
                }
            }

            // updating the steate loop
            for (symbol  , ctx) in self.symbol_ctx.iter_mut(){
                let deref_symbol = *symbol;
                if ctx.state.last_sample_time.elapsed() >= SAMPLE_GAP{
                    ctx.state.rolling_prices.push(ctx.state.market_state.mid_price);
                    ctx.state.last_sample_time = Instant::now();
                }

                if ctx.state.last_volatility_calc.elapsed() >= VOLITILTY_CALC_GAP{
                    let new_vol = self.volitality_estimator.calculate_simple(ctx.state.rolling_prices.as_slice_for_volatility());
                    if new_vol.is_err(){
                        eprint!("error in volatility calc");
                    }
                    ctx.state.market_state.volatility = new_vol.unwrap();
                    ctx.state.last_volatility_calc = Instant::now();
                }

                if ctx.state.last_management_cycle_time.elapsed() >= CYCLE_GAP{
                    for active_order in &mut ctx.orders.pending_orders{
                       match ctx.state.should_cancel_unprofitable_order(active_order, ctx.state.market_state.mid_price, (ctx.state.best_ask - ctx.state.best_bid)){
                            true =>{
                                if active_order.state == OrderState::Active{
                                    self.cancel_batch.push(CancelData { symbol : *symbol , client_id: active_order.client_id, order_id: active_order.exchange_order_id });
                                }
                              
                              // can safely unwrap iguess // but we can have a case , where the order ack dint come and we are 
                              // on a stage of cancelling , keep option itself , can check when we enqueue 
                            }

                            false=>{

                            }
                       }

                       match ctx.state.should_cancel_due_to_inventory(active_order ,  ctx.state.inventory.quantity){
                            true =>{
                                if active_order.state == OrderState::Active{
                                    self.cancel_batch.push(CancelData { symbol : *symbol , client_id: active_order.client_id, order_id: active_order.exchange_order_id });
                                    active_order.state = OrderState::PendingCancel;
                                }
                              // can safely unwrap iguess // but we can have a case , where the order ack dint come and we are 
                              // on a stage of cancelling , keep option itself , can check when we enqueue 
                            }
                        
                            false=>{
                            
                            }
                        }
                    }

                    ctx.check_if_time_caused_cancellation(*symbol, &mut self.cancel_batch);
                    
                    

                    if !ctx.state.is_bootstrapped && ctx.state.should_exit_bootstrap() {
                        ctx.state.is_bootstrapped = true;
                    }

                    let _ = ctx.state.determine_mode(); // no need to return , just update the mode 


                    if ctx.should_requote(){
                        // we compute target laders and try to modify them 
                        match ctx.compute_target_ladder(){
                            Ok(mut target_ladder)=>{
                               if let Ok(requote_result) = ctx.incremental_requote(&mut target_ladder , *symbol){
                                    let orders_to_cancel = requote_result.0;
                                    let orders_to_post = requote_result.1;

                                    for order in orders_to_cancel {
                                        self.cancel_batch.push(CancelData { symbol : deref_symbol , client_id: order.1, order_id: Some(order.0) });
                                    }

                                    for order in orders_to_post{
                                        self.post_bacth.push(PostData { price: order.price, qty: order.qty, side: order.side , symbol : *symbol });
                                    }
                               }
                            }

                            Err(_)=>{
                                eprint!("error occpured in the compute target ladder function ")
                            }
                        }
                    }


                    // shoudl i send requsts here 



                }
            }

            // or shud i send requet here 
            // cudnt call the function becuse it took a mutable refrence to entire self 
            for cancel_order in &mut  self.cancel_batch{
                match cancel_order.order_id{
                    Some(id)=>{
                        match self.order_queue.enqueue(MmOrder { 
                            order_id : id, 
                            client_id : cancel_order.client_id, 
                            price: 0, 
                            timestamp: 0, 
                            shares_qty: 0, 
                            symbol : cancel_order.symbol, 
                            side: 2, 
                            order_type: 1, 
                            status: 4
                        }){
                            Ok(_)=>{
                
                            }
                            Err(queue_error)=>{
                                eprintln!(" enqueue erro {:?}" , queue_error);
                            }
                        }
                    }
                    None =>{

                    }
                }
            }


            for post_order in &mut self.post_bacth{
                match self.symbol_ctx.get_mut(&post_order.symbol){
                    Some(ctx)=>{
                        match self.order_queue.enqueue(MmOrder { 
                            order_id : 0 , 
                            client_id : ctx.orders.alloc_client_id() , 
                            price : post_order.price.to_u64().unwrap(), 
                            timestamp: 0, 
                            shares_qty: post_order.qty, 
                            symbol : post_order.symbol, 
                            side: match post_order.side {
                                Side::ASK => 1 ,
                                Side::BID => 0 
                            }, 
                            order_type: 0, 
                            status: 0
                        }){
                            Ok(_)=>{
                
                            }
                            Err(queue_error)=>{
                                eprintln!(" enqueue erro {:?}" , queue_error);
                            }
                        }
                    }
                    None =>{
        
                    }
                }
            }





        }
    }
}



