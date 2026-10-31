// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{DataLoader, StaticRaw, WebContext, WebRenderSync, WriteBuf};
use proa_macros::html_sync;

use crate::{
    icons::nav_icon_svg,
    model::{NavItem, NavItemKind},
};

pub struct DocsNav<'a> {
    pub items: &'a [NavItem],
    pub active_path: &'a str,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for DocsNav<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        html_sync! {
            <nav data-component="docs-nav" class="proa-docs-nav">
                <ul>
                    {for item in self.items {
                        DocsNavItem {
                            item,
                            active_path: self.active_path,
                        }
                    }}
                </ul>
            </nav>
        }
        .render(cx)
    }
}

pub struct DocsNavItem<'a> {
    pub item: &'a NavItem,
    pub active_path: &'a str,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for DocsNavItem<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        if self.item.is_root() {
            return Ok(());
        }

        html_sync! {
            <li data-nav-kind={self.item.kind.as_str()}>
                {DocsNavLabel {
                    item: self.item,
                    active_path: self.active_path,
                }}
                {DocsNavChildren {
                    children: self.item.children,
                    active_path: self.active_path,
                }}
            </li>
        }
        .render(cx)
    }
}

struct DocsNavLabel<'a> {
    item: &'a NavItem,
    active_path: &'a str,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for DocsNavLabel<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        let active = if self.item.path == Some(self.active_path) {
            Some("true")
        } else {
            None
        };
        if self.item.kind == NavItemKind::Page {
            let Some(path) = self.item.path else {
                return Ok(());
            };
            let path = cx.url(path);
            html_sync! {
                <a href={path} data_active={active}>
                    {DocsNavIcon { icon: self.item.icon }}
                    {self.item.title}
                </a>
            }
            .render(cx)
        } else {
            html_sync! {
                <span class={nav_label_class(self.item.kind)}>
                    {DocsNavIcon { icon: self.item.icon }}
                    {self.item.title}
                </span>
            }
            .render(cx)
        }
    }
}

const fn nav_label_class(kind: NavItemKind) -> Option<&'static str> {
    match kind {
        NavItemKind::Separator => Some("proa-docs-nav-separator"),
        NavItemKind::Group => Some("proa-docs-nav-group"),
        NavItemKind::Page | NavItemKind::Root => None,
    }
}

struct DocsNavIcon {
    icon: Option<&'static str>,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for DocsNavIcon {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        if let Some(icon) = self.icon {
            let svg = nav_icon_svg(icon);
            html_sync! {
                <span class="proa-docs-nav-icon" data_icon={icon} aria-hidden="true">
                    {StaticRaw(svg)}
                </span>
            }
            .render(cx)
        } else {
            Ok(())
        }
    }
}

struct DocsNavChildren<'a> {
    children: &'a [NavItem],
    active_path: &'a str,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for DocsNavChildren<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        if self.children.is_empty() {
            return Ok(());
        }
        html_sync! {
            <ul>
                {for child in self.children {
                    DocsNavItem {
                        item: child,
                        active_path: self.active_path,
                    }
                }}
            </ul>
        }
        .render(cx)
    }
}
