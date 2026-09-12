// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

fn main() {
    print!(
        "{}",
        proa_showcase::sentry_otel::render_benchmark_rows(
            proa_showcase::sentry_otel::DEFAULT_ROW_COUNT,
        )
    );
}
