extern crate alloc;

use core::{
    cell::{SyncUnsafeCell, UnsafeCell},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use alloc::boxed::Box;

use rost_core::{
    consts::MAX_HARTS,
    random::KernelCsprng,
    random::chacha20::ChaCha20Csprng,
    sync::spinlock::Mutex,
    virtio::{self, rng::VirtIORng},
};

/// Global generation counter (0 = uninitialized, >=1 = seeded epoch).
/// See `base_crng.generation` in Linux.
static BASE_GENERATION: AtomicUsize = AtomicUsize::new(0);

static BASE_CSPRNG: Mutex<ChaCha20Csprng> = Mutex::new({ ChaCha20Csprng::unseeded() });

/// Keep track if reesed has already been triggered
static RESEED_TRIGGERED: AtomicBool = AtomicBool::new(false);

static ENTROPY_BUF: SyncUnsafeCell<[u8; 32]> = SyncUnsafeCell::new([0u8; 32]);

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

pub fn init() {
    request_reseed();

    // Initiate a reseed tiemr
    let reseed = rost_core::timer::TimerAction::Periodic(10_000_000, Box::new(request_reseed));

    crate::timer::register_timer_periodic(reseed);
}

/// Non-blocking, safe to call from a interrupt context
///
/// Returns if reseed is already ongoing
pub fn request_reseed() {
    if RESEED_TRIGGERED
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        // Ressed already in motion, return.
        return;
    }
    get_virtio_entropy();
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

fn get_virtio_entropy() {
    let (_, mmio) = virtio::take_transport(virtio::VirtIODeviceType::EntropySource)
        .expect("Failed to get rng_device");
    let mut rng_device = VirtIORng::new(mmio);

    // Safety: RESEED_TRIGGERED ensures exclusive access
    unsafe {
        rng_device.request_entropy(ENTROPY_BUF.get().cast::<u8>(), 32);
    }
}

fn to_le_bytes(input: &[u32; 8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (chunk, &val) in out.as_chunks_mut::<4>().0.iter_mut().zip(input.iter()) {
        chunk.copy_from_slice(&val.to_le_bytes()); // Or .to_be_bytes()
    }
    out
}

pub fn handle_rng_interrupt() {
    let (_, mut mmio) = virtio::take_transport(virtio::VirtIODeviceType::EntropySource)
        .expect("Failed to get rng_device");

    let status = mmio.interrupt_status();
    mmio.ack_interrupt(status);

    if RESEED_TRIGGERED.load(Ordering::Acquire) {
        // SAFETY: IRQ handler owns `ENTROPY_BUF` until release below.
        let entropy = unsafe { &mut *ENTROPY_BUF.get() };
        crng_reseed(entropy);
        RESEED_TRIGGERED.store(false, Ordering::Release);
    }
}
