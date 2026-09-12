// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: (MIT OR Apache-2.0) AND MIT

//! Theme configuration for Proa UI
//!
//! Similar to Panda CSS tokens, this module provides a centralized source of truth
//! for all primitive design values: colors, spacing, border radii, typography, etc.
//!
//! These tokens are **primitive values**, not component-specific implementations.
//! Component recipes compose these tokens to create their styling.
//!
//! ## Token Categories (matching Panda CSS)
//! - Colors: Brand colors, semantic colors, color scales
//! - Spacing: Padding, margin, gap values
//! - Sizing: Width, height values
//! - Fonts: Font family definitions
//! - Font Sizes: Typography scale
//! - Font Weights: Typography weights
//! - Line Heights: Typography line heights
//! - Letter Spacing: Typography tracking
//! - Radii: Border radius values
//! - Shadows: Box shadow definitions
//! - Z-Index: Layering scale
//! - Transitions: Animation timings
//! - Durations: Animation durations
//! - Ease Functions: Animation easing

/// Colors token configuration
///
/// Defines the raw color values used throughout the design system.
/// Based on Tailwind's color palette.
pub struct Colors;

impl Colors {
    // Primary color scale (blue)
    pub const BLUE_50: &'static str = "blue-50";
    pub const BLUE_100: &'static str = "blue-100";
    pub const BLUE_200: &'static str = "blue-200";
    pub const BLUE_300: &'static str = "blue-300";
    pub const BLUE_400: &'static str = "blue-400";
    pub const BLUE_500: &'static str = "blue-500";
    pub const BLUE_600: &'static str = "blue-600";
    pub const BLUE_700: &'static str = "blue-700";
    pub const BLUE_800: &'static str = "blue-800";
    pub const BLUE_900: &'static str = "blue-900";

    // Gray scale
    pub const GRAY_50: &'static str = "gray-50";
    pub const GRAY_100: &'static str = "gray-100";
    pub const GRAY_200: &'static str = "gray-200";
    pub const GRAY_300: &'static str = "gray-300";
    pub const GRAY_400: &'static str = "gray-400";
    pub const GRAY_500: &'static str = "gray-500";
    pub const GRAY_600: &'static str = "gray-600";
    pub const GRAY_700: &'static str = "gray-700";
    pub const GRAY_800: &'static str = "gray-800";
    pub const GRAY_900: &'static str = "gray-900";

    // Success color scale (green)
    pub const GREEN_50: &'static str = "green-50";
    pub const GREEN_500: &'static str = "green-500";
    pub const GREEN_600: &'static str = "green-600";
    pub const GREEN_700: &'static str = "green-700";

    // Danger color scale (red)
    pub const RED_50: &'static str = "red-50";
    pub const RED_500: &'static str = "red-500";
    pub const RED_600: &'static str = "red-600";
    pub const RED_700: &'static str = "red-700";

    // Warning color scale (orange)
    pub const ORANGE_50: &'static str = "orange-50";
    pub const ORANGE_500: &'static str = "orange-500";
    pub const ORANGE_600: &'static str = "orange-600";
    pub const ORANGE_700: &'static str = "orange-700";

    // Semantic color aliases
    pub const TEXT_WHITE: &'static str = "white";
    pub const TEXT_BLACK: &'static str = "black";
}

/// Background color utilities - compose bg-{color} classes from color tokens
pub mod bg {
    use super::Colors;
    use const_format::concatcp;

    pub const BLUE_500: &str = concatcp!("bg-", Colors::BLUE_500);
    pub const BLUE_600: &str = concatcp!("bg-", Colors::BLUE_600);
    pub const BLUE_700: &str = concatcp!("bg-", Colors::BLUE_700);

    pub const GRAY_500: &str = concatcp!("bg-", Colors::GRAY_500);
    pub const GRAY_600: &str = concatcp!("bg-", Colors::GRAY_600);
    pub const GRAY_700: &str = concatcp!("bg-", Colors::GRAY_700);

