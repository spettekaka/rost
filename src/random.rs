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
    csprng: UnsafeCell<ChaCha20Csprng>,
}

unsafe impl Sync for HartEntropySrc {}

impl HartEntropySrc {
    const fn new() -> Self {
        Self {
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

    fn reseed(&mut self, seed: &[u8; 32]);

    fn fill_bytes(&mut self, buf: &mut [u8]);
}

pub fn init() -> Result<(), &'static str> {
    let mut entropy = [0u8; 32];

    get_virtio_entropy(&mut entropy);

    match BASE_CSPRNG.lock() {
        Ok(mut csprng) => csprng.seed(&entropy),
        Err(_) => return Err("Failed to init subsystem for randomness"),
    }
    Ok(())
}

pub fn get_thread_local_source() -> &'static mut HartEntropySrc {
    let hart = crate::arch::riscv::thread_pointer();
    unsafe { &mut PER_HART_SRSC[hart] }
}

pub fn get_random_bytes(dest: &mut [u8]) {
    let hart = crate::arch::riscv::thread_pointer();
    unsafe {
        let csprng = PER_HART_SRSC[hart].csprng.get_mut();
        csprng.fill_bytes(dest);
    }
}

fn get_virtio_entropy(entropy: &mut [u8]) -> bool {
    let (_, mmio) = virtio::take_transport(virtio::VirtIODeviceType::EntropySource)
        .expect("Failed to get rng_device");
    let mut rng_device = VirtIORng::new(mmio);
    log::info!("here1");

    rng_device.request_entropy(entropy);
    log::info!("here2");

    while !ENTROPY_READY.load(Ordering::Acquire) {
        crate::arch::riscv::wait();
    }
    log::info!("here3");
    true
}
