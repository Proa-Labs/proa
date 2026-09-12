// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Dialog component - modal overlay for focused interactions
//!
//! A Dialog is a window overlaid on the primary content that requires
//! user interaction. It traps focus, handles escape key, and prevents
//! background scrolling.
//!
//! # Sub-components
//!
//! - `Dialog` - Root component managing open/close state
//! - `DialogTrigger` - Button that opens the dialog
//! - `DialogContent` - The modal content container
//! - `DialogOverlay` - Semi-transparent backdrop
//! - `DialogHeader` - Container for title and description
//! - `DialogTitle` - Accessible dialog title
//! - `DialogDescription` - Optional description text
//! - `DialogFooter` - Container for action buttons
//! - `DialogClose` - Wrapper marking an author's control as the dismiss action
//! - `DialogCloseButton` - The dismiss action itself, as a typed button
//!
//! # SSR Behavior
//!
//! On the server, Dialog renders its content in a hidden state by default.
//! The TypeScript hydration code handles:
//! - Portal rendering (moving content to document body)
//! - Focus trapping and restoration
//! - Keyboard/click dismissal
//! - Body scroll locking
//!
//! # Example
//!
//! ```ignore
//! html! {
//!     <Dialog>
//!         <DialogTrigger>
//!             <Button>"Open"</Button>
//!         </DialogTrigger>
//!         <DialogContent>
//!             <DialogHeader>
//!                 <DialogTitle>"Edit Profile"</DialogTitle>
//!                 <DialogDescription>"Make changes to your profile."</DialogDescription>
//!             </DialogHeader>
//!             // ... form content
//!             <DialogFooter>
//!                 // Wrapping your own button: DialogClose marks it as the
//!                 // dismiss action and stays out of the way.
//!                 <DialogClose>
//!                     <Button variant={ButtonVariant::Ghost}>"Cancel"</Button>
//!                 </DialogClose>
//!                 // With only a label to render, use DialogCloseButton — it
//!                 // is the button, so the control is focusable and typed.
//!                 <Button>"Save"</Button>
//!             </DialogFooter>
//!         </DialogContent>
//!     </Dialog>
//! }
//! ```

use crate::components::_proa::Anatomy;

#[allow(clippy::module_inception)]
mod dialog;
mod md;
mod recipe;

/// Static anatomy contract for Dialog's public compound parts.
pub const DIALOG_ANATOMY: Anatomy = Anatomy::new(
    "dialog",
    &[
        "root",
        "trigger",
        "portal",
        "overlay",
        "content",
        "close-button",
        "close-label",
        "header",
        "title",
        "description",
        "footer",
        "close",
    ],
);

pub use dialog::{
    Dialog, DialogButtonTrigger, DialogClose, DialogCloseButton, DialogContent, DialogDescription,
    DialogFooter, DialogHeader, DialogOverlay, DialogTitle, DialogTrigger,
};
pub use recipe::{
    dialog_close_classes, dialog_content_classes, dialog_description_classes,
    dialog_footer_classes, dialog_header_classes, dialog_overlay_classes, dialog_recipe,
    dialog_title_classes, dialog_trigger_classes, DialogContentRecipeProps, DialogContentStyle,
    DialogRecipe, DialogRecipeProps, DialogRootRecipeProps, DialogRootStyle, DialogSize,
    DIALOG_RECIPE,
};
