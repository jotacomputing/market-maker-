use market_maker::{mmbot::market_maker::MarketMaker, shm::{order_queue_mm::MarketMakerOrderQueue, response_queue_mm::MessageFromApiQueue}};

fn main() {
    let _ = MarketMakerOrderQueue::create("/tmp/MarketMakerOrders").expect("failed to open market maker order queue");
    let _ = MessageFromApiQueue::create("/tmp/MessageFromApiToMM").expect("failed to open the message queue api -> mm");
    let mut market_maker = MarketMaker::new();
   // run mm
}

