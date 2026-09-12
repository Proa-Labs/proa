// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! End-to-end TCP benchmark for contiguous, paged, and hybrid scatter/gather output.
//!
//! This intentionally uses only blocking `std::net::TcpStream` I/O. On Unix,
//! `write_vectored` maps to the platform's scatter/gather socket write (`writev`).
//!
//! Local loopback sweep:
//!
//! ```text
//! cargo run --release -p proa_core --example scatter_gather_tcp -- \
//!   local --suite all --warmup 100 --requests 1000 --rounds 3
//! ```
//!
//! Separate receiver and sender:
//!
//! ```text
//! cargo run --release -p proa_core --example scatter_gather_tcp -- \
//!   server --bind 0.0.0.0:39091
//! cargo run --release -p proa_core --example scatter_gather_tcp -- \
//!   client --connect 127.0.0.1:39091 --suite all
//! ```

use arrayvec::ArrayVec;
use proa_core::{RenderBuffer, WriteBuf};
use std::env;
use std::io::{self, ErrorKind, IoSlice, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

const MAGIC: &[u8; 8] = b"PROASG01";
const HEADER_LEN: usize = 32;
const MAX_PARTS: usize = 4096;
const CONTROL_LEN: usize = 8;
const DEFAULT_BIND: &str = "127.0.0.1:39091";

#[derive(Debug)]
enum WriteOp {
    Static(&'static [u8]),
    Dynamic(Vec<u8>),
}

#[derive(Debug)]
struct Workload {
    name: String,
    segment_size: Option<usize>,
    element_count: Option<usize>,
    ops: Vec<WriteOp>,
    total_len: usize,
}

#[derive(Debug, Clone, Copy)]
enum Strategy {
    Flat,
    Chained { max_iov: usize },
    Hybrid { threshold: usize, max_iov: usize },
}

#[derive(Debug, Clone, Copy)]
struct CaseSpec {
    workload: usize,
    strategy: Strategy,
}

#[derive(Debug)]
struct Plan {
    workloads: Vec<Workload>,
    cases: Vec<CaseSpec>,
}

#[derive(Debug, Clone, Copy)]
struct HybridGridOptions<'a> {
    static_sizes: &'a [usize],
    static_counts: &'a [usize],
    dynamic_size: usize,
}

#[derive(Debug, Default, Clone, Copy)]
struct PhaseStats {
    build: Duration,
    send: Duration,
    write_calls: u64,
}

#[derive(Debug, Clone, Copy)]
struct Shape {
    parts: usize,
    borrowed_bytes: usize,
}

#[derive(Debug)]
enum HybridPart {
    Static(&'static [u8]),
    Owned { start: usize, len: usize },
}

#[derive(Debug)]
struct HybridBuf {
    parts: Vec<HybridPart>,
    owned: Vec<u8>,
    total_len: usize,
    threshold: usize,
}

impl HybridBuf {
    fn with_capacity(part_capacity: usize, owned_capacity: usize, threshold: usize) -> Self {
        Self {
            parts: Vec::with_capacity(part_capacity),
            owned: Vec::with_capacity(owned_capacity),
            total_len: 0,
            threshold,
        }
    }

    #[inline]
    fn clear(&mut self) {
        self.parts.clear();
        self.owned.clear();
        self.total_len = 0;
    }

    #[inline]
    fn extend_owned(&mut self, bytes: &[u8]) {
        let start = self.owned.len();
        self.owned.extend_from_slice(bytes);
        if let Some(HybridPart::Owned {
            start: previous_start,
            len,
        }) = self.parts.last_mut()
        {
            debug_assert_eq!(*previous_start + *len, start);
            *len += bytes.len();
        } else {
            self.parts.push(HybridPart::Owned {
                start,
                len: bytes.len(),
            });
        }
        self.total_len += bytes.len();
    }

    #[inline]
    fn extend_static(&mut self, bytes: &'static [u8]) {
        if bytes.len() <= self.threshold {
            self.extend_owned(bytes);
        } else {
            self.parts.push(HybridPart::Static(bytes));
            self.total_len += bytes.len();
        }
    }

    fn build(&mut self, workload: &Workload) {
        self.clear();
        for op in &workload.ops {
            match op {
                WriteOp::Static(bytes) => self.extend_static(bytes),
                WriteOp::Dynamic(bytes) => self.extend_owned(bytes),
            }
        }
        debug_assert_eq!(self.total_len, workload.total_len);
    }

    fn push_io_slices<'a>(&'a self, slices: &mut ArrayVec<IoSlice<'a>, MAX_PARTS>) {
        for part in &self.parts {
            let bytes = match part {
                HybridPart::Static(bytes) => *bytes,
                HybridPart::Owned { start, len } => &self.owned[*start..*start + *len],
            };
            if !bytes.is_empty() {
                slices.push(IoSlice::new(bytes));
            }
        }
    }

    fn borrowed_bytes(&self) -> usize {
        self.parts
            .iter()
            .map(|part| match part {
                HybridPart::Static(bytes) => bytes.len(),
                HybridPart::Owned { .. } => 0,
            })
            .sum()
    }
}

fn usage() {
    eprintln!(
        "Usage:\n\
         \x20 scatter_gather_tcp local [options]\n\
         \x20 scatter_gather_tcp server [--bind ADDR] [--connections N]\n\
         \x20 scatter_gather_tcp client --connect ADDR [options]\n\n\
         Client/local options:\n\
         \x20 --suite proa|uniform|cornflakes|hybrid-grid|all  (default: all)\n\
         \x20 --warmup N                (default: 100)\n\
         \x20 --requests N              (default: 1000)\n\
         \x20 --rounds N                (default: 3)\n\
         \x20 --caps CSV                (default: 16,1024)\n\
         \x20 --thresholds CSV          (default: 0,64,128,256,512,1024,2048,4096,8192,inf)\n\
         \x20 --segment-sizes CSV       (default: 64,128,256,512,1024,2048,4096,8192,16384,32768)\n\
         \x20 --payload-sizes CSV       (default: 64,128,256,512,1024,2048,4096,8192)\n\
         \x20 --element-counts CSV      (default: 1,2,4,8,16,32)\n\
         \x20 --static-sizes CSV        (default: 8192,16384,32768,65536)\n\
         \x20 --static-counts CSV       (default: 1,2,4,8)\n\
         \x20 --dynamic-size N          (default: 32)"
    );
}

fn option<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].as_str())
}