    pub const GREEN_500: &str = concatcp!("bg-", Colors::GREEN_500);
    pub const GREEN_600: &str = concatcp!("bg-", Colors::GREEN_600);
    pub const GREEN_700: &str = concatcp!("bg-", Colors::GREEN_700);

    pub const RED_500: &str = concatcp!("bg-", Colors::RED_500);
    pub const RED_600: &str = concatcp!("bg-", Colors::RED_600);
    pub const RED_700: &str = concatcp!("bg-", Colors::RED_700);

    // shadcn semantic colors
    pub const PRIMARY: &str = "bg-primary";
    pub const PRIMARY_HOVER: &str = "bg-primary/90";
    pub const SECONDARY: &str = "bg-secondary";
    pub const DESTRUCTIVE: &str = "bg-destructive";
    pub const MUTED: &str = "bg-muted";
    pub const ACCENT: &str = "bg-accent";
    pub const BACKGROUND: &str = "bg-background";
    pub const TRANSPARENT: &str = "bg-transparent";
}

/// Text color utilities - compose text-{color} classes from color tokens
pub mod text {
    use super::Colors;
    use const_format::concatcp;

    pub const WHITE: &str = concatcp!("text-", Colors::TEXT_WHITE);
    pub const BLACK: &str = concatcp!("text-", Colors::TEXT_BLACK);

    pub const BLUE_500: &str = concatcp!("text-", Colors::BLUE_500);
    pub const BLUE_600: &str = concatcp!("text-", Colors::BLUE_600);
    pub const BLUE_700: &str = concatcp!("text-", Colors::BLUE_700);

    pub const GRAY_500: &str = concatcp!("text-", Colors::GRAY_500);
    pub const GRAY_600: &str = concatcp!("text-", Colors::GRAY_600);
    pub const GRAY_700: &str = concatcp!("text-", Colors::GRAY_700);

    // shadcn semantic colors
    pub const PRIMARY_FOREGROUND: &str = "text-primary-foreground";
    pub const SECONDARY_FOREGROUND: &str = "text-secondary-foreground";
    pub const DESTRUCTIVE_FOREGROUND: &str = "text-destructive-foreground";
    pub const MUTED_FOREGROUND: &str = "text-muted-foreground";
    pub const ACCENT_FOREGROUND: &str = "text-accent-foreground";
}

/// Hover background utilities - compose hover:bg-{color} classes
pub mod hover_bg {
    use super::Colors;
    use const_format::concatcp;

    pub const BLUE_700: &str = concatcp!("hover:bg-", Colors::BLUE_700);
    pub const GRAY_700: &str = concatcp!("hover:bg-", Colors::GRAY_700);
    pub const GREEN_700: &str = concatcp!("hover:bg-", Colors::GREEN_700);
    pub const RED_700: &str = concatcp!("hover:bg-", Colors::RED_700);

    // shadcn semantic colors
    pub const PRIMARY_90: &str = "hover:bg-primary/90";
    pub const SECONDARY_80: &str = "hover:bg-secondary/80";
    pub const DESTRUCTIVE_80: &str = "hover:bg-destructive/80";
    pub const DESTRUCTIVE_90: &str = "hover:bg-destructive/90";
    pub const MUTED_HOVER: &str = "hover:bg-muted/50";
    pub const ACCENT: &str = "hover:bg-accent";
}

/// Hover text utilities - compose hover:text-{color} classes
pub mod hover_text {
    // shadcn semantic colors
    pub const ACCENT_FOREGROUND: &str = "hover:text-accent-foreground";
    pub const PRIMARY_FOREGROUND: &str = "hover:text-primary-foreground";
    pub const SECONDARY_FOREGROUND: &str = "hover:text-secondary-foreground";
    pub const DESTRUCTIVE_FOREGROUND: &str = "hover:text-destructive-foreground";
}

/// Focus ring utilities - compose focus:ring-{color} classes
pub mod focus_ring {
    use super::Colors;
    use const_format::concatcp;

