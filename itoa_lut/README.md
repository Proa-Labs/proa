# itoa_lut

Fast, allocation-free integer-to-ASCII formatting using a digit-pair lookup
table.

`itoa_lut` is a small `no_std` crate for writing primitive Rust integers as
decimal ASCII. Its primary API is a set of fixed-buffer writer functions that
let callers write directly into their own output storage. It also provides an
`itoa::Buffer`-style API for compatibility and convenience, but that is not
the main reason the crate exists.

## Why This Exists

The core use case is Proa's `WriteBuf` render path. Proa already owns the
final output buffer, and `WriteBuf::extend_with` lets a renderer reserve a
known maximum number of bytes, receive that space as `&mut [u8]`, and return
the number of bytes actually written.

`itoa_lut` is shaped specifically for that contract:

```rust
cx.out.extend_with(20, |buf| {
    itoa_lut::write_u64(value, buf.try_into().unwrap())
})?;
```

That path writes the integer directly into the final `WriteBuf` storage and
returns the actual byte length to `extend_with`. It does not need to create a
separate formatting buffer, build an `&str`, then copy that string into the
renderer's output.

This is why the direct writer signatures are fixed-size arrays such as
`write_u64(value, &mut [u8; 20]) -> usize`: the caller reserves the maximum
width for the type, the formatter fills the provided bytes, and `WriteBuf`
commits only the initialized prefix. Proa uses this in `WebRenderSync`,
`AttrValue`, and `MdRenderSync` implementations for primitive integers.

`itoa` is excellent and zero-allocation, but its public API is centered on a
formatter-owned buffer:

```rust
let mut tmp = itoa::Buffer::new();
cx.out.extend(tmp.format(value).as_bytes())?;
```

That is still fast, but it has a different shape: the number is formatted into
`itoa::Buffer` first, exposed as `&str`, and then copied into the caller's
output. `itoa_lut` exists so renderers like Proa can fill already-reserved
output bytes directly.

## Features

- `#![no_std]`
- No heap allocation
- Direct fixed-buffer writers for output-buffer callbacks like Proa's
  `WriteBuf::extend_with`
- Supports all primitive integer types through `Buffer::format`
- Exposes fixed-size write functions for exact buffer sizing
- Uses a 200-byte lookup table for digit pairs from `00` through `99`
- Keeps the public write API safe while using a narrowly scoped internal UTF-8
  conversion for `Buffer::format`

## Usage

```rust
let mut buf = itoa_lut::Buffer::new();

assert_eq!(buf.format(12345u32), "12345");
assert_eq!(buf.format(-42i64), "-42");
assert_eq!(
    buf.format(u128::MAX),
    "340282366920938463463374607431768211455",
);
```

The reusable buffer stores enough space for the largest supported value,
including `i128::MIN`.

## Direct Writers

For lower-level code, write into fixed-size stack buffers directly:

```rust
let mut bytes = [0u8; 20];
let len = itoa_lut::write_u64(18_446_744_073_709_551_615, &mut bytes);

assert_eq!(
    core::str::from_utf8(&bytes[..len]).unwrap(),
    "18446744073709551615",
);
```

Available direct writers:

| Function | Buffer type | Max bytes |
|---|---:|---:|
| `write_u8` | `&mut [u8; 3]` | 3 |
| `write_u16` | `&mut [u8; 5]` | 5 |
| `write_u32` | `&mut [u8; 10]` | 10 |
| `write_u64` | `&mut [u8; 20]` | 20 |
| `write_u128` | `&mut [u8; 39]` | 39 |
| `write_i8` | `&mut [u8; 4]` | 4 |
| `write_i16` | `&mut [u8; 6]` | 6 |
| `write_i32` | `&mut [u8; 11]` | 11 |
| `write_i64` | `&mut [u8; 21]` | 21 |
| `write_i128` | `&mut [u8; 40]` | 40 |

`Buffer::format` also supports `usize` and `isize`.

## How It Works

The core lookup table stores every two-digit ASCII pair from `00` through
`99`. Larger numbers are split into groups of 2, 4, or 8 digits, then written
with fixed-width tail groups so the output stays compact without temporary
strings.

The direct write functions take fixed-size arrays such as `&mut [u8; 20]` for
`u64`. That gives the compiler concrete bounds for the largest possible value
of each integer type and keeps the hot path simple.

Signed integers write a leading `-` when needed and then format the magnitude,
including the minimum value for each signed type.

## Testing And Benchmarks

Run the crate tests:

```bash
cargo test -p itoa_lut
```

Run the crate-local Criterion benchmarks:

```bash
cargo bench -p itoa_lut
```

The benchmark harness compares both `itoa_lut` paths with `itoa` and `itoap`
for fixed `u64` values and randomized workloads:

- `itoa_lut_write_u64`: direct fixed-buffer writer
- `itoa_lut_buffer`: `itoa::Buffer`-compatible API
- targeted fixed-value checks for `u8`, `i8`, `u32`, and `i32`

Run the benchmark on the target hardware and retain the complete Criterion
output with any performance claim. Repository documentation intentionally does
not preserve machine-specific point-in-time results.