fn parse_usize_option(args: &[String], name: &str, default: usize) -> io::Result<usize> {
    match option(args, name) {
        Some(value) => value.parse().map_err(|_| {
            io::Error::new(
                ErrorKind::InvalidInput,
                format!("invalid value for {name}: {value}"),
            )
        }),
        None => Ok(default),
    }
}

fn parse_csv(args: &[String], name: &str, default: &[usize]) -> io::Result<Vec<usize>> {
    let Some(value) = option(args, name) else {
        return Ok(default.to_vec());
    };
    value
        .split(',')
        .map(|item| {
            if item == "inf" {
                Ok(usize::MAX)
            } else {
                item.parse().map_err(|_| {
                    io::Error::new(
                        ErrorKind::InvalidInput,
                        format!("invalid value in {name}: {item}"),
                    )
                })
            }
        })
        .collect()
}

fn leak_static(size: usize, seed: usize) -> &'static [u8] {
    let pattern = b"<div class=\"flex items-center gap-4 rounded-xl border p-6\">proa</div>";
    let mut bytes = Vec::with_capacity(size);
    let mut cursor = seed % pattern.len();
    while bytes.len() < size {
        let remaining = size - bytes.len();
        let available = pattern.len() - cursor;
        let take = remaining.min(available);
        bytes.extend_from_slice(&pattern[cursor..cursor + take]);
        cursor = 0;
    }
    Box::leak(bytes.into_boxed_slice())
}

fn push_static(ops: &mut Vec<WriteOp>, total: &mut usize, size: usize, seed: usize) {
    let bytes = leak_static(size, seed);
    *total += bytes.len();
    ops.push(WriteOp::Static(bytes));
}

fn push_dynamic(ops: &mut Vec<WriteOp>, total: &mut usize, bytes: Vec<u8>) {
    *total += bytes.len();
    ops.push(WriteOp::Dynamic(bytes));
}

