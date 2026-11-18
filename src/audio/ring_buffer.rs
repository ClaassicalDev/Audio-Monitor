use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::Arc;

pub struct AudioRingBuffer {
    buffer: Mutex<VecDeque<[f32; 2]>>,
    max_capacity: usize,
}

impl AudioRingBuffer {
    pub fn new(max_capacity: usize) -> Self {
        Self {
            buffer: Mutex::new(VecDeque::with_capacity(max_capacity)),
            max_capacity,
        }
    }

    pub fn new_shared(max_capacity: usize) -> Arc<Self> {
        Arc::new(Self::new(max_capacity))
    }

    pub fn push_frames(&self, frames: &[[f32; 2]]) {
        let mut lock = self.buffer.lock();
        let total = lock.len() + frames.len();
        if total > self.max_capacity {
            let drop_count = total - self.max_capacity;
            let to_drop = drop_count.min(lock.len());
            lock.drain(0..to_drop);
        }
        lock.extend(frames.iter().copied());
    }

    pub fn pop_frames(&self, out: &mut [[f32; 2]]) -> usize {
        let mut lock = self.buffer.lock();
        let available = lock.len().min(out.len());
        for i in 0..available {
            out[i] = lock.pop_front().unwrap_or([0.0, 0.0]);
        }
        available
    }

    #[allow(dead_code)]
    pub fn available(&self) -> usize {
        self.buffer.lock().len()
    }

    pub fn clear(&self) {
        self.buffer.lock().clear();
    }
}