    pub const BLUE_500: &str = concatcp!("focus:ring-", Colors::BLUE_500);
    pub const GRAY_500: &str = concatcp!("focus:ring-", Colors::GRAY_500);
    pub const GREEN_500: &str = concatcp!("focus:ring-", Colors::GREEN_500);
    pub const RED_500: &str = concatcp!("focus:ring-", Colors::RED_500);
}

/// Border radius utilities - compose rounded-* classes from radius tokens
pub mod rounded {
    pub const NONE: &str = "rounded-none"; // Radii::NONE
    pub const SM: &str = "rounded-xs"; // Radii::SM
    pub const MD: &str = "rounded-md"; // Radii::MD
    pub const LG: &str = "rounded-lg"; // Radii::LG
    pub const FULL: &str = "rounded-full"; // Radii::FULL
}

/// Common layout utilities
pub mod layout {
    pub const INLINE_FLEX: &str = "inline-flex";
    pub const ITEMS_CENTER: &str = "items-center";
    pub const JUSTIFY_CENTER: &str = "justify-center";
}

/// Font size utilities - control font-size and line-height
pub mod font_size {
    pub const TEXT_XS: &str = "text-xs"; // 0.75rem (12px)
    pub const TEXT_SM: &str = "text-sm"; // 0.875rem (14px)
    pub const TEXT_BASE: &str = "text-base"; // 1rem (16px)
    pub const TEXT_LG: &str = "text-lg"; // 1.125rem (18px)
    pub const TEXT_XL: &str = "text-xl"; // 1.25rem (20px)
    pub const TEXT_2XL: &str = "text-2xl"; // 1.5rem (24px)
    pub const TEXT_3XL: &str = "text-3xl"; // 1.875rem (30px)
    pub const TEXT_4XL: &str = "text-4xl"; // 2.25rem (36px)
    pub const TEXT_5XL: &str = "text-5xl"; // 3rem (48px)
    pub const TEXT_6XL: &str = "text-6xl"; // 3.75rem (60px)
    pub const TEXT_7XL: &str = "text-7xl"; // 4.5rem (72px)
    pub const TEXT_8XL: &str = "text-8xl"; // 6rem (96px)
    pub const TEXT_9XL: &str = "text-9xl"; // 8rem (128px)
}

/// Font weight utilities - control font-weight
pub mod font_weight {
    pub const FONT_THIN: &str = "font-thin"; // 100
    pub const FONT_EXTRALIGHT: &str = "font-extralight"; // 200
    pub const FONT_LIGHT: &str = "font-light"; // 300
    pub const FONT_NORMAL: &str = "font-normal"; // 400
    pub const FONT_MEDIUM: &str = "font-medium"; // 500
    pub const FONT_SEMIBOLD: &str = "font-semibold"; // 600
    pub const FONT_BOLD: &str = "font-bold"; // 700
    pub const FONT_EXTRABOLD: &str = "font-extrabold"; // 800
    pub const FONT_BLACK: &str = "font-black"; // 900
}

/// Spacing utilities - padding classes
pub mod padding {
    pub const PX_2: &str = "px-2";
    pub const PX_2_5: &str = "px-2.5";
    pub const PX_3: &str = "px-3";
    pub const PX_4: &str = "px-4";
    pub const PX_6: &str = "px-6";
    pub const PX_8: &str = "px-8";

    pub const PY_1: &str = "py-1";
    pub const PY_1_5: &str = "py-1.5";
    pub const PY_2: &str = "py-2";
    pub const PY_3: &str = "py-3";
    pub const PY_4: &str = "py-4";
}

/// Height utilities
pub mod height {
    pub const H_7: &str = "h-7";
    pub const H_8: &str = "h-8";
    pub const H_9: &str = "h-9";
    pub const H_10: &str = "h-10";
    pub const H_11: &str = "h-11";
}

/// Border utilities
pub mod border {
    pub const BORDER_2: &str = "border-2";
}

/// Effect utilities
pub mod effects {
    pub const SHADOW_NONE: &str = "shadow-none";
    pub const BG_TRANSPARENT: &str = "bg-transparent";
}

