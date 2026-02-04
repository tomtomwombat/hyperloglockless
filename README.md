# hyperloglockless
[![Crates.io](https://img.shields.io/crates/v/hyperloglockless.svg)](https://crates.io/crates/hyperloglockless)
[![docs.rs](https://docs.rs/hyperloglockless/badge.svg)](https://docs.rs/hyperloglockless)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://github.com/tomtomwombat/hyperloglockless/blob/main/LICENSE-MIT)
[![License: APACHE](https://img.shields.io/badge/License-Apache-blue.svg)](https://github.com/tomtomwombat/hyperloglockless/blob/main/LICENSE-APACHE)
![Downloads](https://img.shields.io/crates/d/hyperloglockless)

High-performance HyperLogLog with bias correction and full concurrency support. Used for accurate and space-efficient cardinality estimation.

## Overview

HyperLogLogs are space efficient data structures for the "count-distinct problem", approximating the number of distinct elements in a multiset. [Paper](https://algo.inria.fr/flajolet/Publications/FlFuGaMe07.pdf).

hyperloglockless offers a lockless concurrent HyperLogLog and a single threaded counterpart. They're simpler, faster, and more accurate than other HyperLogLog implementations:
- 🧵 **Concurrent:** `AtomicHyperLogLog` is a drop-in replacement for `RwLock<OtherHyperLogLog>`: all methods take `&self`, so you can wrap it in `Arc` and update it concurrently without `&mut`.
- ⚡ **Fast:** Designed to be fast and simple in both single and multi-threaded scenarios.
- 🎯 **Accurate:** Empirically verified accuracy for *trillions* of elements; other implementations break down after millions.
- ✅ **Tested:** Rigorously tested with loom and [benchmarked](https://github.com/tomtomwombat/bench-hyperloglogs/tree/main).

## Usage

```toml
[dependencies]
hyperloglockless = "0.3.1"
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

Full concurrency support: `AtomicHyperLogLog` is a drop-in replacement for `RwLock<OtherHyperLogLog>`: all methods take `&self`.
```rust
use hyperloglockless::AtomicHyperLogLog;

let hll = AtomicHyperLogLog::new(14);
hll.insert(&'🦀');
hll.insert_all('a'..='z');
```

## Single-Threaded Performance vs Others
Both `hyperloglockless::HyperLogLog` and `hyperloglockless::AtomicHyperLogLog` are extremely fast for both insert and count calls.

![fp-micro](https://github.com/user-attachments/assets/a8ad464a-3d38-4bd5-b0c0-dd9a701484bd)
![fp-micro](https://github.com/user-attachments/assets/487f2448-b14a-4383-86f1-b41c79c5967c)

## Multi-Threaded Performance vs Others
`hyperloglockless::AtomicHyperLogLog` does not require any locking and therefore avoids thread contention.

![fp-micro](https://github.com/user-attachments/assets/a0868670-d31f-4cd8-8611-5dcf0e55b5ad)

## Accuracy vs Others
hyperloglockless stays consistently accurate while other implementations break down after millions of items.

![fp-micro](https://github.com/user-attachments/assets/2a1042c2-20c0-4c3b-866a-8b604cb3690e)

## Available Features

- **`rand`** - Enabled by default, this has the `DefaultHasher` source its random state using `thread_rng()` instead of hardware sources. Getting entropy from a user-space source is considerably faster, but requires additional dependencies to achieve this. Disabling this feature by using `default-features = false` makes `DefaultHasher` source its entropy using `foldhash`, which will have a much simpler code footprint at the expense of speed.
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
