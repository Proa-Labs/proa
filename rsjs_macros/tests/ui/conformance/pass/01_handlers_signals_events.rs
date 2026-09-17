#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct HandlerSignalsEvents;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for HandlerSignalsEvents {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        let text = signal(String::new());
        let enabled = signal(true);
        let shifted = signal(false);
        let last_key = signal(String::new());
        let touch_x = signal(0.0_f64);
        let dropped_file = signal(String::new());
        let picked_file = signal(String::new());

        proa_macros::html_sync! {
            <form on_submit={|event| {
                event.prevent_default();
                event.stop_propagation();

                let value = event.value();
                console::log(value);
                text.set(value);
                shifted.set(event.shift_key());
                last_key.set(event.key());

                if enabled.get() && event.meta_key() {
                    count.update(|n| n + 1);
                } else if event.ctrl_key() {
                    count.set(0_i64);
                } else {
                    count.update(|n| n - 1);
                }
            }}>
                <input value={text.get()} />
                <button
                    type="button"
                    on_touchstart={|event: rsjs::TouchEvent| {
                    touch_x.set(event.changed_touch_client_x());
                    console::log(
                        event.changed_touch_count(),
                        event.changed_touch_client_x_at(0),
                        event.target_touch_count(),
                        event.target_touch_client_y_at(0),
                        event.touch_count(),
                        event.touch_identifier_at(0),
                    );
                }}
                >
                    "Touch"
                </button>
                <input
                    type="file"
                    on_change={|event: rsjs::ChangeEvent| {
                    picked_file.set(event.target_first_file_name());
                    console::log(
                        event.target_file_count(),
                        event.target_first_file_size(),
                        event.target_file_name_at(0),
                        event.target_file_type_at(0),
                        event.target_file_size_at(0),
                    );
                }}
                />
                <div
                    on_dragover={|event: rsjs::DragEvent| {
                        event.prevent_default();
                        console::log(event.data_transfer_text());
                    }}
                    on_drop={|event: rsjs::DragEvent| {
                        event.prevent_default();
                        dropped_file.set(event.data_transfer_first_file_name());
                        console::log(
                            event.data_transfer_file_count(),
                            event.data_transfer_first_file_type(),
                            event.data_transfer_file_name_at(0),
                            event.data_transfer_file_type_at(0),
                            event.data_transfer_file_size_at(0),
                            event.data_transfer_item_count(),
                            event.data_transfer_item_kind_at(0),
                            event.data_transfer_item_type_at(0),
                        );
                    }}
                >
                    "Drop"
                </div>
                <div
                    on_animationend={|event: rsjs::AnimationEvent| {
                        console::log(event.animation_name(), event.elapsed_time(), event.pseudo_element());
                    }}
                    on_transitionend={|event: rsjs::TransitionEvent| {
                        console::log(event.property_name(), event.elapsed_time(), event.pseudo_element());
                    }}
                >
                    "Motion"
                </div>
                <button type="submit">"Save"</button>
                <span>{count.get()}</span>
                <span data-shifted={shifted.get()}></span>
                <span>{last_key.get()}</span>
                <span>{touch_x.get()}</span>
                <span>{picked_file.get()}</span>
                <span>{dropped_file.get()}</span>
            </form>
        }
        .render(cx)
    }
}

fn main() {}