/// Transition utilities
pub mod transition {
    pub const TRANSITION_COLORS: &str = "transition-colors";
    pub const TRANSITION_ALL: &str = "transition-all";
}

/// Animation utilities
pub mod animate {
    pub const SPIN: &str = "animate-spin";
    pub const PING: &str = "animate-ping";
    pub const PULSE: &str = "animate-pulse";
    pub const BOUNCE: &str = "animate-bounce";
}

/// Focus utilities
pub mod focus {
    pub const OUTLINE_NONE: &str = "focus:outline-hidden";
    pub const RING_2: &str = "focus:ring-2";
    pub const RING_OFFSET_2: &str = "focus:ring-offset-2";
}

/// Focus-visible utilities (shadcn style)
pub mod focus_visible {
    pub const OUTLINE_NONE: &str = "outline-hidden";
    pub const BORDER_RING: &str = "focus-visible:border-ring";
    pub const RING_RING: &str = "focus-visible:ring-ring/50";
    pub const RING_3: &str = "focus-visible:ring-[3px]";
}

/// State utilities
pub mod state {
    pub const CURSOR_POINTER: &str = "cursor-pointer";
    pub const DISABLED_OPACITY_50: &str = "disabled:opacity-50";
    pub const DISABLED_CURSOR_NOT_ALLOWED: &str = "disabled:cursor-not-allowed";
    pub const DISABLED_POINTER_EVENTS_NONE: &str = "disabled:pointer-events-none";
}

/// Aria-invalid utilities (shadcn style)
pub mod aria_invalid {
    pub const RING_DESTRUCTIVE: &str = "aria-invalid:ring-destructive/20";
    pub const RING_DESTRUCTIVE_DARK: &str = "dark:aria-invalid:ring-destructive/40";
    pub const BORDER_DESTRUCTIVE: &str = "aria-invalid:border-destructive";
}

/// Additional shadcn utilities
pub mod shadcn_utils {
    pub const WHITESPACE_NOWRAP: &str = "whitespace-nowrap";
    pub const SHRINK_0: &str = "shrink-0";
    pub const GAP_1_5: &str = "gap-1.5";
    pub const GAP_2: &str = "gap-2";
    pub const ITEMS_CENTER: &str = "items-center";
    pub const JUSTIFY_CENTER: &str = "justify-center";
    pub const INLINE_FLEX: &str = "inline-flex";

    // SVG styling
    pub const SVG_POINTER_EVENTS_NONE: &str = "[&_svg]:pointer-events-none";
    pub const SVG_SIZE_4: &str = "[&_svg:not([class*='size-'])]:size-4";
    pub const SVG_SHRINK_0: &str = "[&_svg]:shrink-0";

    // Icon button utilities
    pub const HAS_SVG_PX: &str = "has-[>svg]:px-2.5";
}

/// Spacing tokens (padding, margin, gaps)
///
/// Based on Tailwind's spacing scale (0.25rem = 1 unit)
pub struct Spacing;

impl Spacing {
    pub const SPACING_0: &'static str = "0";
    pub const SPACING_0_5: &'static str = "0.125rem"; // 2px
    pub const SPACING_1: &'static str = "0.25rem"; // 4px
    pub const SPACING_1_5: &'static str = "0.375rem"; // 6px
    pub const SPACING_2: &'static str = "0.5rem"; // 8px
    pub const SPACING_2_5: &'static str = "0.625rem"; // 10px
    pub const SPACING_3: &'static str = "0.75rem"; // 12px
    pub const SPACING_3_5: &'static str = "0.875rem"; // 14px
    pub const SPACING_4: &'static str = "1rem"; // 16px
    pub const SPACING_5: &'static str = "1.25rem"; // 20px
    pub const SPACING_6: &'static str = "1.5rem"; // 24px
    pub const SPACING_7: &'static str = "1.75rem"; // 28px
    pub const SPACING_8: &'static str = "2rem"; // 32px
    pub const SPACING_10: &'static str = "2.5rem"; // 40px
    pub const SPACING_12: &'static str = "3rem"; // 48px
    pub const SPACING_16: &'static str = "4rem"; // 64px
    pub const SPACING_20: &'static str = "5rem"; // 80px
    pub const SPACING_24: &'static str = "6rem"; // 96px
}