fn proa_workload() -> Workload {
    let mut ops = Vec::with_capacity(456);
    let mut total = 0;
    push_static(&mut ops, &mut total, 2048, 1);
    push_dynamic(
        &mut ops,
        &mut total,
        b"Luxury Beachfront Properties - Malibu, CA".to_vec(),
    );
    push_static(&mut ops, &mut total, 512, 2);
    push_dynamic(
        &mut ops,
        &mut total,
        b"243 stays available for your dates".to_vec(),
    );
    push_static(&mut ops, &mut total, 384, 3);

    let titles = [
        "Oceanfront Villa with Infinity Pool",
        "Modern Loft in Downtown Arts District",
        "Cozy Cabin in Big Bear Mountains",
        "Beachside Bungalow with Private Garden",
        "Penthouse Suite with City Views",
    ];
    for card in 0..50 {
        push_static(&mut ops, &mut total, 800, card * 5 + 4);
        push_dynamic(
            &mut ops,
            &mut total,
            format!("/images/listing-{card:04}.webp").into_bytes(),
        );
        push_static(&mut ops, &mut total, 480, card * 5 + 5);
        push_dynamic(
            &mut ops,
            &mut total,
            titles[card % titles.len()].as_bytes().to_vec(),
        );
        push_static(&mut ops, &mut total, 360, card * 5 + 6);
        push_dynamic(
            &mut ops,
            &mut total,
            format!("★ {}.{}", 4 + card % 2, card % 10).into_bytes(),
        );
        push_static(&mut ops, &mut total, 256, card * 5 + 7);
        push_dynamic(
            &mut ops,
            &mut total,
            format!("${}/night", 89 + card * 23).into_bytes(),
        );
        push_static(&mut ops, &mut total, 320, card * 5 + 8);
    }
    push_static(&mut ops, &mut total, 1536, 999);

    Workload {
        name: "proa_consolidated".to_string(),
        segment_size: None,
        element_count: None,
        ops,
        total_len: total,
    }
}

fn uniform_workload(segment_size: usize) -> Workload {
    const TARGET: usize = 128 * 1024;
    const DYNAMIC_SIZE: usize = 32;

    let mut ops = Vec::new();
    let mut total = 0;
    let mut seed = 0;
    while total < TARGET {
        let remaining = TARGET - total;
        if remaining <= DYNAMIC_SIZE {
            push_dynamic(&mut ops, &mut total, vec![b'd'; remaining]);
            break;
        }
        let static_size = segment_size.min(remaining - DYNAMIC_SIZE);
        push_static(&mut ops, &mut total, static_size, seed);
        push_dynamic(&mut ops, &mut total, vec![b'd'; DYNAMIC_SIZE]);
        seed += 1;
    }

    Workload {
        name: format!("uniform_{segment_size}B"),
        segment_size: Some(segment_size),
        element_count: None,
        ops,
        total_len: total,
    }
}

/// Reproduce one cell from Cornflakes Figure 5: a total response payload
/// divided into an exact number of equal-sized, independently gathered fields.
fn cornflakes_workload(payload_size: usize, element_count: usize) -> io::Result<Workload> {
    if payload_size == 0 || element_count == 0 || !payload_size.is_multiple_of(element_count) {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "Cornflakes matrix requires a positive payload divisible by element count; \
                 got payload={payload_size}, elements={element_count}"
            ),
        ));
    }
    let field_size = payload_size / element_count;
    let mut ops = Vec::with_capacity(element_count);
    let mut total = 0;
    for seed in 0..element_count {
        push_static(&mut ops, &mut total, field_size, seed);
    }
    debug_assert_eq!(total, payload_size);
    Ok(Workload {
        name: format!("cornflakes_{payload_size}B_{element_count}e"),
        segment_size: Some(field_size),
        element_count: Some(element_count),
        ops,
        total_len: total,
    })
}

