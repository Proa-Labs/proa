// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::render::DataLoader;
use crate::render::NoLoader;
use crate::{
    render::{WebContext, WebRenderSync},
    shared::{
        attr::{has_dangerous_url_scheme, Attr, AttrKey},
        escape::escape_attr_value_for_dialect,
        tag::Tag,
        write_buf::{WriteBuf, WriteError},
    },
};
use heapless::Vec as InlineVec;
use std::marker::PhantomData;

/// Marker for normal (non-void) elements.
#[derive(Clone, Copy, Debug, Default)]
pub struct Normal;

/// Marker for void (self-closing) elements.
#[derive(Clone, Copy, Debug, Default)]
pub struct Void;

/// Element wrapper capturing tag, attributes, and child continuation.
///
/// Generic over `B: WriteBuf` and `L: DataLoader`. The `L` parameter
/// defaults to [`NoLoader`] so existing call sites compile unchanged.
pub struct Elem<'a, Kind, const N: usize, B: WriteBuf, F, L: DataLoader = NoLoader>
where
    F: FnOnce(&mut WebContext<L, B>) -> Result<(), WriteError>,
{
    tag: Tag,
    attrs: InlineVec<Attr<'a>, N>,
    child: F,
    _kind: PhantomData<Kind>,
    _buf: PhantomData<fn(B)>,
    _loader: PhantomData<fn(L)>,
}

type VoidChild<B, L> = for<'cx> fn(&'cx mut WebContext<L, B>) -> Result<(), WriteError>;

/// Constructor for normal elements.
pub fn el<'a, const N: usize, B: WriteBuf, F, L: DataLoader>(
    tag: Tag,
    attrs: [Attr<'a>; N],
    child: F,
) -> Elem<'a, Normal, N, B, F, L>
where
    F: FnOnce(&mut WebContext<L, B>) -> Result<(), WriteError>,
{
    debug_assert!(
        !tag.is_void(),
        "use `void` constructor when targeting void elements"
    );
    let attrs_vec =
        InlineVec::from_slice(&attrs).expect("attribute spill should be exceedingly rare");
    Elem {
        tag,
        attrs: attrs_vec,
        child,
        _kind: PhantomData,
        _buf: PhantomData,
        _loader: PhantomData,
    }
}

/// Constructor for void elements that cannot carry children.
pub fn void<'a, const N: usize, B: WriteBuf, L: DataLoader>(
    tag: Tag,
    attrs: [Attr<'a>; N],
) -> Elem<'a, Void, N, B, VoidChild<B, L>, L> {
    debug_assert!(
        tag.is_void(),
        "void constructor requires a void-capable tag"
    );
    let attrs_vec =
        InlineVec::from_slice(&attrs).expect("attribute spill should be exceedingly rare");
    Elem {
        tag,
        attrs: attrs_vec,
        child: void_child::<B, L>,
        _kind: PhantomData,
        _buf: PhantomData,
        _loader: PhantomData,
    }
}

fn void_child<B: WriteBuf, L: DataLoader>(_: &mut WebContext<L, B>) -> Result<(), WriteError> {
    Ok(())
}

impl<'a, const N: usize, L: DataLoader, B: WriteBuf, F> WebRenderSync<L, B>
    for Elem<'a, Normal, N, B, F, L>
where
    F: FnOnce(&mut WebContext<L, B>) -> Result<(), WriteError>,
{
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let Elem {
            tag, attrs, child, ..
        } = self;
        write_element_open(cx, tag, attrs.as_slice())?;
        let prev = cx.enter_mode(tag.mode());
        child(cx)?;
        cx.restore_mode(prev);
        write_element_close(cx, tag)
    }
}

impl<'a, const N: usize, L: DataLoader, B: WriteBuf, F> WebRenderSync<L, B>
    for Elem<'a, Void, N, B, F, L>
