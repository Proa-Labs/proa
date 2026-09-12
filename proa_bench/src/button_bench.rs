// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Button benchmark comparing old vs new rendering approaches
//!
//! This module contains two Button implementations:
//! 1. `OldButton` - Current approach with heapless::String class building
//! 2. `NewButton` - Optimized approach with ClassList and html! macro
use proa_core::{ClassList, DataLoader, Variant, WebContext, WebRenderSync};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonType {
    #[default]
    Button,
    Submit,
    Reset,
}
impl ButtonType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Button => "button",
            Self::Submit => "submit",
            Self::Reset => "reset",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonSize {
    Sm,
    #[default]
    Md,
    Lg,
    Icon,
}
impl Variant for ButtonSize {
    #[inline(always)]
    fn classes(self) -> &'static str {
        Self::class_str(self)
    }
}
impl ButtonSize {
    /// Const fn version for compile-time use
    #[inline(always)]
    pub const fn class_str(self) -> &'static str {
        match self {
            Self::Sm => "h-8 gap-1.5 px-3 has-[>svg]:px-2.5",
            Self::Md => "h-9 px-4 py-2 has-[>svg]:px-3",
            Self::Lg => "h-10 px-6 has-[>svg]:px-4",
            Self::Icon => "size-9",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonColor {
    #[default]
    Primary,
    Secondary,
    Destructive,
}
impl Variant for ButtonColor {
    #[inline(always)]
    fn classes(self) -> &'static str {
        Self::class_str(self)
    }
}
impl ButtonColor {
    #[inline(always)]
    pub const fn class_str(self) -> &'static str {
        match self {
            Self::Primary => "bg-primary text-primary-foreground hover:bg-primary/90",
            Self::Secondary => "bg-secondary text-secondary-foreground hover:bg-secondary/80",
            Self::Destructive => "bg-destructive text-white shadow-xs hover:bg-destructive/80",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonVariant {
    #[default]
    Solid,
    Outline,
    Ghost,
}
impl Variant for ButtonVariant {
    #[inline(always)]
    fn classes(self) -> &'static str {
        Self::class_str(self)
    }
}
impl ButtonVariant {
    #[inline(always)]
    pub const fn class_str(self) -> &'static str {
        match self {
            Self::Solid => "",
            Self::Outline => {
                "border border-input !bg-background text-foreground shadow-xs hover:bg-accent hover:text-accent-foreground"
            }
            Self::Ghost => "text-foreground hover:bg-accent hover:text-accent-foreground",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonRadius {
    None,
    Sm,
    #[default]
    Md,
    Lg,
    Full,
}
impl Variant for ButtonRadius {
    #[inline(always)]
    fn classes(self) -> &'static str {
        Self::class_str(self)
    }
}
impl ButtonRadius {
    #[inline(always)]
    pub const fn class_str(self) -> &'static str {
        match self {
            Self::None => "rounded-none",
            Self::Sm => "rounded-sm",
            Self::Md => "rounded-md",
            Self::Lg => "rounded-lg",
            Self::Full => "rounded-full",
        }
    }
}
pub const BUTTON_BASE: &str = "inline-flex items-center justify-center gap-2 whitespace-nowrap text-sm font-medium transition-all cursor-pointer disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg:not([class*='size-'])]:size-4 shrink-0 [&_svg]:shrink-0 focus-visible:outline-none focus-visible:border-ring focus-visible:ring-ring/50 focus-visible:ring-[3px]";
pub struct OldButton<C = ()> {
    pub size: Option<ButtonSize>,
    pub color: Option<ButtonColor>,
    pub variant: Option<ButtonVariant>,
    pub radius: Option<ButtonRadius>,
    pub disabled: bool,
    pub button_type: ButtonType,
    pub class: Option<&'static str>,
    pub id: Option<&'static str>,
    pub children: Option<C>,
}
impl Default for OldButton<()> {
    fn default() -> Self {
        Self::new()
    }
}
impl OldButton<()> {
    pub const fn new() -> Self {
        Self {
            size: None,
            color: None,
            variant: None,
            radius: None,
            disabled: false,
            button_type: ButtonType::Button,
            class: None,
            id: None,
            children: None,
        }
    }
}
impl<C> OldButton<C> {
    /// Old approach: returns array of class strings that need to be joined
    pub fn classes(&self) -> ([&'static str; 5], usize) {
        let mut classes = [""; 5];
        let mut count = 0;
        classes[count] = BUTTON_BASE;
        count += 1;
        let size = self.size.unwrap_or_default();
        classes[count] = size.classes();
        count += 1;
        let color = self.color.unwrap_or_default();
        classes[count] = color.classes();
        count += 1;
        let variant = self.variant.unwrap_or_default();
        let v_classes = variant.classes();
        if !v_classes.is_empty() {
            classes[count] = v_classes;
            count += 1;
        }
        let radius = self.radius.unwrap_or_default();
        classes[count] = radius.classes();
        count += 1;
        (classes, count)
    }
}
// proa-perf-ok(scope): benchmark intentionally measures legacy stack class assembly and manual writes
impl<C, L: DataLoader> WebRenderSync<L> for OldButton<C>
where
    C: WebRenderSync<L>,
{
    fn render(self, cx: &mut WebContext<L>) -> Result<(), proa_core::WriteError> {
        use proa_core::text::UnsafeRaw;
        let (classes, class_count) = self.classes();
        let mut class_str = heapless::String::<512>::new();
        for (idx, c) in classes[..class_count].iter().enumerate() {
            if idx > 0 {
                let _ = class_str.push(' '); // proa-perf-ok: fixed-capacity stack string
            }
            let _ = class_str.push_str(c); // proa-perf-ok: fixed-capacity stack string
        }
        if let Some(custom) = self.class {
            if !class_str.is_empty() {
                let _ = class_str.push(' '); // proa-perf-ok: fixed-capacity stack string
            }
            let _ = class_str.push_str(custom); // proa-perf-ok: fixed-capacity stack string
        }
        UnsafeRaw("<button").render(cx)?;
        UnsafeRaw(" class=\"").render(cx)?;
        UnsafeRaw(class_str.as_str()).render(cx)?;
        UnsafeRaw("\"").render(cx)?;
        UnsafeRaw(" type=\"").render(cx)?;
        UnsafeRaw(self.button_type.as_str()).render(cx)?;
        UnsafeRaw("\"").render(cx)?;
        if let Some(id_val) = self.id {
            UnsafeRaw(" id=\"").render(cx)?;
            UnsafeRaw(id_val).render(cx)?;
            UnsafeRaw("\"").render(cx)?;
        }
        if self.disabled {
            UnsafeRaw(" disabled").render(cx)?;
        }
        UnsafeRaw(">").render(cx)?;
        if let Some(c) = self.children {
            c.render(cx)?;
        }
        UnsafeRaw("</button>").render(cx)
    }
}
/// Class computation from variant selections
/// Uses inherent const fn methods on each variant type
#[inline(always)]
pub fn button_classes(
    size: ButtonSize,
    color: ButtonColor,
    variant: ButtonVariant,
    radius: ButtonRadius,
) -> ClassList<6> {
    ClassList::new()
        .add(BUTTON_BASE)
        .add(size.class_str())
        .add(color.class_str())
        .add_non_empty(variant.class_str())
        .add(radius.class_str())
}
pub struct NewButton<C = ()> {
    pub size: Option<ButtonSize>,
    pub color: Option<ButtonColor>,
    pub variant: Option<ButtonVariant>,
    pub radius: Option<ButtonRadius>,
    pub disabled: bool,
    pub button_type: ButtonType,
    pub class: Option<&'static str>,
    pub id: Option<&'static str>,
    pub children: Option<C>,
}
impl Default for NewButton<()> {
    fn default() -> Self {
        Self::new()
    }
}
impl NewButton<()> {
    pub const fn new() -> Self {
        Self {
            size: None,
            color: None,
            variant: None,
            radius: None,
            disabled: false,
            button_type: ButtonType::Button,
            class: None,
            id: None,
            children: None,
        }
    }
}
// proa-perf-ok(scope): benchmark intentionally measures direct ClassList and manual writes
impl<C, L: DataLoader> WebRenderSync<L> for NewButton<C>
where
    C: WebRenderSync<L>,
{
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L>) -> Result<(), proa_core::WriteError> {
        use proa_core::text::UnsafeRaw;
        let size = self.size.unwrap_or_default();
        let color = self.color.unwrap_or_default();
        let variant = self.variant.unwrap_or_default();
        let radius = self.radius.unwrap_or_default();
        let classes = button_classes(size, color, variant, radius).add_opt(self.class);
        UnsafeRaw("<button type=\"").render(cx)?;
        UnsafeRaw(self.button_type.as_str()).render(cx)?;
        UnsafeRaw("\" class=\"").render(cx)?;
        classes.render(cx)?;
        UnsafeRaw("\"").render(cx)?;
        if let Some(id) = self.id {
            UnsafeRaw(" id=\"").render(cx)?;
            UnsafeRaw(id).render(cx)?;
            UnsafeRaw("\"").render(cx)?;
        }
        if self.disabled {
            UnsafeRaw(" disabled").render(cx)?;
        }
        UnsafeRaw(">").render(cx)?;
        if let Some(children) = self.children {
            children.render(cx)?;
        }
        UnsafeRaw("</button>").render(cx)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use proa_core::text::text;
    #[test]
    fn old_button_renders() {
        let mut cx = WebContext::new();
        OldButton {
            size: Some(ButtonSize::Lg),
            color: Some(ButtonColor::Primary),
            variant: None,
            radius: None,
            disabled: false,
            button_type: ButtonType::Button,
            class: None,
            id: None,
            children: Some(text("Click me")),
        }
        .render(&mut cx)
        .unwrap();
        let html = cx.into_output().into_string();
        assert!(html.contains("h-10"));
        assert!(html.contains("bg-primary"));
        assert!(html.contains("Click me"));
    }
    #[test]
    fn new_button_renders() {
        let mut cx = WebContext::new();
        NewButton {
            size: Some(ButtonSize::Lg),
            color: Some(ButtonColor::Primary),
            variant: None,
            radius: None,
            disabled: false,
            button_type: ButtonType::Button,
            class: None,
            id: None,
            children: Some(text("Click me")),
        }
        .render(&mut cx)
        .unwrap();
        let html = cx.into_output().into_string();
        assert!(html.contains("h-10"));
        assert!(html.contains("bg-primary"));
        assert!(html.contains("Click me"));
    }
    #[test]
    fn both_buttons_have_same_classes() {
        let mut old_cx = WebContext::new();
        OldButton {
            size: Some(ButtonSize::Sm),
            color: Some(ButtonColor::Destructive),
            variant: Some(ButtonVariant::Outline),
            radius: Some(ButtonRadius::Lg),
            disabled: false,
            button_type: ButtonType::Button,
            class: None,
            id: None,
            children: Some(text("Test")),
        }
        .render(&mut old_cx)
        .unwrap();
        let old_html = old_cx.into_output().into_string();
        let mut new_cx = WebContext::new();
        NewButton {
            size: Some(ButtonSize::Sm),
            color: Some(ButtonColor::Destructive),
            variant: Some(ButtonVariant::Outline),
            radius: Some(ButtonRadius::Lg),
            disabled: false,
            button_type: ButtonType::Button,
            class: None,
            id: None,
            children: Some(text("Test")),
        }
        .render(&mut new_cx)
        .unwrap();
        let new_html = new_cx.into_output().into_string();
        for class in ["h-8", "bg-destructive", "border", "rounded-lg"] {
            assert!(old_html.contains(class), "Old missing: {}", class);
            assert!(new_html.contains(class), "New missing: {}", class);
        }
    }
}