/// Model an SSR response that alternates large compile-time static runs with
/// small runtime values. Every static run can remain borrowed while each
/// dynamic value is copied into the hybrid buffer's owned storage.
fn hybrid_grid_workload(
    static_size: usize,
    static_count: usize,
    dynamic_size: usize,
) -> io::Result<Workload> {
    if static_size == 0 || static_count == 0 || dynamic_size == 0 {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "hybrid grid requires positive sizes and count; got static_size={static_size}, \
                 static_count={static_count}, dynamic_size={dynamic_size}"
            ),
        ));
    }

    let mut ops = Vec::with_capacity(static_count * 2);
    let mut total = 0;
    for seed in 0..static_count {
        push_static(&mut ops, &mut total, static_size, seed);
        push_dynamic(&mut ops, &mut total, vec![b'd'; dynamic_size]);
    }

    Ok(Workload {
        name: format!("hybridgrid_{static_size}B_{static_count}s_{dynamic_size}B"),
        segment_size: Some(static_size),
        element_count: Some(static_count),
        ops,
        total_len: total,
    })
}

fn build_plan(
    suite: &str,
    caps: &[usize],
    thresholds: &[usize],
    segment_sizes: &[usize],
    payload_sizes: &[usize],
    element_counts: &[usize],
    hybrid_grid: HybridGridOptions<'_>,
) -> io::Result<Plan> {
    if caps.iter().any(|cap| *cap == 0 || *cap > 1024) {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "every --caps value must be in 1..=1024",
        ));
    }
    let mut workloads = Vec::new();
    let mut cases = Vec::new();

    if suite == "proa" || suite == "all" {
        let workload = workloads.len();
        workloads.push(proa_workload());
        cases.push(CaseSpec {
            workload,
            strategy: Strategy::Flat,
        });
        for &max_iov in caps {
            cases.push(CaseSpec {
                workload,
                strategy: Strategy::Chained { max_iov },
            });
            for &threshold in thresholds {
                cases.push(CaseSpec {
                    workload,
                    strategy: Strategy::Hybrid { threshold, max_iov },
                });
            }
        }
    }

    if suite == "uniform" || suite == "all" {
        for &segment_size in segment_sizes {
            if segment_size == 0 {
                return Err(io::Error::new(
                    ErrorKind::InvalidInput,
                    "--segment-sizes values must be positive",
                ));
            }
            let workload = workloads.len();
            workloads.push(uniform_workload(segment_size));
            cases.push(CaseSpec {
                workload,
                strategy: Strategy::Flat,
            });
            for &max_iov in caps {
                cases.push(CaseSpec {
                    workload,
                    strategy: Strategy::Hybrid {
                        threshold: 0,
                        max_iov,
                    },
                });
            }
        }
    }

    if suite == "cornflakes" || suite == "all" {
        for &element_count in element_counts {
            for &payload_size in payload_sizes {
                let workload = workloads.len();
                workloads.push(cornflakes_workload(payload_size, element_count)?);
                cases.push(CaseSpec {
                    workload,
                    strategy: Strategy::Flat,
                });
                for &max_iov in caps {
                    cases.push(CaseSpec {
                        workload,
                        strategy: Strategy::Hybrid {
                            threshold: 0,
                            max_iov,
                        },
                    });
                }
            }
        }
    }

    if suite == "hybrid-grid" || suite == "all" {
        for &static_count in hybrid_grid.static_counts {
            for &static_size in hybrid_grid.static_sizes {
                let workload = workloads.len();
                workloads.push(hybrid_grid_workload(
                    static_size,
                    static_count,
                    hybrid_grid.dynamic_size,
                )?);
                cases.push(CaseSpec {
                    workload,
                    strategy: Strategy::Flat,
                });
                for &max_iov in caps {
                    cases.push(CaseSpec {
                        workload,
                        strategy: Strategy::Hybrid {
                            threshold: 0,
                            max_iov,
                        },
                    });
                }
            }
        }
    }

    if cases.is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("unknown --suite value: {suite}"),
        ));
    }
    Ok(Plan { workloads, cases })
}

fn render_flat(workload: &Workload, output: &mut Vec<u8>) {
    output.clear();
    for op in &workload.ops {
        match op {
            WriteOp::Static(bytes) => output.extend_from_slice(bytes),
            WriteOp::Dynamic(bytes) => output.extend_from_slice(bytes),
        }
    }
    debug_assert_eq!(output.len(), workload.total_len);
}

