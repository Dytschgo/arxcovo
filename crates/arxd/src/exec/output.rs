use super::CapturedOutput;
use std::collections::VecDeque;
pub(super) struct Capture {
    head: Vec<u8>,
    tail: VecDeque<u8>,
    limit: usize,
    total: u64,
}
impl Capture {
    pub fn new(limit: usize) -> Self {
        Self {
            head: Vec::new(),
            tail: VecDeque::new(),
            limit,
            total: 0,
        }
    }
    pub fn push(&mut self, bytes: &[u8]) {
        self.total = self.total.saturating_add(bytes.len() as u64);
        for &b in bytes {
            if self.head.len() < self.limit / 2 {
                self.head.push(b);
            } else {
                if self.tail.len() == self.limit - self.limit / 2 {
                    self.tail.pop_front();
                }
                self.tail.push_back(b);
            }
        }
    }
    pub fn finish(self) -> CapturedOutput {
        CapturedOutput {
            dropped_bytes: self
                .total
                .saturating_sub((self.head.len() + self.tail.len()) as u64),
            head: self.head,
            tail: self.tail.into(),
        }
    }
}
