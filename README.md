# hyperloglockless
[![Github](https://img.shields.io/badge/github-8da0cb?style=for-the-badge&labelColor=555555&logo=github)](https://github.com/tomtomwombat/hyperloglockless)
[![Crates.io](https://img.shields.io/badge/crates.io-fc8d62?style=for-the-badge&labelColor=555555&logo=rust)](https://crates.io/crates/hyperloglockless)
[![docs.rs](https://img.shields.io/badge/docs.rs-66c2a5?style=for-the-badge&labelColor=555555&logo=docs.rs)](https://docs.rs/hyperloglockless)
![Downloads](https://img.shields.io/crates/d/hyperloglockless?style=for-the-badge)

High-performance HyperLogLogs with bias correction and full concurrency support. Used for accurate and space-efficient cardinality estimation.

## Overview

HyperLogLogs are space efficient data structures for the "count-distinct problem", approximating the number of distinct elements in a multiset. [Paper](https://algo.inria.fr/flajolet/Publications/FlFuGaMe07.pdf).

hyperloglockless includes a suite of cardinality estimator implementations based on [HyperLogLog++](https://static.googleusercontent.com/media/research.google.com/en//pubs/archive/40671.pdf) and [Log Log Beta](https://arxiv.org/abs/1612.02284). They're simpler, faster, and more accurate than other cardinality estimator implementations:
- **O(1) Count Calls**: Internal counts are cheaply updated with each insert, hyperloglockless particularly useful for streaming use-cases.
- **Fastest Inserts:** Inserts are faster than any other crate.
- **Concurrency Support:** `AtomicHyperLogLog` is a drop-in replacement for `RwLock<OtherHyperLogLog>`: all methods take `&self`, so you can wrap it in `Arc` and update it concurrently without `&mut`.
- **Sparse Representation:** `HyperLogLogPlus` uses a tweaked version of Google's [sparse representation](https://static.googleusercontent.com/media/research.google.com/en//pubs/archive/40671.pdf). It's 5x faster, 100x more accurate, and uses less memory than other crates implementing sparse representations.
- **Accurate:** Empirically verified accuracy for *trillions* of elements; other implementations break down after millions.
- **Tested:** Rigorously tested with loom and [benchmarked](https://github.com/tomtomwombat/bench-hyperloglogs/tree/main) for speed, memory, and accuracy.

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

## Insert Performance
hyperloglockless is extremely fast for insert calls:

![fp-micro](https://github.com/user-attachments/assets/cce97150-c21c-48ea-82e6-eec04dd323ff)

## Count Performance
`count` calls for hyperloglockless's cardinality estimators are O(1):

![fp-micro](https://github.com/user-attachments/assets/a4b3b6a3-1bd5-405c-a289-b8c139cae3a1)

## Sparse Representation Performance
Below measures and compares the amortized insert performance of `hyperloglockless::HyperLogLogPlus`, which first uses a sparse representation then automatically switches to classic "dense" HLL representation after a certain number of inserts. `hyperloglockless::HyperLogLogPlus` is 5x faster than other sparse implementations while using less memory. It achieves this by eliminating unnecessary hashing, using faster hash encoding, devirtualization avoidance, and smarter memory managment.

![fp-micro](https://github.com/user-attachments/assets/14cb3440-3f18-4dd2-b6a3-38eab485a1d3)

![fp-micro](https://github.com/user-attachments/assets/72c236d9-6983-4034-ba9e-671ee715014a)

## Multi-Threaded Performance
`hyperloglockless::AtomicHyperLogLog` does not require any locking and therefore avoids thread contention.

![fp-micro](https://github.com/user-attachments/assets/a0868670-d31f-4cd8-8611-5dcf0e55b5ad)

## Accuracy
hyperloglockless stays consistently accurate while other implementations break down after millions of items. hyperloglockless's sparse HLL is ~100 more accurate than other sparse implementations. It achieves high accuracy by cramming more information into each hash encoding and using more accurate error correction models.

![fp-micro](https://github.com/user-attachments/assets/8b58e182-3684-4edb-9784-34ae5878746e)

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
