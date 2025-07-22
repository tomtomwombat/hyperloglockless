# hyperloglockless
[![Crates.io](https://img.shields.io/crates/v/hyperloglockless.svg)](https://crates.io/crates/hyperloglockless)
[![docs.rs](https://docs.rs/hyperloglockless/badge.svg)](https://docs.rs/hyperloglockless)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://github.com/tomtomwombat/hyperloglockless/blob/main/LICENSE-MIT)
[![License: APACHE](https://img.shields.io/badge/License-Apache-blue.svg)](https://github.com/tomtomwombat/hyperloglockless/blob/main/LICENSE-APACHE)
![Downloads](https://img.shields.io/crates/d/hyperloglockless)

The fastest HyperLogLogs in Rust with bias correction and concurrency support. Used for high-precision cardinality estimation.

## Overview

HyperLogLogs are space efficient data structures for the "count-distinct problem", approximating the number of distinct elements in a multiset. [Paper](https://algo.inria.fr/flajolet/Publications/FlFuGaMe07.pdf).

hyperloglockless offers a lockless concurrent HyperLogLog and a single threaded counterpart. They're simpler, faster, and more accurate than other HyperLogLog implementations:
- 🧵 **Concurrent:** `AtomicHyperLogLog` is a drop-in replacement for `RwLock<OtherHyperLogLog>`: all methods take `&self`, so you can wrap it in `Arc` and update it concurrently without `&mut`.
- ⚡ **Fast:** Designed to be fast and simple in both single and multi-threaded scenarios.
- 🎯 **Accurate:** Empirically verified accuracy for *trillions* of elements; other implementations break down after millions.
- 🔧 **Flexible:** Configurable with custom hashers, seeds, and more registers for higher precision.
- ✅ **Tested:** Rigorously tested with loom and [benchmarked](https://github.com/tomtomwombat/bench-hyperloglogs/tree/main).

## Usage

```toml
[dependencies]
hyperloglockless = "0.3.0"
```

A HyperLogLog with precision `p` uses `2^p` bytes of memory and has an error % of roughly `104 / sqrt(2^p)`.
```rust
use hyperloglockless::HyperLogLog;

let precision = hyperloglockless::precision_for_error(0.01); // 1% error
assert_eq!(precision, 14);

let mut hll = HyperLogLog::new(precision);
hll.insert(&'🦀');
hll.insert_all('a'..='z');

let count = hll.count(); // ~27
assert_eq!(hll.len(), 1 << precision); // 16384 bytes
```
```rust
use hyperloglockless::AtomicHyperLogLog;

let hll = AtomicHyperLogLog::new(14);
hll.insert(&'🦀');
hll.insert_all('a'..='z');
```

## Performance vs Others
hyperloglockless performs better in both a criterion micro-benchmark and while being shared across multiple threads.

![perf](https://github.com/user-attachments/assets/f00d7fa6-e161-4b29-8e80-1e066c85bf65)

## Accuracy vs Others
hyperloglockless stays accurate while other implementations break down after millions of items.

![err](https://github.com/user-attachments/assets/e2caf2da-35f2-4d82-bcb7-fb32b1419071)


## Available Features

- **`rand`** - Enabled by default, this has the `DefaultHasher` source its random state using `thread_rng()` instead of hardware sources. Getting entropy from a user-space source is considerably faster, but requires additional dependencies to achieve this. Disabling this feature by using `default-features = false` makes `DefaultHasher` source its entropy using `getrandom`, which will have a much simpler code footprint at the expense of speed.
- **`serde`** - HyperLogLogs implement `Serialize` and `Deserialize` when possible.
- **`loom`** - `AtomicHyperLogLog`s use [loom](https://github.com/tokio-rs/loom) atomics, making it compatible with loom testing.

## License

Licensed under either of

 * Apache License, Version 2.0
   ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license
   ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
