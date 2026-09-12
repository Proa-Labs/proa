// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Trigger and Effect definitions
//!
//! Triggers define when effects should run. Effects are blocks of statements
//! that execute in response to triggers.

use crate::expr::Expr;
use crate::stmt::Block;
use crate::types::{FieldId, Selector, SignalId};
use serde::Serialize;

/// An effect - code that runs in response to a trigger
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Effect {
    /// When this effect should run
    pub trigger: Trigger,
    /// Optional condition - effect only runs if this is true
    #[serde(skip_serializing_if = "Option::is_none")]
    pub when: Option<Expr>,
    /// The code to execute
    pub body: Block,
}

impl Effect {
    pub fn new(trigger: Trigger, body: Block) -> Self {
        Self {
            trigger,
            when: None,
            body,
        }
    }

    pub fn with_condition(mut self, cond: Expr) -> Self {
        self.when = Some(cond);
        self
    }
}

/// What triggers an effect to run
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Trigger {
    // =========================================================================
    // Lifecycle
    // =========================================================================
    /// Run when the form/component mounts
    Mount,

    /// Run when the form/component unmounts
    Unmount,

    // =========================================================================
    // Form Field Events
    // =========================================================================
    /// Run when a field receives input (every keystroke)
    Input { field: FieldId },

    /// Run when a field value changes (on blur for text, immediately for select/checkbox)
    Change { field: FieldId },

    /// Run when a field loses focus
    Blur { field: FieldId },

    /// Run when a field gains focus
    Focus { field: FieldId },

    /// Run when a field is validated
    Validate { field: FieldId },

    // =========================================================================
    // Form Events
    // =========================================================================
    /// Run when form is submitted
    Submit,

    /// Run when form is reset
    Reset,

    /// Run when form validity changes
    ValidityChange,

    // =========================================================================
    // Element Events
    // =========================================================================
    /// Run when element is clicked
    Click { selector: Selector },

    /// Run on double click
    DblClick { selector: Selector },

    /// Run on mouse enter
    MouseEnter { selector: Selector },

    /// Run on mouse leave
    MouseLeave { selector: Selector },

    /// Run on key down
    KeyDown {
        selector: Selector,
        #[serde(skip_serializing_if = "Option::is_none")]
        key: Option<KeyFilter>,
    },

    /// Run on key up
    KeyUp {
        selector: Selector,
        #[serde(skip_serializing_if = "Option::is_none")]
        key: Option<KeyFilter>,
    },

    /// Run on key press (deprecated but still used)
    KeyPress {
        selector: Selector,
        #[serde(skip_serializing_if = "Option::is_none")]
        key: Option<KeyFilter>,
    },

    // =========================================================================
    // Reactive
    // =========================================================================
    /// Run when a signal's value changes
    SignalChange { signal: SignalId },

    /// Run when any of the given signals change
    SignalsChange { signals: Vec<SignalId> },

    /// Run when a derived expression's value changes
    Computed { expr: Expr },

    // =========================================================================
    // Window/Document Events
    // =========================================================================
    /// Run when window gains focus
    WindowFocus,

    /// Run when window loses focus
    WindowBlur,

    /// Run when page becomes visible
    Visible,

    /// Run when page becomes hidden
    Hidden,

    /// Run when browser goes online
    Online,

    /// Run when browser goes offline
    Offline,

    /// Run on window resize (debounced by default)
    Resize,

    /// Run on scroll (debounced by default)
    Scroll { selector: Option<Selector> },

    // =========================================================================
    // Timers
    // =========================================================================
    /// Run at interval
    Interval { ms: u32 },

    /// Run after timeout (one-shot)
    Timeout { ms: u32 },

    // =========================================================================
    // Custom
    // =========================================================================
    /// Run when a custom event is dispatched
    CustomEvent { event: &'static str },
}

/// Filter for keyboard events
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct KeyFilter {
    /// The key to match (e.g., "Enter", "Escape", "a")
    pub key: &'static str,
    /// Require Ctrl/Cmd key
    #[serde(default, skip_serializing_if = "is_false")]
    pub ctrl: bool,
    /// Require Shift key
    #[serde(default, skip_serializing_if = "is_false")]
    pub shift: bool,
    /// Require Alt key
    #[serde(default, skip_serializing_if = "is_false")]
    pub alt: bool,
    /// Require Meta key (Cmd on Mac, Win on Windows)
    #[serde(default, skip_serializing_if = "is_false")]
    pub meta: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl KeyFilter {
    pub const fn new(key: &'static str) -> Self {
        Self {
            key,
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
        }
    }

    pub const fn ctrl(mut self) -> Self {
        self.ctrl = true;
        self
    }

    pub const fn shift(mut self) -> Self {
        self.shift = true;
        self
    }

    pub const fn alt(mut self) -> Self {
        self.alt = true;
        self
    }

    pub const fn meta(mut self) -> Self {
        self.meta = true;
        self
    }
}

// Convenience constructors for Trigger
impl Trigger {
    pub fn on_input(field: &'static str) -> Self {
        Self::Input {
            field: FieldId::new(field),
        }
    }

    pub fn on_change(field: &'static str) -> Self {
        Self::Change {
            field: FieldId::new(field),
        }
    }

    pub fn on_blur(field: &'static str) -> Self {
        Self::Blur {
            field: FieldId::new(field),
        }
    }

    pub fn on_focus(field: &'static str) -> Self {
        Self::Focus {
            field: FieldId::new(field),
        }
    }

    pub fn on_click(selector: &'static str) -> Self {
        Self::Click {
            selector: Selector::new(selector),
        }
    }

    pub fn on_signal(signal: &'static str) -> Self {
        Self::SignalChange {
            signal: SignalId::new(signal),
        }
    }

    pub fn on_key(selector: &'static str, key: &'static str) -> Self {
        Self::KeyDown {
            selector: Selector::new(selector),
            key: Some(KeyFilter::new(key)),
        }
    }

    pub fn on_enter(selector: &'static str) -> Self {
        Self::KeyDown {
            selector: Selector::new(selector),
            key: Some(KeyFilter::new("Enter")),
        }
    }

    pub fn on_escape(selector: &'static str) -> Self {
        Self::KeyDown {
            selector: Selector::new(selector),
            key: Some(KeyFilter::new("Escape")),
        }
    }
}