fn render_chained(workload: &Workload, output: &mut RenderBuffer) {
    output.clear();
    for op in &workload.ops {
        match op {
            WriteOp::Static(bytes) => output.extend_static(bytes).unwrap(),
            WriteOp::Dynamic(bytes) => output.extend(bytes).unwrap(),
        }
    }
    debug_assert_eq!(output.len(), workload.total_len);
}

fn write_all_counted(stream: &mut TcpStream, mut bytes: &[u8]) -> io::Result<u64> {
    let mut calls = 0;
    while !bytes.is_empty() {
        let written = stream.write(bytes)?;
        calls += 1;
        if written == 0 {
            return Err(io::Error::new(
                ErrorKind::WriteZero,
                "socket write returned zero",
            ));
        }
        bytes = &bytes[written..];
    }
    Ok(calls)
}

fn write_vectored_all(
    stream: &mut TcpStream,
    mut slices: &mut [IoSlice<'_>],
    max_iov: usize,
) -> io::Result<u64> {
    let mut calls = 0;
    while !slices.is_empty() {
        let count = slices.len().min(max_iov);
        let written = stream.write_vectored(&slices[..count])?;
        calls += 1;
        if written == 0 {
            return Err(io::Error::new(
                ErrorKind::WriteZero,
                "socket write_vectored returned zero",
            ));
        }
        IoSlice::advance_slices(&mut slices, written);
    }
    Ok(calls)
}

fn flat_phase(
    stream: &mut TcpStream,
    workload: &Workload,
    output: &mut Vec<u8>,
    requests: usize,
) -> io::Result<PhaseStats> {
    let mut stats = PhaseStats::default();
    for _ in 0..requests {
        let started = Instant::now();
        render_flat(workload, output);
        stats.build += started.elapsed();

        let started = Instant::now();
        stats.write_calls += write_all_counted(stream, output)?;
        stats.send += started.elapsed();
    }
    Ok(stats)
}

fn chained_phase(
    stream: &mut TcpStream,
    workload: &Workload,
    output: &mut RenderBuffer,
    requests: usize,
    max_iov: usize,
) -> io::Result<PhaseStats> {
    let mut stats = PhaseStats::default();
    for _ in 0..requests {
        let started = Instant::now();
        render_chained(workload, output);
        stats.build += started.elapsed();

        let started = Instant::now();
        let mut slices = ArrayVec::<IoSlice<'_>, MAX_PARTS>::new();
        for bytes in output.slices().filter(|bytes| !bytes.is_empty()) {
            slices.push(IoSlice::new(bytes));
        }
        stats.write_calls += write_vectored_all(stream, slices.as_mut_slice(), max_iov)?;
        stats.send += started.elapsed();
    }
    Ok(stats)
}

fn hybrid_phase(
    stream: &mut TcpStream,
    workload: &Workload,
    output: &mut HybridBuf,
    requests: usize,
    max_iov: usize,
) -> io::Result<PhaseStats> {
    let mut stats = PhaseStats::default();
    for _ in 0..requests {
        let started = Instant::now();
        output.build(workload);
        stats.build += started.elapsed();

        let started = Instant::now();
        let mut slices = ArrayVec::<IoSlice<'_>, MAX_PARTS>::new();
        output.push_io_slices(&mut slices);
        stats.write_calls += write_vectored_all(stream, slices.as_mut_slice(), max_iov)?;
        stats.send += started.elapsed();
    }
    Ok(stats)
}

fn header(payload_len: usize, warmup: usize, requests: usize) -> [u8; HEADER_LEN] {
    let mut bytes = [0; HEADER_LEN];
    bytes[..8].copy_from_slice(MAGIC);
    bytes[8..16].copy_from_slice(&(payload_len as u64).to_be_bytes());
    bytes[16..24].copy_from_slice(&(warmup as u64).to_be_bytes());
    bytes[24..32].copy_from_slice(&(requests as u64).to_be_bytes());
    bytes
}

fn read_control(stream: &mut TcpStream, expected: u64) -> io::Result<()> {
    let mut bytes = [0; CONTROL_LEN];
    stream.read_exact(&mut bytes)?;
    let actual = u64::from_be_bytes(bytes);
    if actual != expected {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!("receiver reported {actual} bytes; expected {expected}"),
        ));
    }
    Ok(())
}

