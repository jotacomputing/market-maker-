use std::time::Duration;

use market_maker_rs::dec;
use rust_decimal::Decimal;

pub const SAMPLE_GAP : Duration = Duration::from_millis(50);
pub const MAX_SYMBOLS : usize = 100;
pub const VOLITILTY_CALC_GAP  : Duration = Duration::from_millis(100);
pub const QUOTING_GAP : Duration = Duration::from_millis(200);
pub const CYCLE_GAP : Duration = Duration::from_millis(250);
pub const TARGET_INVENTORY : Decimal = dec!(0); 
pub const MAX_SIZE_FOR_ORDER : Decimal = dec!(100) ; 
pub const INVENTORY_CAP :Decimal = dec!(1000);
pub const MAX_BOOK_MULT : Decimal = dec!(2);