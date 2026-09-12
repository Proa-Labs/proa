// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use proa_core::{NoLoader, WebContext, WebRenderSync};
use proa_macros::text;

fn render_once<T>(value: T, output: &mut Vec<u8>)
where
    T: WebRenderSync<NoLoader, Vec<u8>>,
{
    output.clear();
    let mut cx = WebContext::with_buffer(core::mem::take(output));
    value.render(&mut cx).unwrap();
    *output = cx.into_output();
    black_box(output.as_slice());
}

fn render_generic_fixed<T>(value: T, cx: &mut WebContext<NoLoader, Vec<u8>>)
where
    T: core::fmt::Display,
{
    text!("{:.2}", value).render(cx).unwrap();
}

#[inline]
fn render_std_zero_pad_2(value: u32, cx: &mut WebContext<NoLoader, Vec<u8>>) {
    proa_core::shared::text::__proa_render_fmt_text(format_args!("{:02}", value), cx).unwrap();
}

fn numeric_format(c: &mut Criterion) {
    let mut group = c.benchmark_group("numeric_format");

    group.bench_function("f64_shortest", |b| {
        let mut output = Vec::with_capacity(64);
        b.iter(|| render_once(black_box(12.345_f64), &mut output));
    });

    group.bench_function("f64_fixed_2", |b| {
        let mut output = Vec::with_capacity(64);
        b.iter(|| render_once(text!("{:.2}", black_box(12.345_f64)), &mut output));
    });

    group.bench_function("f64_fixed_2_hot", |b| {
        let mut cx = WebContext::with_buffer(Vec::with_capacity(64));
        b.iter(|| {
            cx.out.clear();
            text!("{:.2}", black_box(12.345_f64))
                .render(&mut cx)
                .unwrap();
            black_box(cx.out.as_slice());
        });
    });

    group.bench_function("f64_fixed_2_generic_fmt", |b| {
        let mut cx = WebContext::with_buffer(Vec::with_capacity(64));
        b.iter(|| {
            cx.out.clear();
            render_generic_fixed(black_box(12.345_f64), &mut cx);
            black_box(cx.out.as_slice());
        });
    });

    group.bench_function("u32_zero_pad_2_hot", |b| {
        let mut cx = WebContext::with_buffer(Vec::with_capacity(64));
        b.iter(|| {
            cx.out.clear();
            text!("{:02}", black_box(7u32)).render(&mut cx).unwrap();
            black_box(cx.out.as_slice());
        });
    });

    group.bench_function("u32_zero_pad_2_std_fmt_hot", |b| {
        let mut cx = WebContext::with_buffer(Vec::with_capacity(64));
        b.iter(|| {
            cx.out.clear();
            render_std_zero_pad_2(black_box(7u32), &mut cx);
            black_box(cx.out.as_slice());
        });
    });

    group.bench_function("u32_zero_pad_3_hot", |b| {
        let mut cx = WebContext::with_buffer(Vec::with_capacity(64));
        b.iter(|| {
            cx.out.clear();
            text!("{:03}", black_box(42u32)).render(&mut cx).unwrap();
            black_box(cx.out.as_slice());
        });
    });

    group.bench_function("i64_formatted_matrix_hot", |b| {
        let mut cx = WebContext::with_buffer(Vec::with_capacity(128));
        b.iter(|| {
            cx.out.clear();
            text!(
                "{:+016} {:#018x} {:.2e}",
                black_box(-1_234_567_890i64),
                black_box(-1_234_567_890i64),
                black_box(-1_234_567_890i64),
            )
            .render(&mut cx)
            .unwrap();
            black_box(cx.out.as_slice());
        });
    });

    group.finish();
}

criterion_group!(benches, numeric_format);
criterion_main!(benches);