fn case_shape(workload: &Workload, strategy: Strategy) -> Shape {
    match strategy {
        Strategy::Flat => Shape {
            parts: 1,
            borrowed_bytes: 0,
        },
        Strategy::Chained { .. } => {
            let mut output = RenderBuffer::new();
            render_chained(workload, &mut output);
            Shape {
                parts: output.slices().filter(|bytes| !bytes.is_empty()).count(),
                borrowed_bytes: 0,
            }
        }
        Strategy::Hybrid { threshold, .. } => {
            let mut output =
                HybridBuf::with_capacity(workload.ops.len(), workload.total_len, threshold);
            output.build(workload);
            Shape {
                parts: output.parts.len(),
                borrowed_bytes: output.borrowed_bytes(),
            }
        }
    }
}

fn run_case(
    connect: SocketAddr,
    workload: &Workload,
    strategy: Strategy,
    warmup: usize,
    requests: usize,
) -> io::Result<(PhaseStats, Duration)> {
    let mut stream = TcpStream::connect(connect)?;
    stream.set_nodelay(true)?;
    stream.write_all(&header(workload.total_len, warmup, requests))?;

    let stats;
    let wall;
    match strategy {
        Strategy::Flat => {
            let mut output = Vec::with_capacity(workload.total_len);
            flat_phase(&mut stream, workload, &mut output, warmup)?;
            read_control(&mut stream, (workload.total_len * warmup) as u64)?;
            let started = Instant::now();
            stats = flat_phase(&mut stream, workload, &mut output, requests)?;
            read_control(&mut stream, (workload.total_len * requests) as u64)?;
            wall = started.elapsed();
        }
        Strategy::Chained { max_iov } => {
            let mut output = RenderBuffer::new();
            chained_phase(&mut stream, workload, &mut output, warmup, max_iov)?;
            read_control(&mut stream, (workload.total_len * warmup) as u64)?;
            let started = Instant::now();
            stats = chained_phase(&mut stream, workload, &mut output, requests, max_iov)?;
            read_control(&mut stream, (workload.total_len * requests) as u64)?;
            wall = started.elapsed();
        }
        Strategy::Hybrid { threshold, max_iov } => {
            let mut output =
                HybridBuf::with_capacity(workload.ops.len(), workload.total_len, threshold);
            hybrid_phase(&mut stream, workload, &mut output, warmup, max_iov)?;
            read_control(&mut stream, (workload.total_len * warmup) as u64)?;
            let started = Instant::now();
            stats = hybrid_phase(&mut stream, workload, &mut output, requests, max_iov)?;
            read_control(&mut stream, (workload.total_len * requests) as u64)?;
            wall = started.elapsed();
        }
    }
    Ok((stats, wall))
}

fn strategy_fields(strategy: Strategy) -> (&'static str, String, usize) {
    match strategy {
        Strategy::Flat => ("flat", "na".to_string(), 1),
        Strategy::Chained { max_iov } => ("chained", "na".to_string(), max_iov),
        Strategy::Hybrid { threshold, max_iov } => (
            "hybrid",
            if threshold == usize::MAX {
                "inf".to_string()
            } else {
                threshold.to_string()
            },
            max_iov,
        ),
    }
}

