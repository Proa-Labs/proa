#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

#[path = "../support.rs"]
mod support;

use rsjs::rsjs;
use support::*;

pub struct VisibleHydration;

#[rsjs(component, client, strategy = "visible")]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for VisibleHydration {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let ready = signal(0_i64);

        proa_macros::html_sync! {
            <button type="button" on_click={|_| ready.set(1_i64)}>
                <span>{ready.get()}</span>
            </button>
        }
        .render(cx)
    }
}

pub struct IdleHydration;

#[rsjs(component, client, strategy = "idle")]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for IdleHydration {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let ready = signal(0_i64);

        proa_macros::html_sync! {
            <button type="button" on_click={|_| ready.set(1_i64)}>
                <span>{ready.get()}</span>
            </button>
        }
        .render(cx)
    }
}

pub struct InteractionHydration;

#[rsjs(component, client, strategy = "interaction:click")]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for InteractionHydration {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let ready = signal(0_i64);

        proa_macros::html_sync! {
            <button type="button" on_click={|_| ready.set(1_i64)}>
                <span>{ready.get()}</span>
            </button>
        }
        .render(cx)
    }
}

pub struct MediaHydration;

#[rsjs(component, client, strategy = "media:(min-width: 768px)")]
impl<B: WriteBuf, L: proa_core::DataLoader> WebRenderSync<L, B> for MediaHydration {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let ready = signal(0_i64);

        proa_macros::html_sync! {
            <button type="button" on_click={|_| ready.set(1_i64)}>
                <span>{ready.get()}</span>
            </button>
        }
        .render(cx)
    }
}

fn main() {}
