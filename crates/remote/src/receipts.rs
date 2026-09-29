use crate::proto::Ack;
use std::{
    collections::{HashMap, VecDeque},
    fs,
    io::Write as _,
    path::PathBuf,
};

pub struct Receipts {
    acks: HashMap<String, Ack>,
    order: VecDeque<String>,
    capacity: usize,
    journal: Option<PathBuf>,
}

impl Receipts {
    pub fn new(capacity: usize) -> Self {
        Self {
            acks: HashMap::new(),
            order: VecDeque::new(),
            capacity: capacity.max(1),
            journal: None,
        }
    }

    pub fn journaled(path: PathBuf, capacity: usize) -> Self {
        let mut receipts = Self::new(capacity);
        let lines = fs::read_to_string(&path).unwrap_or_default();
        for ack in lines.lines().filter_map(|line| serde_json::from_str(line).ok()) {
            receipts.remember(ack);
        }
        receipts.journal = Some(path);
        receipts.compact();
        receipts
    }

    pub fn get(&self, id: &str) -> Option<&Ack> {
        self.acks.get(id)
    }

    pub fn keep(&mut self, ack: Ack) {
        if self.acks.contains_key(&ack.id) {
            return;
        }
        self.append(&ack);
        self.remember(ack);
    }

    fn remember(&mut self, ack: Ack) {
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

    fn append(&self, ack: &Ack) {
        let Some(path) = &self.journal else { return };
        let Ok(line) = serde_json::to_string(ack) else { return };
        if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(file, "{line}");
        }
    }

    fn compact(&self) {
        let Some(path) = &self.journal else { return };
        let kept: Vec<String> = self
            .order
            .iter()
            .filter_map(|id| self.acks.get(id))
            .filter_map(|ack| serde_json::to_string(ack).ok())
            .collect();
        let _ = fs::write(path, kept.join("\n") + if kept.is_empty() { "" } else { "\n" });
    }
}
