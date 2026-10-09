// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::signal;
use rsjs_macros::rsjs;

/// The tutorial's quantity stepper, rendered live so the docs page can frame it.
///
/// Everything here matches
/// [`/docs/guides/tutorial`](../../docs/guides/tutorial.mdx) line for line, so
/// the running demo and the copy-pasteable listing cannot drift apart.
pub(crate) struct QuantityStepper {
    pub(crate) unit_price: i64,
}

#[rsjs(component, client)]
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for QuantityStepper {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let qty = signal(1_i64);
        // The explicit `Signal<i64>` gives the multiply below a proven
        // fixed-width integer domain; without it rsjs cannot lower it.
        let unit_price: rsjs::Signal<i64> = signal(self.unit_price);
        let subtotal = rsjs::memo(|| qty.get() * unit_price.get());

        proa_macros::html_sync! {
            <div>
                <div class="flex items-center justify-between">
                    <span class="text-base font-medium text-zinc-950 dark:text-zinc-50">
                        "Quantity"
                    </span>
                    <span class="text-sm text-zinc-500 dark:text-zinc-400" aria-live="polite">
                        {text!("Subtotal ${}", subtotal.get())}
                    </span>
                </div>
                <div class="mt-3 flex items-center gap-2">
                    <button
                        type="button"
                        class="h-12 w-12 rounded-lg border border-zinc-300 text-lg transition hover:border-zinc-950 disabled:border-zinc-100 disabled:bg-zinc-100 disabled:text-zinc-400 dark:border-zinc-700 dark:text-zinc-100 dark:hover:border-zinc-300 dark:disabled:border-zinc-800 dark:disabled:bg-zinc-900 dark:disabled:text-zinc-600"
                        aria-label="Decrease quantity"
                        disabled={qty.get() == 1}
                        on_click={|_| qty.update(|q| q - 1)}
                    >
                        "-"
                    </button>
                    <span class="flex h-12 w-16 items-center justify-center rounded-lg border border-zinc-300 text-base font-medium text-zinc-950 dark:border-zinc-700 dark:text-zinc-50">
                        {qty.get()}
                    </span>
                    <button
                        type="button"
                        class="h-12 w-12 rounded-lg border border-zinc-300 text-lg transition hover:border-zinc-950 dark:border-zinc-700 dark:text-zinc-100 dark:hover:border-zinc-300"
                        aria-label="Increase quantity"
                        on_click={|_| qty.update(|q| q + 1)}
                    >
                        "+"
                    </button>
                </div>
                <input type="hidden" name="qty" value={qty.get()} />
            </div>
        }
        .render(cx)
    }
}
