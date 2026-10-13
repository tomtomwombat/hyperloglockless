use core::hash::BuildHasher;
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

/// The default hasher for [`crate::HyperLogLog`] and
/// [`crate::AtomicHyperLogLog`].
///
/// It stores Rapidhash's seeded builder state and creates a fresh hasher for
/// each item, allowing the compiler to see that the sponge starts empty.
pub type DefaultHasher = RandomDefaultHasher;

#[derive(Clone)]
pub struct RandomDefaultHasher {
    #[cfg(feature = "serde")]
    seed: u64,
    state: SeedableState<'static>,
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
        let state = SeedableState::custom(mixed_seed, &RAPID_SECRETS);
        Self {
            #[cfg(feature = "serde")]
            seed,
            state,
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

impl BuildHasher for RandomDefaultHasher {
    type Hasher = RapidHasher<'static>;

    #[inline(always)]
    fn build_hasher(&self) -> Self::Hasher {
        self.state.build_hasher()
    }
}

#[cfg(test)]
mod test {
    use crate::hasher::RandomDefaultHasher;
    use core::hash::{BuildHasher, Hasher};

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
        let h1 = RandomDefaultHasher::seeded(0).build_hasher();
        let h2 = RandomDefaultHasher::seeded(0).build_hasher();
        assert_eq!(hash_all(h1), hash_all(h2),);
    }
}
