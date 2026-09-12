// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Dialog recipe — typed root/content resolution and named slot classes.
//!
//! Dialog has two styling boundaries: the semantic root and the portalled
//! content. The root resolves independently, while the content owns the size
//! variant. Every remaining public part reads one static named slot.

use crate::tw_join;
use proa_core::shared::attr_value::AttrValue;
use proa_core::{DataLoader, WebContext, WriteBuf, WriteError};

pub(crate) const DIALOG_ROOT: &str = "group/dialog";

pub(crate) const DIALOG_TRIGGER: &str = tw_join!(
    "inline-flex",
    "items-center",
    "justify-center",
    "gap-2",
    "whitespace-nowrap",
    "text-sm",
    "font-medium",
    "transition-all",
    "cursor-pointer",
    "rounded-md",
    "h-9",
    "px-4",
    "py-2",
    "border",
    "border-input",
    "bg-background",
    "text-foreground",
    "shadow-xs",
    "hover:bg-accent",
    "hover:text-accent-foreground"
);

pub(crate) const DIALOG_OVERLAY: &str = tw_join!(
    "fixed",
    "inset-0",
    "z-50",
    "bg-black/50",
    "data-[state=open]:animate-in",
    "data-[state=closed]:animate-out",
    "data-[state=closed]:fade-out-0",
    "data-[state=open]:fade-in-0"
);

pub(crate) const DIALOG_CONTENT: &str = tw_join!(
    "fixed",
    "left-1/2",
    "top-1/2",
    "z-50",
    "-translate-x-1/2",
    "-translate-y-1/2",
    "grid",
    "w-full",
    "max-w-[calc(100%-2rem)]",
    "gap-4",
    "border",
    "border-border",
    "bg-background",
    "p-6",
    "shadow-lg",
    "duration-200",
    "data-[state=open]:animate-in",
    "data-[state=closed]:animate-out",
    "data-[state=closed]:fade-out-0",
    "data-[state=open]:fade-in-0",
    "data-[state=closed]:zoom-out-95",
    "data-[state=open]:zoom-in-95",
    "rounded-lg"
);

pub(crate) const DIALOG_HEADER: &str = tw_join!(
    "flex",
    "flex-col",
    "space-y-1.5",
    "text-center",
    "sm:text-left"
);

/// The footer is the one row that mixes a caller's own button with whatever
/// `DialogClose` was handed, so it cannot lean on the flex defaults.
///
/// `align-items` defaults to `stretch`, which gives every child the height of
/// the tallest one. A button that centres its own label (`min-h-… items-center`)
/// still reads as centred; a plain label paints on the first line box of a box
/// it never asked to be that tall, and the two sit visibly off each other.
/// `sm:items-center` sizes each child to its content and centres it instead.
///
/// `gap-2` rather than `sm:space-x-2`, because `space-x-*` is horizontal
/// margin only: in the stacked mobile layout the actions had no separation at
/// all.
pub(crate) const DIALOG_FOOTER: &str = tw_join!(
    "flex",
    "flex-col-reverse",
    "gap-2",
    "sm:flex-row",
    "sm:items-center",
    "sm:justify-end"
);

pub(crate) const DIALOG_TITLE: &str = tw_join!(
    "text-lg",
    "font-semibold",
    "leading-none",
    "tracking-tight",
    "text-foreground"
);

pub(crate) const DIALOG_DESCRIPTION: &str = tw_join!("text-sm", "text-muted-foreground");

/// The public `DialogClose` is a semantic wrapper around author-provided
/// controls, so it carries no visual treatment — a border or padding here would
/// paint a second box around a caller's own button.
///
/// Alignment it does need. The wrapper is a footer flex item sitting beside
/// buttons that centre their own label, and it was previously the empty string,
/// which also left `class=" "` on every close in the rendered markup.
pub(crate) const DIALOG_CLOSE: &str = tw_join!("inline-flex", "items-center", "cursor-pointer");

/// Styling for the optional built-in X button rendered by `DialogContent`.
pub(crate) const DIALOG_CLOSE_BUTTON: &str = tw_join!(
    "absolute",
    "right-4",
    "top-4",
    "rounded-sm",
    "opacity-70",
    "ring-offset-background",
    "transition-opacity",
    "hover:opacity-100",
    "focus:outline-hidden",
    "focus:ring-2",
    "focus:ring-ring",
    "focus:ring-offset-2",
    "disabled:pointer-events-none",
    "data-[state=open]:bg-accent",
    "data-[state=open]:text-muted-foreground"
);

const DIALOG_SIZE_SM: &str = "max-w-sm";
pub(crate) const DIALOG_SIZE_MD: &str = "max-w-lg";
const DIALOG_SIZE_LG: &str = "max-w-2xl";
const DIALOG_SIZE_XL: &str = "max-w-4xl";
const DIALOG_SIZE_FULL: &str = "max-w-[calc(100vw-2rem)] max-h-[calc(100vh-2rem)]";

/// Dialog content size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, proa_macros::ProaVariant)]
#[variant(name = "size", path = "crate::components::_proa::variant_spec")]
pub enum DialogSize {
    Sm,
    #[default]
    Md,
    Lg,
    Xl,
    Full,
}

impl DialogSize {
    pub const fn classes(self) -> &'static str {
        match self {
            Self::Sm => DIALOG_SIZE_SM,
            Self::Md => DIALOG_SIZE_MD,
            Self::Lg => DIALOG_SIZE_LG,
            Self::Xl => DIALOG_SIZE_XL,
            Self::Full => DIALOG_SIZE_FULL,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sm => "sm",
            Self::Md => "md",
            Self::Lg => "lg",
            Self::Xl => "xl",
            Self::Full => "full",
        }
    }
}