fn print_result(
    round: usize,
    workload: &Workload,
    strategy: Strategy,
    shape: Shape,
    requests: usize,
    stats: PhaseStats,
    wall: Duration,
) {
    let (strategy_name, threshold, max_iov) = strategy_fields(strategy);
    let bytes = workload.total_len as f64 * requests as f64;
    let req_per_s = requests as f64 / wall.as_secs_f64();
    let gb_per_s = bytes / wall.as_secs_f64() / 1e9;
    let build_us = stats.build.as_secs_f64() * 1e6 / requests as f64;
    let send_us = stats.send.as_secs_f64() * 1e6 / requests as f64;
    let calls = stats.write_calls as f64 / requests as f64;
    let borrowed_pct = 100.0 * shape.borrowed_bytes as f64 / workload.total_len as f64;
    let segment_size = workload
        .segment_size
        .map_or_else(|| "na".to_string(), |size| size.to_string());
    let element_count = workload
        .element_count
        .map_or_else(|| "na".to_string(), |count| count.to_string());

    println!(
        "round={round} workload={} strategy={strategy_name} threshold={threshold} cap={max_iov} \
         elements={element_count} parts={} borrowed={borrowed_pct:.1}% build={build_us:.3}us send={send_us:.3}us \
         calls={calls:.2} req/s={req_per_s:.0} GB/s={gb_per_s:.3}",
        workload.name, shape.parts
    );
    println!(
        "RESULT,{round},{},{segment_size},{element_count},{strategy_name},{threshold},{max_iov},{},{},{},{borrowed_pct:.3},{requests},{:.9},{req_per_s:.3},{gb_per_s:.6},{build_us:.6},{send_us:.6},{calls:.6}",
        workload.name,
        workload.total_len,
        workload.ops.len(),
        shape.parts,
        wall.as_secs_f64(),
    );
}

fn run_client(
    connect: SocketAddr,
    plan: &Plan,
    warmup: usize,
    requests: usize,
    rounds: usize,
) -> io::Result<()> {
    println!(
        "RESULT_HEADER,round,workload,segment_size,element_count,strategy,threshold,max_iov,payload_bytes,ops,parts,borrowed_pct,requests,wall_s,req_per_s,GB_per_s,build_us_per_req,send_us_per_req,write_calls_per_req"
    );
    for round in 1..=rounds {
        let mut order: Vec<usize> = (0..plan.cases.len()).collect();
        // Deterministically shuffle every round so thermal state, scheduler
        // noise, and the first connection cannot consistently favor one case.
        let mut state = (round as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        for index in (1..order.len()).rev() {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            order.swap(index, (state as usize) % (index + 1));
        }
        for index in order {
            let case = plan.cases[index];
            let workload = &plan.workloads[case.workload];
            let shape = case_shape(workload, case.strategy);
            if shape.parts > MAX_PARTS {
                return Err(io::Error::new(
                    ErrorKind::InvalidInput,
                    format!(
                        "{} produced {} parts, exceeding MAX_PARTS={MAX_PARTS}",
                        workload.name, shape.parts
                    ),
                ));
            }
            let (stats, wall) = run_case(connect, workload, case.strategy, warmup, requests)?;
            print_result(round, workload, case.strategy, shape, requests, stats, wall);
        }
    }
    Ok(())
}

fn parse_header(bytes: &[u8; HEADER_LEN]) -> io::Result<(u64, u64, u64)> {
    if &bytes[..8] != MAGIC {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "invalid protocol magic",
        ));
    }
    let payload = u64::from_be_bytes(bytes[8..16].try_into().unwrap());
    let warmup = u64::from_be_bytes(bytes[16..24].try_into().unwrap());
    let requests = u64::from_be_bytes(bytes[24..32].try_into().unwrap());
    Ok((payload, warmup, requests))
}

fn drain_bytes(stream: &mut TcpStream, mut remaining: u64) -> io::Result<u64> {
    let expected = remaining;
    let mut buffer = [0_u8; 256 * 1024];
    while remaining > 0 {
        let wanted = remaining.min(buffer.len() as u64) as usize;
        let read = stream.read(&mut buffer[..wanted])?;
        if read == 0 {
            return Err(io::Error::new(
                ErrorKind::UnexpectedEof,
                format!("connection ended with {remaining} bytes remaining"),
            ));
        }
        remaining -= read as u64;
    }
    Ok(expected)
}

fn handle_connection(mut stream: TcpStream) -> io::Result<()> {
    stream.set_nodelay(true)?;
    let mut header = [0; HEADER_LEN];
    stream.read_exact(&mut header)?;
    let (payload, warmup, requests) = parse_header(&header)?;

    let warmup_bytes = payload
        .checked_mul(warmup)
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "warmup byte count overflow"))?;
    let measured_bytes = payload
        .checked_mul(requests)
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "measured byte count overflow"))?;
    let received = drain_bytes(&mut stream, warmup_bytes)?;
    stream.write_all(&received.to_be_bytes())?;
    let received = drain_bytes(&mut stream, measured_bytes)?;
    stream.write_all(&received.to_be_bytes())?;
    Ok(())
}

