use core::{
    cell::UnsafeCell,
    sync::atomic::{AtomicUsize, Ordering},
};

use crate::{
    consts::MAX_HARTS,
    random::chacha20::ChaCha20Csprng,
    sync::spinlock::Mutex,
    virtio::{
        self,
        rng::{ENTROPY_READY, VirtIORng},
    },
};

mod chacha20;

/// Global generation counter (0 = uninitialized, >=1 = seeded epoch).
/// See `base_crng.generation` in Linux.
static BASE_GENERATION: AtomicUsize = AtomicUsize::new(0);

static BASE_CSPRNG: Mutex<ChaCha20Csprng> = Mutex::new({ ChaCha20Csprng::unseeded() });

pub struct HartEntropySrc {
    generation: usize,
    csprng: UnsafeCell<ChaCha20Csprng>,
}

unsafe impl Sync for HartEntropySrc {}

impl HartEntropySrc {
    const fn new() -> Self {
        Self {
            generation: 0,
            csprng: UnsafeCell::new(ChaCha20Csprng::unseeded()),
        }
    }
}

static mut PER_HART_SRSC: [HartEntropySrc; MAX_HARTS] = [
    HartEntropySrc::new(),
    HartEntropySrc::new(),
    HartEntropySrc::new(),
    HartEntropySrc::new(),
    HartEntropySrc::new(),
    HartEntropySrc::new(),
    HartEntropySrc::new(),
    HartEntropySrc::new(),
];

pub trait Rng {
    fn next_u8(&mut self) -> u8;

    fn get_random<T: RandomValue>(&mut self) -> T {
        T::random(self)
    }

    fn fill<T: RandomValue>(&mut self, buf: &mut [u8]) {
        for b in buf.iter_mut() {
            *b = self.get_random();
        }
    }
}

pub trait RandomValue {
    fn random<R: Rng + ?Sized>(rng: &mut R) -> Self;
}

macro_rules! impl_get_random_int {
    ($($t:ty), *) => {$(
        impl RandomValue for $t {
            fn random<R: Rng + ?Sized>(rng: &mut R) -> Self {
                let mut bytes = [0u8; core::mem::size_of::<$t>()];
                for b in &mut bytes {
                    *b = rng.next_u8();
                }
                <$t>::from_ne_bytes(bytes)
            }
        }
        )*
    };
}

impl_get_random_int!(u8, u16, u32, u64, u128, i8, i16, i32, i64, i128);

impl<T: RandomValue, const N: usize> RandomValue for [T; N] {
    fn random<R: Rng + ?Sized>(rng: &mut R) -> Self {
        core::array::from_fn(|_| T::random(rng))
    }
}

pub trait KernelCsprng {
    fn seed(&mut self, seed: &[u8; 32]);

    fn reseed(&mut self);

    fn fill_bytes(&mut self, buf: &mut [u8]);
}

pub fn init() {
    let mut entropy = [0u8; 32];

    get_virtio_entropy(&mut entropy);

    BASE_CSPRNG.with_lock(|csprng| {
        csprng.seed(&entropy);
        BASE_GENERATION.store(1, Ordering::Release)
    });
}

pub fn crng_reseed(entropy: &mut [u8; 32]) {
    BASE_CSPRNG.with_lock(|csprng| {
        csprng.seed(entropy);

        entropy.fill(0);
        core::hint::black_box(&entropy);

        // Increment global generation (wrapping to 1 if it ever overflows usize::MAX)
        let next_gen = BASE_GENERATION
            .load(Ordering::Relaxed)
            .wrapping_add(1)
            .max(1);
        BASE_GENERATION.store(next_gen, Ordering::Release);
    });
}

pub fn get_thread_local_source() -> &'static mut HartEntropySrc {
    let hart = crate::arch::riscv::thread_pointer();
    unsafe { &mut PER_HART_SRSC[hart] }
}

pub fn get_random_bytes(dest: &mut [u8]) {
    let hart = crate::arch::riscv::thread_pointer();
    unsafe {
        let csprng = PER_HART_SRSC[hart].csprng.get_mut();
        let generation = PER_HART_SRSC[hart].generation;

        let base_gen = BASE_GENERATION.load(Ordering::Relaxed);

        if generation != base_gen {
            BASE_CSPRNG.with_lock(|base_csprng| {
                let seed = base_csprng.fast_key_erasure();
                let mut seed = to_le_bytes(&seed);
                csprng.seed(&seed);

                seed.fill(0);
                core::hint::black_box(&seed);
            });
            PER_HART_SRSC[hart].generation = base_gen;
        }

        csprng.fill_bytes(dest);
    }
}

fn get_virtio_entropy(entropy: &mut [u8]) {
    let (_, mmio) = virtio::take_transport(virtio::VirtIODeviceType::EntropySource)
        .expect("Failed to get rng_device");
    let mut rng_device = VirtIORng::new(mmio);

    rng_device.request_entropy(entropy);

    while !ENTROPY_READY.load(Ordering::Acquire) {
        crate::arch::riscv::wait();
    }
}

fn to_le_bytes(input: &[u32; 8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (chunk, &val) in out.chunks_exact_mut(4).zip(input.iter()) {
        chunk.copy_from_slice(&val.to_le_bytes()); // Or .to_be_bytes()
    }
    out
}