/// Typed inputs for the Dialog root styling boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DialogRootRecipeProps;

/// Typed inputs for the portalled Dialog content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DialogContentRecipeProps {
    pub size: Option<DialogSize>,
}

/// Conventional recipe-props name retained for consumers that resolve content.
pub type DialogRecipeProps = DialogContentRecipeProps;

/// Static named classes for every Dialog part.
#[derive(Debug, Clone, Copy)]
pub struct DialogRecipe {
    pub root: &'static str,
    pub trigger: &'static str,
    pub overlay: &'static str,
    pub content: &'static str,
    pub header: &'static str,
    pub footer: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub close: &'static str,
    pub close_button: &'static str,
    pub close_label: &'static str,
    size_default: DialogSize,
}

impl DialogRecipe {
    pub const CLASS_CAPACITY: usize = 2;

    pub const fn resolve_root(&'static self, _props: DialogRootRecipeProps) -> DialogRootStyle {
        DialogRootStyle { recipe: self }
    }

    pub const fn resolve_content(
        &'static self,
        props: DialogContentRecipeProps,
    ) -> DialogContentStyle {
        let size = match props.size {
            Some(size) => size,
            None => self.size_default,
        };
        DialogContentStyle { recipe: self, size }
    }

    /// Compatibility alias for the former content-oriented recipe resolver.
    pub const fn resolve(&'static self, props: DialogRecipeProps) -> DialogContentStyle {
        self.resolve_content(props)
    }

    /// Compatibility representation for callers of the former derived recipe.
    pub const fn classes(
        &'static self,
        size: Option<DialogSize>,
    ) -> ([&'static str; Self::CLASS_CAPACITY], usize) {
        let style = self.resolve_content(DialogContentRecipeProps { size });
        ([self.content, style.size.classes()], Self::CLASS_CAPACITY)
    }
}

/// Resolved Dialog root classes.
#[must_use]
#[derive(Debug, Clone, Copy)]
pub struct DialogRootStyle {
    recipe: &'static DialogRecipe,
}

impl AttrValue for DialogRootStyle {
    #[inline(always)]
    fn should_render(&self) -> bool {
        true
    }

    #[inline(always)]
    fn render_attr_value<B: WriteBuf, L: DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), WriteError> {
        cx.out.extend_static(self.recipe.root.as_bytes())
    }
}

/// Resolved Dialog content classes and effective size.
#[must_use]
#[derive(Debug, Clone, Copy)]
pub struct DialogContentStyle {
    recipe: &'static DialogRecipe,
    size: DialogSize,
}

impl DialogContentStyle {
    pub const fn size(self) -> DialogSize {
        self.size
    }
}

impl AttrValue for DialogContentStyle {
    #[inline(always)]
    fn should_render(&self) -> bool {
        true
    }

    #[inline(always)]
    fn render_attr_value<B: WriteBuf, L: DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), WriteError> {
        cx.out.extend_static(self.recipe.content.as_bytes())?;
        cx.out.extend_static(b" ")?;
        cx.out.extend_static(self.size.classes().as_bytes())
    }
}

/// Global zero-initialization Dialog recipe.
pub const DIALOG_RECIPE: DialogRecipe = DialogRecipe {
    root: DIALOG_ROOT,
    trigger: DIALOG_TRIGGER,
    overlay: DIALOG_OVERLAY,
    content: DIALOG_CONTENT,
    header: DIALOG_HEADER,
    footer: DIALOG_FOOTER,
    title: DIALOG_TITLE,
    description: DIALOG_DESCRIPTION,
    close: DIALOG_CLOSE,
    close_button: DIALOG_CLOSE_BUTTON,
    close_label: "sr-only",
    size_default: DialogSize::Md,
};

pub const fn dialog_recipe() -> &'static DialogRecipe {
    &DIALOG_RECIPE
}

/// Compatibility helpers for the former standalone class API.
pub const fn dialog_trigger_classes() -> &'static str {
    DIALOG_RECIPE.trigger
}

pub const fn dialog_overlay_classes() -> &'static str {
    DIALOG_RECIPE.overlay
}

pub const fn dialog_content_classes(size: DialogSize) -> DialogContentStyle {
    DIALOG_RECIPE.resolve_content(DialogContentRecipeProps { size: Some(size) })
}

pub const fn dialog_header_classes() -> &'static str {
    DIALOG_RECIPE.header
}

pub const fn dialog_footer_classes() -> &'static str {
    DIALOG_RECIPE.footer
}

pub const fn dialog_title_classes() -> &'static str {
    DIALOG_RECIPE.title
}

pub const fn dialog_description_classes() -> &'static str {
    DIALOG_RECIPE.description
}

/// Historical helper for the built-in X button.
pub const fn dialog_close_classes() -> &'static str {
    DIALOG_RECIPE.close_button
}

impl crate::components::_proa::variant_spec::ComponentSpec for DialogRecipe {
    const NAME: &'static str = "Dialog";

    fn base_classes(&self) -> &'static str {
        self.content
    }

    fn variant_metadata(&self) -> Vec<crate::components::_proa::variant_spec::VariantMetadata> {
        vec![crate::components::_proa::variant_spec::variant_metadata::<
            DialogSize,
        >()]
    }

    fn anatomy(&self) -> Option<crate::components::_proa::Anatomy> {
        Some(super::DIALOG_ANATOMY)
    }
}
