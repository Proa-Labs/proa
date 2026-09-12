// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Design tokens for the Proa UI system.
//!
//! Design tokens are the core visual primitives that define your design system:
//! colors, spacing, typography, shadows, etc.
//!
//! These tokens integrate seamlessly with the recipe system to provide
//! type-safe, semantic styling with Tailwind output.

/// Size scale for components (spacing, sizing, etc.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    /// Extra small: xs
    Xs,
    /// Small: sm
    Sm,
    /// Medium: md (default)
    Md,
    /// Large: lg
    Lg,
    /// Extra large: xl
    Xl,
}

impl Size {
    /// Get the CSS class suffix for this size
    pub const fn as_str(&self) -> &'static str {
        match self {
            Size::Xs => "xs",
            Size::Sm => "sm",
            Size::Md => "md",
            Size::Lg => "lg",
            Size::Xl => "xl",
        }
    }

    /// Get spacing value in rem
    pub const fn spacing(&self) -> &'static str {
        match self {
            Size::Xs => "0.5rem",
            Size::Sm => "0.75rem",
            Size::Md => "1rem",
            Size::Lg => "1.5rem",
            Size::Xl => "2rem",
        }
    }

    /// Get Tailwind padding classes for this size (horizontal + vertical)
    pub const fn padding_classes(&self) -> &'static [&'static str] {
        match self {
            Size::Xs => &["px-2", "py-1"],
            Size::Sm => &["px-3", "py-1.5"],
            Size::Md => &["px-4", "py-2"],
            Size::Lg => &["px-6", "py-3"],
            Size::Xl => &["px-8", "py-4"],
        }
    }

    /// Get Tailwind text size class for this size
    pub const fn text_class(&self) -> &'static str {
        match self {
            Size::Xs => "text-xs",
            Size::Sm => "text-sm",
            Size::Md => "text-base",
            Size::Lg => "text-lg",
            Size::Xl => "text-xl",
        }
    }
}

/// Color scheme for components
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorScheme {
    /// Primary brand color
    Primary,
    /// Secondary brand color
    Secondary,
    /// Success state (green)
    Success,
    /// Warning state (yellow/orange)
    Warning,
    /// Danger/error state (red)
    Danger,
    /// Neutral/gray
    Neutral,
}

impl ColorScheme {
    pub const fn as_str(&self) -> &'static str {
        match self {
            ColorScheme::Primary => "primary",
            ColorScheme::Secondary => "secondary",
            ColorScheme::Success => "success",
            ColorScheme::Warning => "warning",
            ColorScheme::Danger => "danger",
            ColorScheme::Neutral => "neutral",
        }
    }

    /// Get Tailwind solid button classes for this color scheme
    pub const fn solid_button_classes(&self) -> &'static [&'static str] {
        match self {
            ColorScheme::Primary => &[
                "bg-blue-600",
                "text-white",
                "hover:bg-blue-700",
                "focus:ring-blue-500",
            ],
            ColorScheme::Secondary => &[
                "bg-gray-600",
                "text-white",
                "hover:bg-gray-700",
                "focus:ring-gray-500",
            ],
            ColorScheme::Success => &[
                "bg-green-600",
                "text-white",
                "hover:bg-green-700",
                "focus:ring-green-500",
            ],
            ColorScheme::Warning => &[
                "bg-orange-600",
                "text-white",
                "hover:bg-orange-700",
                "focus:ring-orange-500",
            ],
            ColorScheme::Danger => &[
                "bg-red-600",
                "text-white",
                "hover:bg-red-700",
                "focus:ring-red-500",
            ],
            ColorScheme::Neutral => &[
                "bg-gray-600",
                "text-white",
                "hover:bg-gray-700",
                "focus:ring-gray-500",
            ],
        }
    }

    /// Get Tailwind outline button classes for this color scheme
    pub const fn outline_button_classes(&self) -> &'static [&'static str] {
        match self {
            ColorScheme::Primary => &["border-blue-600", "text-blue-600", "hover:bg-blue-50"],
            ColorScheme::Secondary => &["border-gray-600", "text-gray-600", "hover:bg-gray-50"],
            ColorScheme::Success => &["border-green-600", "text-green-600", "hover:bg-green-50"],
            ColorScheme::Warning => &["border-orange-600", "text-orange-600", "hover:bg-orange-50"],
            ColorScheme::Danger => &["border-red-600", "text-red-600", "hover:bg-red-50"],
            ColorScheme::Neutral => &["border-gray-600", "text-gray-600", "hover:bg-gray-50"],
        }
    }

    /// Get Tailwind ghost button classes for this color scheme
    pub const fn ghost_button_classes(&self) -> &'static [&'static str] {
        match self {
            ColorScheme::Primary => &["text-blue-600", "hover:bg-blue-50"],
            ColorScheme::Secondary => &["text-gray-600", "hover:bg-gray-50"],
            ColorScheme::Success => &["text-green-600", "hover:bg-green-50"],
            ColorScheme::Warning => &["text-orange-600", "hover:bg-orange-50"],
            ColorScheme::Danger => &["text-red-600", "hover:bg-red-50"],
            ColorScheme::Neutral => &["text-gray-600", "hover:bg-gray-50"],
        }
    }

    /// Get background color class
    pub const fn bg_class(&self) -> &'static str {
        match self {
            ColorScheme::Primary => "bg-blue-600",
            ColorScheme::Secondary => "bg-gray-600",
            ColorScheme::Success => "bg-green-600",
            ColorScheme::Warning => "bg-orange-600",
            ColorScheme::Danger => "bg-red-600",
            ColorScheme::Neutral => "bg-gray-600",
        }
    }

    /// Get text color class
    pub const fn text_class(&self) -> &'static str {
        match self {
            ColorScheme::Primary => "text-blue-600",
            ColorScheme::Secondary => "text-gray-600",
            ColorScheme::Success => "text-green-600",
            ColorScheme::Warning => "text-orange-600",
            ColorScheme::Danger => "text-red-600",
            ColorScheme::Neutral => "text-gray-600",
        }
    }
}

