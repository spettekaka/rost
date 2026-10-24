use core::num::Wrapping;

pub struct XorShift {
    state: Wrapping<u32>,
}

impl XorShift {
    pub fn from_seed(seed: u32) -> Self {
        Self {
            state: Wrapping(seed),
        }
    }

    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.state = x;
        return self.state.0;
    }
}

impl Iterator for XorShift {
    type Item = u32;

    fn next(&mut self) -> Option<Self::Item> {
        Some(self.next_u32())
    }
}
