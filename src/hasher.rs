use core::fmt;
use core::hash::BuildHasher;
use rapidhash::quality::{RapidHasher, SeedableState};

const DEFAULT_SECRET: [u64; 7] = [
    0x243F6A8885A308D3,
    0x13198A2E03707344,
    0xA4093822299F31D0,
    0x082EFA98EC4E6C89,
    0x452821E638D01377,
    0xBE5466CF34E90C6C,
    0xC0AC29B7C97C50DD,
];

#[derive(Clone, Eq, PartialEq)]
pub struct DefaultHasher {
    seed: u64,
    state: SeedableState<'static>,
}

impl DefaultHasher {
    pub fn seeded(seed: u64) -> Self {
        Self {
            seed,
            state: SeedableState::custom(seed, &DEFAULT_SECRET),
        }
    }
}

impl BuildHasher for DefaultHasher {
    type Hasher = RapidHasher<'static>;
    #[inline(always)]
    fn build_hasher(&self) -> Self::Hasher {
        self.state.build_hasher()
    }
}

impl fmt::Debug for DefaultHasher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DefaultHasher")
            .field("seed", &self.seed)
            .finish()
    }
}

impl Default for DefaultHasher {
    #[inline]
    fn default() -> Self {
        #[cfg(not(feature = "rand"))]
        {
            use core::hash::Hasher;
            use foldhash::fast::RandomState;
            Self::seeded(RandomState::default().build_hasher().finish())
        }
        #[cfg(feature = "rand")]
        {
            use rand::Rng;
            Self::seeded(rand::rng().random::<u64>())
        }
    }
}

#[cfg(feature = "serde")]
mod serde_impl {
    use super::DefaultHasher;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    impl Serialize for DefaultHasher {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            serializer.serialize_u64(self.seed)
        }
    }

    impl<'de> Deserialize<'de> for DefaultHasher {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            let seed = u64::deserialize(deserializer)?;
            Ok(DefaultHasher::seeded(seed))
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use core::hash::Hasher;

    #[test]
    fn test_equality() {
        let seed = 42;
        let left = DefaultHasher::seeded(seed);
        let right = DefaultHasher::seeded(seed);
        assert_eq!(left, right);
        assert_eq!(left.clone(), right);
        assert_eq!(left, DefaultHasher::seeded(seed));
        assert!(left != DefaultHasher::seeded(420));
        let mut x = left.build_hasher();
        x.write_u64(7);
        assert_eq!(left, DefaultHasher::seeded(seed));
    }
}
