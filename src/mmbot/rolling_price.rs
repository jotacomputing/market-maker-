use std::collections::VecDeque;
use rust_decimal::Decimal;


#[derive(Debug , Clone)]
pub struct RollingPrice{
   pub deque: VecDeque<Decimal>,
   pub capacity: usize,
}

impl RollingPrice{
    pub fn new(capacity: usize, ipo_price: Decimal) -> Self {
        let mut deque = VecDeque::with_capacity(capacity);
        for _ in 0..capacity {
            deque.push_back(ipo_price);
        }
        Self { deque, capacity }
    }

    pub fn push(&mut self, price: Decimal) {
        if self.deque.len() == self.capacity {
            self.deque.pop_front(); 
        }
        self.deque.push_back(price); 
    }

   
    pub fn as_slice_for_volatility(&mut self) -> &[Decimal] {
      
        let contiguous = self.deque.make_contiguous();
        contiguous
    }

    pub fn len(&self) -> usize {
        self.deque.len()
    }

    pub fn clear(&mut self, ipo_price: Decimal) {
        self.deque.clear();
        for _ in 0..self.capacity {
            self.deque.push_back(ipo_price);
        }
    }
}
