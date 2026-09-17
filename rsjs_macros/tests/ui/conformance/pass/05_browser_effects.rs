#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct BrowserEffects;

#[rsjs(component, client)]
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for BrowserEffects {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let hash = signal("filters=open".to_string());
        let credential_options = signal(r#"{"challenge":"passkey"}"#.to_string());
        let credential_json = signal(String::new());

        proa_macros::html_sync! {
            <div data-url={rsjs::location::href()}>
                <span>{rsjs::location::search()}</span>
                <button on_click={|_| {
                    hash.set(rsjs::local_storage::get_item_or(String::new(), String::new()));
                    console::log(rsjs::session_storage::get_item(String::new()));
                    console::log(
                        rsjs::local_storage::get_bool_or("enabled", false),
                        rsjs::local_storage::get_i64_or("count", 0_i64),
                        rsjs::session_storage::get_f64_or("ratio", 0.0_f64),
                    );
                    rsjs::window::scroll_to(0_i64, 100_i64);
                    rsjs::window::scroll_by(0_i64, 20_i64);
                    rsjs::window::print();
                    rsjs::location::set_hash(hash.get());
                    rsjs::location::assign("/catalog/filters");
                    rsjs::location::replace(hash.get());
                    rsjs::location::reload();
                    rsjs::history::push_state("/catalog#filters");
                    rsjs::history::replace_state(rsjs::location::pathname());
                    rsjs::history::back();
                    rsjs::history::forward();
                    rsjs::history::go(1_i64);
                    rsjs::document::dispatch_event("filters:changed");
                    rsjs::document::dispatch_event_with_detail("filters:changed", hash.get());
                    rsjs::document::set_title(hash.get());
                    rsjs::clipboard::write_text(hash.get());
                    rsjs::webauthn::create_json_into(credential_options.get(), &credential_json);
                    rsjs::webauthn::get_json_into_or(credential_options.get(), &credential_json, "unavailable");
                    rsjs::local_storage::set_item("filters", hash.get());
                    rsjs::local_storage::remove_item("filters:stale");
                    rsjs::session_storage::set_item("filters", hash.get());
                    rsjs::session_storage::remove_item("filters:stale");
                    rsjs::url::set_search_param("filters", hash.get());
                    rsjs::url::delete_search_param("stale");
                    rsjs::url::push_search_param("page", "2");
                    rsjs::url::push_delete_search_param("modal");
                    console::log(rsjs::encoding::base64url_encode(rsjs::encoding::utf8_bytes(hash.get())));
                }}>
                    "Apply"
                </button>
            </div>
        }
        .render(cx)
    }
}

fn main() {}
