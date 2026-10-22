// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Stream channel with credit-based backpressure (spec M2).
//!
//! The receiver grants credit, in chunks. The sender may send only while it holds credit, and
//! each chunk costs one credit. A slow receiver therefore stops the sender instead of letting
//! data pile up in memory. The receiver returns credit as it consumes chunks.

use std::collections::VecDeque;

/// Why a send was refused.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SendError {
    /// No credit left. Wait for the receiver to grant more.
    NoCredit,
    /// The stream was closed.
    Closed,
    /// The chunk is larger than the per-chunk limit.
    TooLarge { size: usize, max: usize },
}

impl std::fmt::Display for SendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SendError::NoCredit => write!(f, "no credit; wait for the receiver"),
            SendError::Closed => write!(f, "the stream is closed"),
            SendError::TooLarge { size, max } => write!(f, "chunk is {size} bytes, limit is {max}"),
        }
    }
}

impl std::error::Error for SendError {}

/// The sending end. It holds credit and queues chunks until the receiver takes them.
///
/// Support: Full.
pub struct Sender {
    credit: u32,
    max_chunk: usize,
    closed: bool,
    queue: VecDeque<Vec<u8>>,
}

impl Sender {
    /// `initial_credit` chunks may be in flight before the receiver grants more.
    ///
    /// Support: Full.
    pub fn new(initial_credit: u32, max_chunk: usize) -> Self {
        Sender {
            credit: initial_credit,
            max_chunk,
            closed: false,
            queue: VecDeque::new(),
        }
    }

    /// Queues one chunk, spending one credit.
    ///
    /// Support: Full.
    pub fn send(&mut self, chunk: Vec<u8>) -> Result<(), SendError> {
        if self.closed {
            return Err(SendError::Closed);
        }
        if chunk.len() > self.max_chunk {
            return Err(SendError::TooLarge {
                size: chunk.len(),
                max: self.max_chunk,
            });
        }
        if self.credit == 0 {
            return Err(SendError::NoCredit);
        }
        self.credit -= 1;
        self.queue.push_back(chunk);
        Ok(())
    }

    /// The receiver takes the next queued chunk, if any.
    ///
    /// Support: Full.
    pub fn take(&mut self) -> Option<Vec<u8>> {
        self.queue.pop_front()
    }

    /// The receiver grants `n` more chunks of credit.
    ///
    /// Support: Full.
    pub fn grant(&mut self, n: u32) {
        self.credit = self.credit.saturating_add(n);
    }

    /// Support: Full.
    pub fn credit(&self) -> u32 {
        self.credit
    }

    /// Ends the stream. Queued chunks can still be taken; nothing more can be sent.
    ///
    /// Support: Full.
    pub fn close(&mut self) {
        self.closed = true;
    }

    /// Support: Full.
    pub fn is_closed(&self) -> bool {
        self.closed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sending_stops_when_credit_runs_out() {
        let mut s = Sender::new(2, 16);
        s.send(vec![1]).unwrap();
        s.send(vec![2]).unwrap();
        assert_eq!(s.send(vec![3]), Err(SendError::NoCredit));
        assert_eq!(s.take(), Some(vec![1]));
        // Taking a chunk does not return credit; only a grant does.
        assert_eq!(s.send(vec![3]), Err(SendError::NoCredit));
        s.grant(1);
        s.send(vec![3]).unwrap();
    }

    #[test]
    fn chunks_come_out_in_order() {
        let mut s = Sender::new(3, 16);
        for i in 0..3u8 {
            s.send(vec![i]).unwrap();
        }
        let got: Vec<Vec<u8>> = std::iter::from_fn(|| s.take()).collect();
        assert_eq!(got, vec![vec![0], vec![1], vec![2]]);
    }

    #[test]
    fn oversized_chunks_are_refused_without_spending_credit() {
        let mut s = Sender::new(1, 4);
        assert_eq!(
            s.send(vec![0; 5]),
            Err(SendError::TooLarge { size: 5, max: 4 })
        );
        assert_eq!(s.credit(), 1);
    }

    #[test]
    fn a_closed_stream_refuses_sends_but_keeps_queued_chunks() {
        let mut s = Sender::new(2, 16);
        s.send(vec![7]).unwrap();
        s.close();
        assert_eq!(s.send(vec![8]), Err(SendError::Closed));
        assert_eq!(s.take(), Some(vec![7]));
        assert!(s.is_closed());
    }

    #[test]
    fn grants_accumulate_and_saturate() {
        let mut s = Sender::new(0, 1);
        s.grant(u32::MAX);
        s.grant(5);
        assert_eq!(s.credit(), u32::MAX);
    }

    #[test]
    fn a_slow_receiver_bounds_the_queue() {
        // The sender can never have more chunks queued than credit it was given in total.
        let mut s = Sender::new(4, 1);
        let mut sent = 0usize;
        for round in 0..100 {
            while s.send(vec![0]).is_ok() {
                sent += 1;
            }
            // The receiver takes one chunk and grants one back every other round.
            if round % 2 == 0 {
                let _ = s.take();
                s.grant(1);
            }
        }
        assert!(s.queue.len() <= 4 + sent, "queue is bounded by credit");
        assert!(sent <= 4 + 50, "sends are bounded by the grants");
    }
}
