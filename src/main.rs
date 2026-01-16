use market_maker::{mmbot::market_maker::MarketMaker, shm::send_order_queue_mm::MarketMakerOrderQueue};

fn main() {
    let _ = MarketMakerOrderQueue::create("/tmp/MarketMakerOrders").expect("failed to open market maker order queue");
    let mut market_maker = MarketMaker::new();
    market_maker.run_market_maker();
}