/// Sizing tokens (width, height)
///
/// Includes spacing values plus additional size-specific tokens
pub struct Sizes;

impl Sizes {
    // Small sizes
    pub const SIZE_XS: &'static str = "20rem"; // 320px
    pub const SIZE_SM: &'static str = "24rem"; // 384px
    pub const SIZE_MD: &'static str = "28rem"; // 448px
    pub const SIZE_LG: &'static str = "32rem"; // 512px
    pub const SIZE_XL: &'static str = "36rem"; // 576px
    pub const SIZE_2XL: &'static str = "42rem"; // 672px
    pub const SIZE_3XL: &'static str = "48rem"; // 768px
    pub const SIZE_4XL: &'static str = "56rem"; // 896px
    pub const SIZE_5XL: &'static str = "64rem"; // 1024px
    pub const SIZE_6XL: &'static str = "72rem"; // 1152px

    // Special sizes
    pub const SIZE_FULL: &'static str = "100%";
    pub const SIZE_SCREEN: &'static str = "100vh";
}

/// Font family tokens
pub struct Fonts;

impl Fonts {
    pub const SANS: &'static str = "ui-sans-serif, system-ui, sans-serif";
    pub const SERIF: &'static str = "ui-serif, Georgia, serif";
    pub const MONO: &'static str = "ui-monospace, monospace";
}

/// Font size tokens
pub struct FontSizes;

impl FontSizes {
    pub const XS: &'static str = "0.75rem"; // 12px
    pub const SM: &'static str = "0.875rem"; // 14px
    pub const BASE: &'static str = "1rem"; // 16px
    pub const LG: &'static str = "1.125rem"; // 18px
    pub const XL: &'static str = "1.25rem"; // 20px
    pub const XL2: &'static str = "1.5rem"; // 24px
    pub const XL3: &'static str = "1.875rem"; // 30px
    pub const XL4: &'static str = "2.25rem"; // 36px
    pub const XL5: &'static str = "3rem"; // 48px
    pub const XL6: &'static str = "3.75rem"; // 60px
    pub const XL7: &'static str = "4.5rem"; // 72px
    pub const XL8: &'static str = "6rem"; // 96px
}

/// Font weight tokens
pub struct FontWeights;

impl FontWeights {
    pub const THIN: u16 = 100;
    pub const EXTRALIGHT: u16 = 200;
    pub const LIGHT: u16 = 300;
    pub const NORMAL: u16 = 400;
    pub const MEDIUM: u16 = 500;
    pub const SEMIBOLD: u16 = 600;
    pub const BOLD: u16 = 700;
    pub const EXTRABOLD: u16 = 800;
    pub const BLACK: u16 = 900;
}

/// Line height tokens
pub struct LineHeights;

impl LineHeights {
    pub const NONE: &'static str = "1";
    pub const TIGHT: &'static str = "1.25";
    pub const SNUG: &'static str = "1.375";
    pub const NORMAL: &'static str = "1.5";
    pub const RELAXED: &'static str = "1.625";
    pub const LOOSE: &'static str = "2";
}

/// Letter spacing tokens
pub struct LetterSpacings;

impl LetterSpacings {
    pub const TIGHTER: &'static str = "-0.05em";
    pub const TIGHT: &'static str = "-0.025em";
    pub const NORMAL: &'static str = "0";
    pub const WIDE: &'static str = "0.025em";
    pub const WIDER: &'static str = "0.05em";
    pub const WIDEST: &'static str = "0.1em";
}

/// Border radius tokens
pub struct Radii;

impl Radii {
    pub const NONE: &'static str = "0";
    pub const SM: &'static str = "0.125rem"; // 2px
    pub const BASE: &'static str = "0.25rem"; // 4px
    pub const MD: &'static str = "0.375rem"; // 6px
    pub const LG: &'static str = "0.5rem"; // 8px
    pub const XL: &'static str = "0.75rem"; // 12px
    pub const XL2: &'static str = "1rem"; // 16px
    pub const XL3: &'static str = "1.5rem"; // 24px
    pub const FULL: &'static str = "9999px";
}

