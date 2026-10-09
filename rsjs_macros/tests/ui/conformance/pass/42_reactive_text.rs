#![allow(unused_imports)]
use proa_core::{DataLoader, WebContext, WebRenderSync, WriteError};
use proa_macros::{html_sync, text};
use rsjs::{rsjs, signal};

#[derive(Debug)]
pub struct ServerOnlyText {
    pub code: u32,
}

pub struct ReactiveTextSnapshot {
    pub initial_quantity: u32,
    pub initial_sampled: String,
    pub initial_flag: bool,
    pub price: f64,
    pub server_only: ServerOnlyText,
}

#[rsjs(component, client)]
impl<L: DataLoader> WebRenderSync<L> for ReactiveTextSnapshot {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let quantity: rsjs::Signal<u32> = signal(self.initial_quantity);
        let subtotal = quantity.get() * 105_u32;
        let label = signal("<tag & value>".to_string()); // proa-perf-ok: fixture tests encoded string state
        let flag: rsjs::Signal<bool> = signal(self.initial_quantity % 2_u32 == 1_u32);
        let other = signal("fallback".to_string()); // proa-perf-ok: fixture tests conditional owned/borrowed branches
        let zero = signal(0.0_f64);
        let one = signal(1.0_f64);
        let ratio = signal(0.0_f64);
        let sampled = signal(self.initial_sampled);
        let unknown_flag = signal(self.initial_flag);
        let active = text!("Active: {}", unknown_flag.get());
        html_sync! {
            <section>
                <button
                    type="button"
                    on_click={|_| {
                    quantity.update(|n| n + 1_u32);
                    label.set("<new & value>".to_string());
                    flag.update(|value| !value);
                    unknown_flag.set(flag.get());
                    ratio.set(if flag.get() { one.get() / zero.get() * -1.0_f64 } else { one.get() / zero.get() });
                }}
                >
                    "Add"
                </button>
                <p>{text!("Subtotal: ${}.{:02}", subtotal / 100_u32, subtotal % 100_u32)}</p>
                <p title={proa_macros::text!("Quantity {}", quantity.get())}>
                    "Before "
                    {::proa_macros::text!("{{{}}} {} {}", label.get(), quantity.get(), label.get())}
                    " After"
                </p>
                <p>
                    {text!("{} {}", if flag.get() { label.get() } else { other.get() }, label.get())}
                </p>
                <p>{text!("{} {:02} {:x} {:#X}", quantity.get(), 5_u32, -1_i32, -1_i64)}</p>
                <p>{text!("Ratio {}", ratio.get())}</p>
                <p title={text!("{:>16.8}", if flag.get() { label.get() } else { other.get().to_uppercase() })}>
                    {text!("{} {}", if flag.get() { label.get() } else { other.get().to_uppercase() }, label.get())}
                </p>
                <p>
                    {text!("{} {}", if flag.get() { sampled.get() } else { label.get() }, sampled.get())}
                </p>
                <p title={text!("Active: {}", flag.get())}>
                    {text!("{}", if flag.get() { "true" } else { "false" })}
                </p>
                <p title={active} data-state={active}>
                    {text!("Active: {}", if unknown_flag.get() { "true" } else { "false" })}
                </p>
                <p title={text!("Price {:.2}", self.price)}>{text!("{:.2}", self.price)}</p>
                <p>{text!("{:?}", self.server_only)}</p>
            </section>
        }
        .render(cx)
    }
}

fn main() {}
