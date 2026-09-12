// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Benchmark comparing old vs new Button rendering approaches
//!
//! Old: heapless::String class building + multiple UnsafeRaw calls
//! New: ClassList direct buffer writes

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use proa_bench::button_bench::{
    ButtonColor, ButtonRadius, ButtonSize, ButtonType, ButtonVariant, NewButton, OldButton,
};
use proa_core::{text::text, RenderBuffer, WebContext, WebRenderSync, WriteBuf};

const ITERATIONS: usize = 1000;

fn bench_button_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("button_render");

    // Benchmark old button approach
    group.bench_function(BenchmarkId::new("old_button", "default"), |b| {
        let mut buffer = RenderBuffer::new();
        b.iter(|| {
            for _ in 0..ITERATIONS {
                buffer.clear();
                let mut cx = WebContext::with_buffer(std::mem::take(&mut buffer));
                OldButton {
                    size: None,
                    color: None,
                    variant: None,
                    radius: None,
                    disabled: false,
                    button_type: ButtonType::Button,
                    class: None,
                    id: None,
                    children: Some(text("Click me")),
                }
                .render(&mut cx)
                .unwrap();
                buffer = cx.into_output();
                black_box(buffer.len());
            }
        });
    });

    // Benchmark new button approach
    group.bench_function(BenchmarkId::new("new_button", "default"), |b| {
        let mut buffer = RenderBuffer::new();
        b.iter(|| {
            for _ in 0..ITERATIONS {
                buffer.clear();
                let mut cx = WebContext::with_buffer(std::mem::take(&mut buffer));
                NewButton {
                    size: None,
                    color: None,
                    variant: None,
                    radius: None,
                    disabled: false,
                    button_type: ButtonType::Button,
                    class: None,
                    id: None,
                    children: Some(text("Click me")),
                }
                .render(&mut cx)
                .unwrap();
                buffer = cx.into_output();
                black_box(buffer.len());
            }
        });
    });

    // Benchmark with all variants specified
    group.bench_function(BenchmarkId::new("old_button", "all_variants"), |b| {
        let mut buffer = RenderBuffer::new();
        b.iter(|| {
            for _ in 0..ITERATIONS {
                buffer.clear();
                let mut cx = WebContext::with_buffer(std::mem::take(&mut buffer));
                OldButton {
                    size: Some(ButtonSize::Lg),
                    color: Some(ButtonColor::Destructive),
                    variant: Some(ButtonVariant::Outline),
                    radius: Some(ButtonRadius::Full),
                    disabled: true,
                    button_type: ButtonType::Submit,
                    class: Some("custom-class"),
                    id: Some("my-button"),
                    children: Some(text("Submit Form")),
                }
                .render(&mut cx)
                .unwrap();
                buffer = cx.into_output();
                black_box(buffer.len());
            }
        });
    });

    group.bench_function(BenchmarkId::new("new_button", "all_variants"), |b| {
        let mut buffer = RenderBuffer::new();
        b.iter(|| {
            for _ in 0..ITERATIONS {
                buffer.clear();
                let mut cx = WebContext::with_buffer(std::mem::take(&mut buffer));
                NewButton {
                    size: Some(ButtonSize::Lg),
                    color: Some(ButtonColor::Destructive),
                    variant: Some(ButtonVariant::Outline),
                    radius: Some(ButtonRadius::Full),
                    disabled: true,
                    button_type: ButtonType::Submit,
                    class: Some("custom-class"),
                    id: Some("my-button"),
                    children: Some(text("Submit Form")),
                }
                .render(&mut cx)
                .unwrap();
                buffer = cx.into_output();
                black_box(buffer.len());
            }
        });
    });

    group.finish();
}

fn bench_button_single(c: &mut Criterion) {
    let mut group = c.benchmark_group("button_single");

    // Single render - old
    group.bench_function("old_button", |b| {
        let mut buffer = RenderBuffer::new();
        b.iter(|| {
            buffer.clear();
            let mut cx = WebContext::with_buffer(std::mem::take(&mut buffer));
            OldButton {
                size: Some(ButtonSize::Md),
                color: Some(ButtonColor::Primary),
                variant: None,
                radius: None,
                disabled: false,
                button_type: ButtonType::Button,
                class: None,
                id: None,
                children: Some(text("Click")),
            }
            .render(&mut cx)
            .unwrap();
            buffer = cx.into_output();
            black_box(buffer.len())
        });
    });

    // Single render - new
    group.bench_function("new_button", |b| {
        let mut buffer = RenderBuffer::new();
        b.iter(|| {
            buffer.clear();
            let mut cx = WebContext::with_buffer(std::mem::take(&mut buffer));
            NewButton {
                size: Some(ButtonSize::Md),
                color: Some(ButtonColor::Primary),
                variant: None,
                radius: None,
                disabled: false,
                button_type: ButtonType::Button,
                class: None,
                id: None,
                children: Some(text("Click")),
            }
            .render(&mut cx)
            .unwrap();
            buffer = cx.into_output();
            black_box(buffer.len())
        });
    });

    group.finish();
}

/// Benchmark a page with many buttons (simulates real-world usage)
fn bench_button_page(c: &mut Criterion) {
    let mut group = c.benchmark_group("button_page");

    const BUTTONS_PER_PAGE: usize = 50;

    group.bench_function("old_buttons_x50", |b| {
        let mut buffer = RenderBuffer::new();
        b.iter(|| {
            buffer.clear();
            let mut cx = WebContext::with_buffer(std::mem::take(&mut buffer));

            for i in 0..BUTTONS_PER_PAGE {
                let size = match i % 4 {
                    0 => ButtonSize::Sm,
                    1 => ButtonSize::Md,
                    2 => ButtonSize::Lg,
                    _ => ButtonSize::Icon,
                };
                let color = match i % 3 {
                    0 => ButtonColor::Primary,
                    1 => ButtonColor::Secondary,
                    _ => ButtonColor::Destructive,
                };

                OldButton {
                    size: Some(size),
                    color: Some(color),
                    variant: None,
                    radius: None,
                    disabled: false,
                    button_type: ButtonType::Button,
                    class: None,
                    id: None,
                    children: Some(text("Button")),
                }
                .render(&mut cx)
                .unwrap();
            }
            buffer = cx.into_output();
            black_box(buffer.len())
        });
    });

    group.bench_function("new_buttons_x50", |b| {
        let mut buffer = RenderBuffer::new();
        b.iter(|| {
            buffer.clear();
            let mut cx = WebContext::with_buffer(std::mem::take(&mut buffer));

            for i in 0..BUTTONS_PER_PAGE {
                let size = match i % 4 {
                    0 => ButtonSize::Sm,
                    1 => ButtonSize::Md,
                    2 => ButtonSize::Lg,
                    _ => ButtonSize::Icon,
                };
                let color = match i % 3 {
                    0 => ButtonColor::Primary,
                    1 => ButtonColor::Secondary,
                    _ => ButtonColor::Destructive,
                };

                NewButton {
                    size: Some(size),
                    color: Some(color),
                    variant: None,
                    radius: None,
                    disabled: false,
                    button_type: ButtonType::Button,
                    class: None,
                    id: None,
                    children: Some(text("Button")),
                }
                .render(&mut cx)
                .unwrap();
            }
            buffer = cx.into_output();
            black_box(buffer.len())
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_button_comparison,
    bench_button_single,
    bench_button_page
);
criterion_main!(benches);
