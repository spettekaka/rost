pub mod chacha20;

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
