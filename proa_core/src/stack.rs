// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Stack-allocated rendering for maximum performance.
//!
//! This module provides rendering utilities that use stack-allocated buffers
//! via `WriteCursor`, avoiding heap allocation entirely for small to medium pages.
//!
//! # Performance
//!
//! Stack allocation with `WriteCursor` can be faster than heap allocation because:
//! - No allocator overhead (malloc/free)
//! - Better cache locality (stack is typically hot in cache)
//! - Bump allocator style - just increment a cursor position
//! - No potential for allocation failure / OOM (within stack limits)
//!
//! # Usage
//!
//! ```rust
//! use proa_core::stack::{WriteBuf, WriteCursor, WriteError};
//! use proa_core::{DataLoader, WebRenderSync, WebContext, text};
//!
//! struct SmallPage;
//! impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for SmallPage {
//!     fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
//!         text("Hello, stack!").render(cx)
//!     }
//! }
//!
//! // Render to a 4KB stack buffer
//! let mut backing = [0u8; 4096];
//! let mut cursor = WriteCursor::new(&mut backing);
//! {
//!     let mut cx = WebContext::with_buffer(&mut cursor);
//!     SmallPage.render(&mut cx).unwrap();
//! }
//! println!("Rendered {} bytes on stack", cursor.len());
//! ```
//!
//! # Choosing buffer sizes
//!
//! - `1024` (1KB): Simple components, small fragments
//! - `4096` (4KB): Typical small pages, cards, modals
//! - `8192` (8KB): Medium pages
//! - `16384` (16KB): Larger pages with lists
//! - `32768` (32KB): Complex pages (consider heap at this point)
#[cfg(test)]
use crate::render::DataLoader;
#[allow(deprecated)]
pub use crate::shared::write_buf::DefaultChainedBumpWriter;
pub use crate::shared::write_buf::{
    render_growable, BumpWriter, ChainedBumpWriter, FlushMark, Part, RenderBuffer, SmallBuf,
    StagedBuf, StreamingChunk, StreamingChunkPart, StreamingZeroCopyBuf, WriteBuf, WriteCursor,
    WriteError, ZeroCopyBuf, DEFAULT_CHAINED_BUMP_PAGE_SIZE,
};
use crate::{WebContext, WebRenderSync};
/// Render to a `Vec<u8>` with a capacity hint.
///
/// This is a convenience function that pre-allocates the buffer with the given hint.
///
/// # Example
///
/// ```rust
/// use proa_core::stack::{render_into_vec_with_hint, WriteBuf};
/// use proa_core::{DataLoader, WebRenderSync, WebContext};
///
/// struct MyComponent;
/// impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for MyComponent {
///     fn render(self, cx: &mut WebContext<L, B>) -> Result<(), proa_core::WriteError> {
///         cx.out.extend(b"Hello")
///     }
/// }
///
/// let result = render_into_vec_with_hint(MyComponent, 64);
/// assert_eq!(&result, b"Hello");
/// ```
pub fn render_into_vec_with_hint<R: WebRenderSync<::proa_core::NoLoader, Vec<u8>>>(
    root: R,
    hint: usize,
) -> Vec<u8> {
    let mut cx = WebContext::with_buffer(Vec::with_capacity(hint));
    root.render(&mut cx).unwrap();
    cx.into_output()
}
/// Type aliases for common stack buffer sizes (for `WriteCursor`)
pub type StackBuffer1K = [u8; 1024];
pub type StackBuffer4K = [u8; 4096];
pub type StackBuffer8K = [u8; 8192];
pub type StackBuffer16K = [u8; 16384];
pub type StackBuffer32K = [u8; 32768];
/// Type aliases for common `SmallBuf` sizes (inline + heap spill)
pub type SmallBuf4K = SmallBuf<4096>;
pub type SmallBuf8K = SmallBuf<8192>;
pub type SmallBuf16K = SmallBuf<16384>;
pub type SmallBuf32K = SmallBuf<32768>;
#[cfg(test)]
mod tests {
    use super::*;
    use crate::text;
    struct SmallComponent;
    impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for SmallComponent {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            text("Hello, stack!").render(cx)
        }
    }
    #[test]
    fn test_write_cursor_basic() {
        let mut backing: StackBuffer1K = [0u8; 1024];
        let mut cursor = WriteCursor::new(&mut backing);
        cursor.extend(b"<div>").unwrap();
        cursor.extend(b"Hello").unwrap();
        cursor.extend(b"</div>").unwrap();
        assert_eq!(cursor.as_slice(), b"<div>Hello</div>");
        assert_eq!(cursor.len(), 16);
    }
    #[test]
    fn test_write_cursor_overflow() {
        let mut backing = [0u8; 8];
        let mut cursor = WriteCursor::new(&mut backing);
        let result = cursor.extend(b"0123456789");
        assert!(result.is_err());
    }
    #[test]
    fn test_render_to_vec_with_hint() {
        let result = render_into_vec_with_hint(SmallComponent, 64);
        assert_eq!(&result, b"Hello, stack!");
    }
    #[test]
    fn test_render_to_cursor() {
        let mut backing: StackBuffer1K = [0u8; 1024];
        let mut cursor = WriteCursor::new(&mut backing);
        {
            let mut cx = WebContext::with_buffer(&mut cursor);
            SmallComponent.render(&mut cx).unwrap();
        }
        assert_eq!(cursor.as_slice(), b"Hello, stack!");
    }
}