/// Shadow tokens
pub struct Shadows;

impl Shadows {
    pub const XS: &'static str = "0 1px 2px 0 rgb(0 0 0 / 0.05)";
    pub const SM: &'static str = "0 1px 2px 0 rgb(0 0 0 / 0.05)";
    pub const BASE: &'static str = "0 1px 3px 0 rgb(0 0 0 / 0.1), 0 1px 2px -1px rgb(0 0 0 / 0.1)";
    pub const MD: &'static str = "0 4px 6px -1px rgb(0 0 0 / 0.1), 0 2px 4px -2px rgb(0 0 0 / 0.1)";
    pub const LG: &'static str =
        "0 10px 15px -3px rgb(0 0 0 / 0.1), 0 4px 6px -4px rgb(0 0 0 / 0.1)";
    pub const XL: &'static str =
        "0 20px 25px -5px rgb(0 0 0 / 0.1), 0 8px 10px -6px rgb(0 0 0 / 0.1)";
    pub const XL2: &'static str = "0 25px 50px -12px rgb(0 0 0 / 0.25)";
    pub const INNER: &'static str = "inset 0 2px 4px 0 rgb(0 0 0 / 0.05)";
    pub const NONE: &'static str = "0 0 #0000";
}

/// Z-index tokens for layering
pub struct ZIndex;

impl ZIndex {
    pub const HIDE: i32 = -1;
    pub const BASE: i32 = 0;
    pub const DOCKED: i32 = 10;
    pub const DROPDOWN: i32 = 1000;
    pub const STICKY: i32 = 1100;
    pub const BANNER: i32 = 1200;
    pub const OVERLAY: i32 = 1300;
    pub const MODAL: i32 = 1400;
    pub const POPOVER: i32 = 1500;
    pub const SKIPLINK: i32 = 1600;
    pub const TOAST: i32 = 1700;
    pub const TOOLTIP: i32 = 1800;
}

/// Transition duration tokens
pub struct Durations;

impl Durations {
    pub const FASTEST: &'static str = "50ms";
    pub const FASTER: &'static str = "100ms";
    pub const FAST: &'static str = "150ms";
    pub const NORMAL: &'static str = "200ms";
    pub const SLOW: &'static str = "300ms";
    pub const SLOWER: &'static str = "400ms";
    pub const SLOWEST: &'static str = "500ms";
}

/// Easing function tokens
pub struct Easings;

impl Easings {
    pub const EASE_IN: &'static str = "cubic-bezier(0.4, 0, 1, 1)";
    pub const EASE_OUT: &'static str = "cubic-bezier(0, 0, 0.2, 1)";
    pub const EASE_IN_OUT: &'static str = "cubic-bezier(0.4, 0, 0.2, 1)";
    pub const LINEAR: &'static str = "linear";
}

/// Helper macro to build Tailwind class strings from tokens with spaces
///
/// Like `const_format::concatcp!()` but automatically adds spaces between arguments.
/// Perfect for composing multiple Tailwind utility classes.
///
/// # Example
/// ```ignore
/// use crate::tw_join;
/// const CLASSES: &str = tw_join!(bg::BLUE_600, text::WHITE, hover_bg::BLUE_700);
/// // Expands to: concatcp!(bg::BLUE_600, " ", text::WHITE, " ", hover_bg::BLUE_700)
/// ```
#[macro_export]
macro_rules! tw_join {
    ($first:expr $(, $rest:expr)* $(,)?) => {
        const_format::concatcp!($first $(, " ", $rest)*)
    };
}

/// Helper macro to build Tailwind class strings from tokens
///
/// This makes it easier to compose multiple token references into a single class string.
#[macro_export]
macro_rules! tw {
    ($($class:expr),* $(,)?) => {
        concat!($($class, " "),*)
    };
}