where
    F: FnOnce(&mut WebContext<L, B>) -> Result<(), WriteError>,
{
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let Elem { tag, attrs, .. } = self;
        write_element_open(cx, tag, attrs.as_slice())?;
        Ok(())
    }
}

#[inline(always)]
pub(crate) fn write_element_open<B: WriteBuf, L: DataLoader>(
    cx: &mut WebContext<L, B>,
    tag: Tag,
    attrs: &[Attr<'_>],
) -> Result<(), WriteError> {
    cx.out.extend_static(tag.open_bytes())?;
    let mut has_target_blank = false;
    let mut existing_rel: Option<&str> = None;
    let mut rel_has_noopener = false;
    let is_anchor = matches!(tag, Tag::A);
    if is_anchor {
        for attr in attrs {
            match attr {
                Attr::Str {
                    k: AttrKey::Target,
                    v,
                } if *v == "_blank" => {
                    has_target_blank = true;
                }
                Attr::Str { k: AttrKey::Rel, v } => {
                    existing_rel = Some(v);
                    rel_has_noopener = v
                        .as_bytes()
                        .windows(8)
                        .any(|w| w.eq_ignore_ascii_case(b"noopener"));
                }
                _ => {}
            }
        }
    }
    let needs_merge = is_anchor && has_target_blank && !rel_has_noopener;
    for attr in attrs {
        if needs_merge
            && matches!(
                attr,
                Attr::Str {
                    k: AttrKey::Rel,
                    ..
                }
            )
        {
            continue;
        }
        write_attr(cx, attr)?;
    }
    if needs_merge {
        match existing_rel {
            None => {
                cx.out.extend_static(b" rel=\"noopener noreferrer\"")?;
            }
            Some(existing) => {
                cx.out.extend_static(b" rel=\"")?;
                cx.out.extend(existing.as_bytes())?;
                cx.out.extend_static(b" noopener noreferrer\"")?;
            }
        }
    }
    cx.out.extend_static(b">")
}

#[inline(always)]
pub(crate) fn write_element_close<B: WriteBuf, L: DataLoader>(
    cx: &mut WebContext<L, B>,
    tag: Tag,
) -> Result<(), WriteError> {
    debug_assert!(!tag.is_void(), "void tag should not emit a close element");
    cx.out.extend_static(tag.close_bytes())
}

#[inline(always)]
fn write_attr<B: WriteBuf, L: DataLoader>(
    cx: &mut WebContext<L, B>,
    attr: &Attr<'_>,
) -> Result<(), WriteError> {
    if let Err(msg) = attr.validate() {
        return Err(WriteError::InvalidAttribute(msg));
    }
    match attr {
        Attr::Str { k, v } => {
            if k.is_url_attr() && has_dangerous_url_scheme(v) {
                debug_assert!(
                    false,
                    "dangerous URL scheme in `{}` attribute: \
                     `javascript:`, `vbscript:`, and executable `data:` URIs are blocked to prevent XSS. \
                     Use a safe scheme (https:, /, #) or pass data via data-* attributes.",
                    k.as_str()
                );
                cx.out.extend_static(k.attr_open_bytes())?;
                cx.out.extend_static(b"\"")?;
                return Ok(());
            }
            cx.out.extend_static(k.attr_open_bytes())?;
            let dialect = cx.__proa_current_escape_dialect();
            escape_attr_value_for_dialect(&mut cx.out, v, dialect)?;
            cx.out.extend_static(b"\"")?;
        }
        Attr::Bool { k, present } => {
            if *present {
                cx.out.extend_static(k.attr_bare_bytes())?;
            }
        }
        Attr::Data { name, v } => {
            cx.out.extend_static(b" ")?;
            cx.out.extend(name.as_bytes())?;
            cx.out.extend_static(b"=\"")?;
            let dialect = cx.__proa_current_escape_dialect();
            escape_attr_value_for_dialect(&mut cx.out, v, dialect)?;
            cx.out.extend_static(b"\"")?;
        }
    }
    Ok(())
}