/// Border radius variants
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Radius {
    /// No rounding
    None,
    /// Small rounding
    Sm,
    /// Medium rounding (default)
    Md,
    /// Large rounding
    Lg,
    /// Full rounding (pill shape)
    Full,
}

impl Radius {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Radius::None => "none",
            Radius::Sm => "sm",
            Radius::Md => "md",
            Radius::Lg => "lg",
            Radius::Full => "full",
        }
    }

    pub const fn value(&self) -> &'static str {
        match self {
            Radius::None => "0",
            Radius::Sm => "0.25rem",
            Radius::Md => "0.5rem",
            Radius::Lg => "1rem",
            Radius::Full => "9999px",
        }
    }

    /// Get Tailwind rounded class for this radius
    pub const fn tailwind_class(&self) -> &'static str {
        match self {
            Radius::None => "rounded-none",
            Radius::Sm => "rounded-xs",
            Radius::Md => "rounded-md",
            Radius::Lg => "rounded-lg",
            Radius::Full => "rounded-full",
        }
    }
}

/// Typography variants
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextSize {
    Xs,
    Sm,
    Base,
    Lg,
    Xl,
    Xl2,
    Xl3,
    Xl4,
}

impl TextSize {
    pub const fn as_str(&self) -> &'static str {
        match self {
            TextSize::Xs => "xs",
            TextSize::Sm => "sm",
            TextSize::Base => "base",
            TextSize::Lg => "lg",
            TextSize::Xl => "xl",
            TextSize::Xl2 => "2xl",
            TextSize::Xl3 => "3xl",
            TextSize::Xl4 => "4xl",
        }
    }

    /// Get Tailwind text size class
    pub const fn tailwind_class(&self) -> &'static str {
        match self {
            TextSize::Xs => "text-xs",
            TextSize::Sm => "text-sm",
            TextSize::Base => "text-base",
            TextSize::Lg => "text-lg",
            TextSize::Xl => "text-xl",
            TextSize::Xl2 => "text-2xl",
            TextSize::Xl3 => "text-3xl",
            TextSize::Xl4 => "text-4xl",
        }
    }
}

/// Font weight variants
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontWeight {
    Normal,
    Medium,
    Semibold,
    Bold,
}

impl FontWeight {
    pub const fn as_str(&self) -> &'static str {
        match self {
            FontWeight::Normal => "normal",
            FontWeight::Medium => "medium",
            FontWeight::Semibold => "semibold",
            FontWeight::Bold => "bold",
        }
    }

    pub const fn value(&self) -> u16 {
        match self {
            FontWeight::Normal => 400,
            FontWeight::Medium => 500,
            FontWeight::Semibold => 600,
            FontWeight::Bold => 700,
        }
    }

    /// Get Tailwind font weight class
    pub const fn tailwind_class(&self) -> &'static str {
        match self {
            FontWeight::Normal => "font-normal",
            FontWeight::Medium => "font-medium",
            FontWeight::Semibold => "font-semibold",
            FontWeight::Bold => "font-bold",
        }
    }
}
