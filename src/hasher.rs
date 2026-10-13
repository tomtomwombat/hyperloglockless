use core::hash::{BuildHasher, Hasher};
#[cfg(not(feature = "rand"))]
use rapidhash::quality::RandomState;
use rapidhash::quality::{RapidHasher, SeedableState};

const RAPID_SECRETS: [u64; 7] = [
    0x2d35_8dcc_aa6c_78a5,
    0x8bb8_4b93_962e_acc9,
    0x4b33_a62e_857b_191d,
    0x6a9d_1a5b_4f3c_2e17,
    0x1c69_b3f7_4ac4_ae35,
    0x9e37_79b9_7f4a_7c15,
    0x4f1b_bcdd_7a2d_8c13,
];

#[derive(Clone, Debug, Default, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CloneBuildHasher<H: Hasher + Clone> {
    hasher: H,
}

impl<H: Hasher + Clone> CloneBuildHasher<H> {
    fn new(hasher: H) -> Self {
        Self { hasher }
    }
}

impl<H: Hasher + Clone> BuildHasher for CloneBuildHasher<H> {
    type Hasher = H;
    #[inline]
    fn build_hasher(&self) -> Self::Hasher {
        self.hasher.clone()
    }
}

/// The default hasher for [`crate::HyperLogLog`] and
/// [`crate::AtomicHyperLogLog`].
///
/// It stores a pre-initialized hasher and clones it for each build. This avoids
/// rebuilding rapidhash's per-hasher state for every item inserted.
pub type DefaultHasher = CloneBuildHasher<RandomDefaultHasher>;

impl DefaultHasher {
    pub fn seeded(seed: u64) -> Self {
        Self::new(RandomDefaultHasher::seeded(seed))
    }
}

#[derive(Clone)]
pub struct RandomDefaultHasher {
    #[cfg(feature = "serde")]
    seed: u64,
    hasher: RapidHasher<'static>,
}

impl core::fmt::Debug for RandomDefaultHasher {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RandomDefaultHasher").finish_non_exhaustive()
    }
}

impl RandomDefaultHasher {
    #[inline]
    pub fn seeded(seed: u64) -> Self {
        // Mix the caller's seed before using it as the rapidhash key. Keep the
        // secrets fixed so a seeded sketch retains identical hashes after serde
        // round-trips, including across process boundaries.
        let mixed_seed = SeedableState::custom(seed, &RAPID_SECRETS).hash_one(seed);
        let hasher = SeedableState::custom(mixed_seed, &RAPID_SECRETS).build_hasher();
        Self {
            #[cfg(feature = "serde")]
            seed,
            hasher,
        }
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for RandomDefaultHasher {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_newtype_struct("RandomDefaultHasher", &self.seed)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for RandomDefaultHasher {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let seed = <u64 as serde::Deserialize>::deserialize(deserializer)?;
        Ok(Self::seeded(seed))
    }
}

impl Default for RandomDefaultHasher {
    #[inline]
    fn default() -> Self {
        #[cfg(feature = "rand")]
        let seed = {
            use rand::Rng;
            rand::rng().random()
        };

        #[cfg(not(feature = "rand"))]
        let seed = RandomState::default().hash_one(0x7261_7069_6468_6173_u64);

        Self::seeded(seed)
    }
}

impl Hasher for RandomDefaultHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.hasher.finish()
    }
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        self.hasher.write(bytes)
    }
    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.hasher.write_u8(i)
    }
    #[inline]
    fn write_u16(&mut self, i: u16) {
        self.hasher.write_u16(i)
    }
    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.hasher.write_u32(i)
    }
    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.hasher.write_u64(i)
    }
    #[inline]
    fn write_u128(&mut self, i: u128) {
        self.hasher.write_u128(i)
    }
    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.hasher.write_usize(i)
    }
    #[inline]
    fn write_i8(&mut self, i: i8) {
        self.hasher.write_i8(i)
    }
    #[inline]
    fn write_i16(&mut self, i: i16) {
        self.hasher.write_i16(i)
    }
    #[inline]
    fn write_i32(&mut self, i: i32) {
        self.hasher.write_i32(i)
    }
    #[inline]
    fn write_i64(&mut self, i: i64) {
        self.hasher.write_i64(i)
    }
    #[inline]
    fn write_i128(&mut self, i: i128) {
        self.hasher.write_i128(i)
    }
    #[inline]
    fn write_isize(&mut self, i: isize) {
        self.hasher.write_isize(i)
    }
}

#[cfg(test)]
mod test {
    use crate::hasher::RandomDefaultHasher;
    use core::hash::Hasher;

    fn hash_all(mut x: impl Hasher) -> u64 {
        x.write(&[1; 16]);
        x.write_u8(1);
        x.write_u16(1);
        x.write_u32(1);
        x.write_u64(1);
        x.write_u128(1);
        x.write_usize(1);
        x.write_i8(1);
        x.write_i16(1);
        x.write_i32(1);
        x.write_i64(1);
        x.write_i128(1);
        x.write_isize(1);
        x.finish()
    }

    #[test]
    fn test_hasher() {
        let h1 = RandomDefaultHasher::seeded(0);
        let h2 = RandomDefaultHasher::seeded(0);
        assert_eq!(hash_all(h1), hash_all(h2),);
    }
}
