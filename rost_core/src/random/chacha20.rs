use crate::random::{KernelCsprng, Rng};

pub struct ChaCha20Csprng {
    key: [u32; 8],
    buffer: [u8; 224], // 4 ChaCha blocks (256B) minus 32B used for immediate re-keying
    pos: usize,        // Current read offset in buffer
    reseed_counter: u32,
    ready: bool,
}

impl ChaCha20Csprng {
    pub const fn unseeded() -> Self {
        Self {
            key: [0; 8],
            buffer: [0; 224],
            pos: 224,
            reseed_counter: 0,
            ready: false,
        }
    }

    pub fn fast_key_erasure(&mut self) -> [u32; 8] {
        let nonce = self.nonce();
        let mut block = chacha20_block(&self.key, 0, nonce);

        // Copy the first 32 bytes to the key,
        self.key.copy_from_slice(&block[0..8]);

        // Copy the last 32 bytes to the buffer, the first 32 bytes are the key
        let mut out = [0u32; 8];
        out.copy_from_slice(&block[8..16]);

        // Zeroise block 0
        block.fill(0);
        core::hint::black_box(&block);
        out
    }

    fn nonce(&self) -> [u32; 2] {
        let cycles = crate::arch::riscv::cycles();
        [cycles as u32, ((cycles >> 32) as u32) ^ self.reseed_counter]
    }

    fn refill(&mut self) {
        let mut bytes = self.buffer;
        self.fill_bytes(&mut bytes);
        self.pos = 0;
    }
}

impl Rng for ChaCha20Csprng {
    fn next_u8(&mut self) -> u8 {
        if self.pos == self.buffer.len() {
            self.refill();
        }

        let val = self.buffer[self.pos];
        self.buffer[self.pos] = 0; // Zeroise consumed byte to prevent backtracking
        self.pos += 1;
        val
    }
}

impl KernelCsprng for ChaCha20Csprng {
    fn seed(&mut self, seed: &[u8; 32]) {
        for (i, chunk) in seed.chunks_exact(4).enumerate() {
            self.key[i] ^= u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        self.fast_key_erasure();
        self.reseed_counter = self.reseed_counter.wrapping_add(1);
        self.ready = true;
    }

    fn reseed(&mut self, seed: &[u8; 32]) {
        self.seed(seed);
    }

    fn fill_bytes(&mut self, mut dest: &mut [u8]) {
        if dest.is_empty() {
            return;
        }

        let mut eph_key = self.fast_key_erasure();
        let nonce = self.nonce();

        let mut counter = 1u64;
        while !dest.is_empty() {
            let mut block = chacha20_block(&eph_key, counter, nonce);
            let take = dest.len().min(64);
            write_words_partial(&block, &mut dest[..take]);

            block.fill(0);
            core::hint::black_box(&block);

            dest = &mut dest[take..];
            counter = counter.wrapping_add(1);
        }

        eph_key.fill(0);
        core::hint::black_box(&eph_key);
    }
}

/// Helper function to copy from a u32 array to a u8 array.
/// Since len dest might not be a multiple of 4, this helps fill dest.
fn write_words_partial(words: &[u32; 16], dest: &mut [u8]) {
    let full = dest.len() / 4;
    for i in 0..full {
        dest[i * 4..(i + 1) * 4].copy_from_slice(&words[i].to_le_bytes());
    }
    let rem = dest.len() % 4;
    if rem > 0 {
        let last = words[full].to_le_bytes();
        dest[full * 4..full * 4 + rem].copy_from_slice(&last[..rem]);
    }
}

fn quarter_round(s: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    s[a] = s[a].wrapping_add(s[b]);
    s[d] ^= s[a];
    s[d] = s[d].rotate_left(16);

    s[c] = s[c].wrapping_add(s[d]);
    s[b] ^= s[c];
    s[b] = s[b].rotate_left(12);

    s[a] = s[a].wrapping_add(s[b]);
    s[d] ^= s[a];
    s[d] = s[d].rotate_left(8);

    s[c] = s[c].wrapping_add(s[d]);
    s[b] ^= s[c];
    s[b] = s[b].rotate_left(7);
}

fn chacha20_block(key: &[u32; 8], cntr: u64, nonce: [u32; 2]) -> [u32; 16] {
    // RFC 7539 ChaCha20 constants
    // ASCII for "expand 32-byte k".
    const C0: u32 = 0x6170_7865;
    const C1: u32 = 0x3320_646e;
    const C2: u32 = 0x7962_2d32;
    const C3: u32 = 0x6b20_6574;
    let initial_state = [
        C0,
        C1,
        C2,
        C3,
        key[0],
        key[1],
        key[2],
        key[3],
        key[4],
        key[5],
        key[6],
        key[7],
        (cntr >> 32) as u32,
        cntr as u32,
        nonce[0],
        nonce[1],
    ];

    let mut state = initial_state;

    for _ in 0..10 {
        quarter_round(&mut state, 0, 4, 8, 12);
        quarter_round(&mut state, 1, 5, 9, 13);
        quarter_round(&mut state, 2, 6, 10, 14);
        quarter_round(&mut state, 3, 7, 11, 15);
        quarter_round(&mut state, 0, 5, 10, 15);
        quarter_round(&mut state, 1, 6, 11, 12);
        quarter_round(&mut state, 2, 7, 8, 13);
        quarter_round(&mut state, 3, 4, 9, 14);
    }

    for i in 0..16 {
        state[i] = state[i].wrapping_add(initial_state[i]);
    }

    state
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chacha20_block_rfc8439_kat() {
        const KEY: [u32; 8] = [
            0x03020100, 0x07060504, 0x0b0a0908, 0x0f0e0d0c, 0x13121110, 0x17161514, 0x1b1a1918,
            0x1f1e1d1c,
        ];
        // Low 32 bits (state[12]) = 0x00000001, High 32 bits (state[13]) = 0x09000000
        const CNTR: u64 = 0x09000000_00000001;
        const NONCE: [u32; 2] = [0x4a000000, 0x00000000];

        const EXPECTED: [u32; 16] = [
            0xe4e7f110, 0x15593bd1, 0x1fdd0f50, 0xc47120a3, 0xc7f4d1c7, 0x0368c033, 0x9aaa2204,
            0x4e6cd4c3, 0x466482d2, 0x09aa9f07, 0x05d7c214, 0xa2028bd9, 0xd19c12b5, 0xb94e16de,
            0xe883d0cb, 0x4e3c50a2,
        ];

        assert_eq!(chacha20_block(&KEY, CNTR, NONCE), EXPECTED);
    }
}
