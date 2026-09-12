// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Zero-allocation class list for component styling
//!
//! `ClassList` stores references to static class strings and writes them
//! directly to the output buffer, avoiding intermediate string concatenation.
use super::write_buf::WriteBuf;
use crate::render::DataLoader;
use crate::render::{WebContext, WebRenderSync};
/// Zero-allocation class list that writes directly to buffer
///
/// All methods except `add_opt` are const fn, allowing compile-time construction
/// of class lists from known variant combinations. At render time, writes each
/// class segment directly to output with space separators.
///
/// # Example
///
/// ```ignore
/// use proa_core::ClassList;
///
/// // Compile-time construction
/// const fn button_classes(size: ButtonSize) -> ClassList<3> {
///     ClassList::new()
///         .add("inline-flex items-center")
///         .add(size.classes())
/// }
///
/// // Runtime: add optional user class
/// let classes = button_classes(ButtonSize::Md)
///     .add_opt(user_class);
/// ```
#[derive(Clone, Copy)]
pub struct ClassList<const N: usize> {
    classes: [&'static str; N],
    count: usize,
}
impl<const N: usize> ClassList<N> {
    /// Create a new empty class list
    #[inline]
    pub const fn new() -> Self {
        Self {
            classes: [""; N],
            count: 0,
        }
    }
    /// Add a class string (const fn)
    ///
    /// The class is always added, even if empty. Use `add_non_empty`
    /// to skip empty strings.
    #[inline]
    pub const fn add(mut self, class: &'static str) -> Self {
        assert!(
            self.count < N,
            "ClassList overflow: added more classes than capacity N"
        );
        self.classes[self.count] = class;
        self.count += 1;
        self
    }
    /// Add a class string only if non-empty (const fn)
    ///
    /// Useful for variant classes that may be empty (e.g., "Solid" variant
    /// that relies on color classes instead of its own).
    #[inline]
    pub const fn add_non_empty(self, class: &'static str) -> Self {
        if class.is_empty() {
            self
        } else {
            self.add(class)
        }
    }
    /// Add a class conditionally (const fn)
    #[inline]
    pub const fn add_if(self, condition: bool, class: &'static str) -> Self {
        if condition {
            self.add(class)
        } else {
            self
        }
    }
    /// Add an optional class at runtime
    ///
    /// This is the only non-const method - used for user-provided classes
    /// that aren't known at compile time.
    #[inline]
    pub fn add_opt(mut self, class: Option<&'static str>) -> Self {
        if let Some(c) = class {
            if !c.is_empty() {
                self.classes[self.count] = c;
                self.count += 1;
            }
        }
        self
    }
    /// Get the number of class strings
    #[inline]
    pub const fn len(&self) -> usize {
        self.count
    }
    /// Check if empty
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }
    /// Get the classes as a slice
    #[inline]
    pub fn as_slice(&self) -> &[&'static str] {
        &self.classes[..self.count]
    }
}
impl<const N: usize> Default for ClassList<N> {
    fn default() -> Self {
        Self::new()
    }
}
/// Implement WebRender so ClassList can be used in html! macro class attributes
impl<B: WriteBuf, L: DataLoader, const N: usize> WebRenderSync<L, B> for ClassList<N> {
    #[inline(always)]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), crate::shared::write_buf::WriteError> {
        for (i, class) in self.classes[..self.count].iter().enumerate() {
            if i > 0 {
                cx.out.extend(b" ")?;
            }
            cx.out.extend(class.as_bytes())?;
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn class_list_basic() {
        let list = ClassList::<4>::new()
            .add("flex")
            .add("items-center")
            .add("gap-2");
        assert_eq!(list.len(), 3);
        assert_eq!(list.as_slice(), &["flex", "items-center", "gap-2"]);
    }
    #[test]
    fn class_list_add_non_empty() {
        let list = ClassList::<4>::new()
            .add("base")
            .add_non_empty("")
            .add_non_empty("variant");
        assert_eq!(list.len(), 2);
        assert_eq!(list.as_slice(), &["base", "variant"]);
    }
    #[test]
    fn class_list_add_opt() {
        let list = ClassList::<4>::new()
            .add("base")
            .add_opt(None)
            .add_opt(Some("custom"));
        assert_eq!(list.len(), 2);
        assert_eq!(list.as_slice(), &["base", "custom"]);
    }
    #[test]
    fn class_list_add_if() {
        let list = ClassList::<4>::new()
            .add("base")
            .add_if(false, "skipped")
            .add_if(true, "included");
        assert_eq!(list.len(), 2);
        assert_eq!(list.as_slice(), &["base", "included"]);
    }
    #[test]
    fn class_list_renders_with_spaces() {
        let list = ClassList::<3>::new()
            .add("flex")
            .add("items-center")
            .add("gap-2");
        let mut buf = Vec::new();
        let mut cx = WebContext::with_buffer(&mut buf);
        list.render(&mut cx).unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "flex items-center gap-2");
    }
    #[test]
    fn class_list_const_construction() {
        const LIST: ClassList<2> = ClassList::new().add("a").add("b");
        assert_eq!(LIST.len(), 2);
    }
}
