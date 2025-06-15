# hyperloglockless
[![Crates.io](https://img.shields.io/crates/v/hyperloglockless.svg)](https://crates.io/crates/hyperloglockless)
[![docs.rs](https://docs.rs/hyperloglockless/badge.svg)](https://docs.rs/hyperloglockless)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://github.com/tomtomwombat/hyperloglockless/blob/main/LICENSE-MIT)
[![License: APACHE](https://img.shields.io/badge/License-Apache-blue.svg)](https://github.com/tomtomwombat/hyperloglockless/blob/main/LICENSE-APACHE)
![Downloads](https://img.shields.io/crates/d/hyperloglockless)

Lightning fast concurrent HyperLogLog for Rust.

## Overview

HyperLogLogs are a space efficient data structures for the "count-distinct problem", approximating the number of distinct elements in a multiset. [Paper](https://algo.inria.fr/flajolet/Publications/FlFuGaMe07.pdf).

hyperloglockless is a lockless concurrent HyperLogLog, making it a direct replacement for `RwLock<OtherHyperLogLog<K, V>>`. To accomplish these goals, all methods take `&self` instead of modifying methods taking `&mut self`. This allows you to put a HyperLogLog in an `Arc<T>` and share it between threads while still being able to modify it. HyperLogLog puts great effort into performance and aims to be as fast as possible, even in single threaded scenarios.

hyperloglockless is also extremely accurate--it is able to count billions of items with 99.5% accuracy using only 65kb of memory.

## Usage

```toml
# Cargo.toml
[dependencies]
hyperloglockless = "0.1.0"
```
Basic usage:
```rust
use hyperloglockless::HyperLogLog;

let hll = HyperLogLog::new(16);
hll.insert("42");
hll.insert("🦀");

let count = hll.count();
```

## Performance
![perf-single](https://github.com/user-attachments/assets/8b3df60a-5e42-4f70-b81e-68b3446ade83)
![multi-insert-perf](https://github.com/user-attachments/assets/93bf3b54-c4e1-4d33-a14d-b73aa947a851)

## Accuracy
hyperloglockless stays accurate while other implementations break down after millions of items.

![err](https://github.com/user-attachments/assets/82690b1d-e9f0-4335-96c9-23914548ab65)


## Available Features

- **`rand`** - Enabled by default, this has the `DefaultHasher` source its random state using `thread_rng()` instead of hardware sources. Getting entropy from a user-space source is considerably faster, but requires additional dependencies to achieve this. Disabling this feature by using `default-features = false` makes `DefaultHasher` source its entropy using `getrandom`, which will have a much simpler code footprint at the expense of speed.

- **`serde`** - `HyperLogLog`s implement `Serialize` and `Deserialize` when possible.

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