fn serve(listener: TcpListener, connections: Option<usize>) -> io::Result<()> {
    eprintln!("receiver listening on {}", listener.local_addr()?);
    let mut accepted = 0;
    loop {
        let (stream, peer) = listener.accept()?;
        if let Err(error) = handle_connection(stream) {
            eprintln!("receiver error from {peer}: {error}");
            return Err(error);
        }
        accepted += 1;
        if connections.is_some_and(|limit| accepted >= limit) {
            return Ok(());
        }
    }
}

fn client_options(args: &[String]) -> io::Result<(Plan, usize, usize, usize)> {
    let suite = option(args, "--suite").unwrap_or("all");
    let warmup = parse_usize_option(args, "--warmup", 100)?;
    let requests = parse_usize_option(args, "--requests", 1000)?;
    let rounds = parse_usize_option(args, "--rounds", 3)?;
    if requests == 0 || rounds == 0 {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "--requests and --rounds must be positive",
        ));
    }
    let caps = parse_csv(args, "--caps", &[16, 1024])?;
    let thresholds = parse_csv(
        args,
        "--thresholds",
        &[0, 64, 128, 256, 512, 1024, 2048, 4096, 8192, usize::MAX],
    )?;
    let segment_sizes = parse_csv(
        args,
        "--segment-sizes",
        &[64, 128, 256, 512, 1024, 2048, 4096, 8192, 16384, 32768],
    )?;
    let payload_sizes = parse_csv(
        args,
        "--payload-sizes",
        &[64, 128, 256, 512, 1024, 2048, 4096, 8192],
    )?;
    let element_counts = parse_csv(args, "--element-counts", &[1, 2, 4, 8, 16, 32])?;
    let static_sizes = parse_csv(args, "--static-sizes", &[8192, 16384, 32768, 65536])?;
    let static_counts = parse_csv(args, "--static-counts", &[1, 2, 4, 8])?;
    let dynamic_size = parse_usize_option(args, "--dynamic-size", 32)?;
    let plan = build_plan(
        suite,
        &caps,
        &thresholds,
        &segment_sizes,
        &payload_sizes,
        &element_counts,
        HybridGridOptions {
            static_sizes: &static_sizes,
            static_counts: &static_counts,
            dynamic_size,
        },
    )?;
    Ok((plan, warmup, requests, rounds))
}

fn main() -> io::Result<()> {
    let args: Vec<String> = env::args().collect();
    let Some(command) = args.get(1).map(String::as_str) else {
        usage();
        return Err(io::Error::new(ErrorKind::InvalidInput, "missing command"));
    };

    match command {
        "server" => {
            let bind = option(&args, "--bind").unwrap_or(DEFAULT_BIND);
            let connections = option(&args, "--connections")
                .map(|value| value.parse::<usize>())
                .transpose()
                .map_err(|_| io::Error::new(ErrorKind::InvalidInput, "invalid --connections"))?;
            serve(TcpListener::bind(bind)?, connections)
        }
        "client" => {
            let connect = option(&args, "--connect").ok_or_else(|| {
                io::Error::new(ErrorKind::InvalidInput, "client requires --connect ADDR")
            })?;
            let connect: SocketAddr = connect.parse().map_err(|_| {
                io::Error::new(ErrorKind::InvalidInput, "invalid --connect address")
            })?;
            let (plan, warmup, requests, rounds) = client_options(&args)?;
            run_client(connect, &plan, warmup, requests, rounds)
        }
        "local" => {
            let (plan, warmup, requests, rounds) = client_options(&args)?;
            let listener = TcpListener::bind("127.0.0.1:0")?;
            let connect = listener.local_addr()?;
            let connections = plan.cases.len() * rounds;
            let receiver = thread::spawn(move || serve(listener, Some(connections)));
            let result = run_client(connect, &plan, warmup, requests, rounds);
            let receiver_result = receiver
                .join()
                .map_err(|_| io::Error::other("receiver panicked"))?;
            result.and(receiver_result)
        }
        _ => {
            usage();
            Err(io::Error::new(
                ErrorKind::InvalidInput,
                format!("unknown command: {command}"),
            ))
        }
    }
}
