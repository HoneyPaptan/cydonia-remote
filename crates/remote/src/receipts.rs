use crate::proto::Ack;
use std::collections::{HashMap, VecDeque};

pub struct Receipts {
    acks: HashMap<String, Ack>,
    order: VecDeque<String>,
    capacity: usize,
}

impl Receipts {
    pub fn new(capacity: usize) -> Self {
        Self {
            acks: HashMap::new(),
            order: VecDeque::new(),
            capacity: capacity.max(1),
        }
    }

    pub fn get(&self, id: &str) -> Option<&Ack> {
        self.acks.get(id)
    }

    pub fn keep(&mut self, ack: Ack) {
        if self.acks.contains_key(&ack.id) {
            return;
        }
        if self.order.len() == self.capacity
            && let Some(oldest) = self.order.pop_front()
        {
            self.acks.remove(&oldest);
        }
        self.order.push_back(ack.id.clone());
        self.acks.insert(ack.id.clone(), ack);
    }
}
