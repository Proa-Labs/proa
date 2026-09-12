// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use proa_core::{
    ctx::Ctx, el, text, Attr, AttrKey, Render, RenderBuffer, Tag, WriteBuf, WriteError,
};

// A simple page with nested elements
#[derive(Clone, Copy)]
struct SimplePage;

impl Render for SimplePage {
    fn render(self, cx: &mut Ctx) -> Result<(), WriteError> {
        el(
            Tag::Div,
            [Attr::Str {
                k: AttrKey::Class,
                v: "container",
            }],
            |cx| {
                for _ in 0..5 {
                    el(Tag::P, [], |cx| text("Hello World").render(cx)).render(cx)?;
                }
                Ok(())
            },
        )
        .render(cx)
    }
}

fn bench_current_implementation(c: &mut Criterion) {
    c.bench_function("current_no_inline", |b| {
        let mut buffer = RenderBuffer::new();
        b.iter(|| {
            buffer.clear();
            let mut cx = Ctx::with_buffer(std::mem::take(&mut buffer));
            SimplePage.render(&mut cx).unwrap();
            buffer = cx.into_output();
            black_box(buffer.len());
        });
    });
}

criterion_group!(benches, bench_current_implementation);
criterion_main!(benches);
