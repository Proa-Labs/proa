// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{DataLoader, StaticRaw, WebContext, WebRenderSync, WriteBuf};
use proa_macros::html_sync;

use crate::{icons::nav_icon_svg, model::Docs};

pub struct DocsSectionMenu<'a> {
    pub docs: &'a Docs,
    pub active_path: &'a str,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for DocsSectionMenu<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        if !self.docs.has_nav_roots() {
            return Ok(());
        }

        let active_root = self.docs.active_nav_root(self.active_path);
        html_sync! {
            <details data-component="docs-section-menu" class="docs-sidebar-section-menu">
                <summary class="docs-sidebar-section-select" aria-label="Switch content section">
                    {DocsSectionIcon { icon: active_root.icon }}
                    <span>{active_root.title}</span>
                    <span class="docs-sidebar-section-chevron" aria-hidden="true">"v"</span>
                </summary>
                <div class="docs-sidebar-section-list" role="menu">
                    {DocsSectionMenuItem {
                        href: self.docs.config.base_path,
                        label: self.docs.nav_title,
                        icon: self.docs.nav_icon,
                        active: active_root.href == self.docs.config.base_path,
                    }}
                    {DocsSectionMenuRoots {
                        items: self.docs.nav,
                        active_path: self.active_path,
                        default_href: self.docs.config.base_path,
                    }}
                </div>
            </details>
        }
        .render(cx)
    }
}

struct DocsSectionMenuRoots<'a> {
    items: &'static [crate::model::NavItem],
    active_path: &'a str,
    default_href: &'static str,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for DocsSectionMenuRoots<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        for item in self.items {
            if item.is_root() {
                DocsSectionMenuItem {
                    href: item.first_page_path().unwrap_or(self.default_href),
                    label: item.title,
                    icon: item.icon,
                    active: item.contains_path(self.active_path),
                }
                .render(cx)?;
            }
            DocsSectionMenuRoots {
                items: item.children,
                active_path: self.active_path,
                default_href: self.default_href,
            }
            .render(cx)?;
        }
        Ok(())
    }
}

struct DocsSectionMenuItem {
    href: &'static str,
    label: &'static str,
    icon: Option<&'static str>,
    active: bool,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for DocsSectionMenuItem {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        let active = self.active.then_some("true");
        let href = cx.url(self.href);
        html_sync! {
            <a
                href={href}
                role="menuitem"
                data-active={active}
                data-nav-root={self.label}
            >
                {DocsSectionIcon { icon: self.icon }}
                <span>{self.label}</span>
            </a>
        }
        .render(cx)
    }
}

struct DocsSectionIcon {
    icon: Option<&'static str>,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for DocsSectionIcon {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        let svg = self.icon.map(nav_icon_svg).unwrap_or("");
        html_sync! {
            <span class="docs-sidebar-section-icon" data-icon={self.icon} aria-hidden="true">
                {StaticRaw(svg)}
            </span>
        }
        .render(cx)
    }
}
