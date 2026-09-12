// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Compare hot-cache and streaming `copy_from_slice` throughput.

use std::hint::black_box;
use std::time::Instant;

const SIZES: &[usize] = &[
    4 * 1024,
    8 * 1024,
    16 * 1024,
    32 * 1024,
    64 * 1024,
    128 * 1024,
    256 * 1024,
    512 * 1024,
    1024 * 1024,
    2 * 1024 * 1024,
    4 * 1024 * 1024,
];
const HOT_TARGET_BYTES: usize = 4 * 1024 * 1024 * 1024;
const STREAM_TARGET_BYTES: usize = 2 * 1024 * 1024 * 1024;
const STREAM_WORKING_SET: usize = 128 * 1024 * 1024;

fn fill_source(len: usize) -> Vec<u8> {
    (0..len).map(|index| index.wrapping_mul(31) as u8).collect()
}

fn hot_copy(size: usize) -> f64 {
    let source = fill_source(size);
    let mut destination = vec![0_u8; size];
    let iterations = (HOT_TARGET_BYTES / size).max(16);

    for _ in 0..128 {
        black_box(destination.as_mut_slice()).copy_from_slice(black_box(source.as_slice()));
    }

    let started = Instant::now();
    for _ in 0..iterations {
        black_box(destination.as_mut_slice()).copy_from_slice(black_box(source.as_slice()));
    }
    let elapsed = started.elapsed();
    black_box(destination);
    size as f64 * iterations as f64 / elapsed.as_secs_f64() / 1e9
}

fn streaming_copy(size: usize) -> f64 {
    let source = fill_source(STREAM_WORKING_SET);
    let mut destination = vec![0_u8; STREAM_WORKING_SET];
    let chunks = STREAM_WORKING_SET / size;
    let iterations = (STREAM_TARGET_BYTES / size).max(chunks);

    let started = Instant::now();
    for iteration in 0..iterations {
        let offset = (iteration % chunks) * size;
        black_box(&mut destination[offset..offset + size])
            .copy_from_slice(black_box(&source[offset..offset + size]));
    }
    let elapsed = started.elapsed();
    black_box(destination);
    size as f64 * iterations as f64 / elapsed.as_secs_f64() / 1e9
}

fn fragmented_render_copy() -> f64 {
    const STATIC_SIZE: usize = 32 * 1024;
    const DYNAMIC_SIZE: usize = 32;
    const TOTAL_SIZE: usize = 2 * (STATIC_SIZE + DYNAMIC_SIZE);

    let first = fill_source(STATIC_SIZE);
    let second = fill_source(STATIC_SIZE);
    let dynamic = [b'd'; DYNAMIC_SIZE];
    let mut destination = Vec::with_capacity(TOTAL_SIZE);
    let iterations = HOT_TARGET_BYTES / TOTAL_SIZE;

    let started = Instant::now();
    for _ in 0..iterations {
        destination.clear();
        destination.extend_from_slice(black_box(first.as_slice()));
        destination.extend_from_slice(black_box(&dynamic));
        destination.extend_from_slice(black_box(second.as_slice()));
        destination.extend_from_slice(black_box(&dynamic));
        black_box(destination.as_slice());
    }
    let elapsed = started.elapsed();
    black_box(destination);
    TOTAL_SIZE as f64 * iterations as f64 / elapsed.as_secs_f64() / 1e9
}

fn main() {
    println!("size_bytes,hot_GB_per_s,streaming_GB_per_s");
    for &size in SIZES {
        println!("{size},{:.3},{:.3}", hot_copy(size), streaming_copy(size));
    }
    println!("fragmented_2x32KiB,{},na", fragmented_render_copy());
}
