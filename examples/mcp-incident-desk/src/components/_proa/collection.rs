// SPDX-FileCopyrightText: 2026 Proa Labs, Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Shared, allocation-free collection traversal for compound controls.
//!
//! Select, Combobox, Listbox, Menu, Tags Input, and Tree View should share
//! ordering, disabled-item traversal, typeahead, and tree-path semantics rather
//! than reimplementing them in each browser handler.

use core::ops::ControlFlow;

/// The item contract used by list-shaped components.
pub trait ListCollectionItem {
    type Value: PartialEq + ?Sized;

    fn value(&self) -> &Self::Value;
    fn text(&self) -> &str;

    fn disabled(&self) -> bool {
        false
    }

    fn group(&self) -> Option<&str> {
        None
    }
}

/// A borrowed collection with shared traversal and typeahead semantics.
#[derive(Debug, Clone, Copy)]
pub struct ListCollection<'a, T> {
    items: &'a [T],
}

impl<'a, T> ListCollection<'a, T> {
    pub const fn new(items: &'a [T]) -> Self {
        Self { items }
    }

    pub const fn items(&self) -> &'a [T] {
        self.items
    }

    pub const fn len(&self) -> usize {
        self.items.len()
    }

    pub const fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn get(&self, index: usize) -> Option<&'a T> {
        self.items.get(index)
    }

    pub fn iter(&self) -> core::slice::Iter<'a, T> {
        self.items.iter()
    }
}

impl<'a, T: ListCollectionItem> ListCollection<'a, T> {
    /// Resolve a collection value to its item.
    pub fn find(&self, value: &T::Value) -> Option<&'a T> {
        self.item(value)
    }

    pub fn index_of(&self, value: &T::Value) -> Option<usize> {
        self.items.iter().position(|item| item.value() == value)
    }

    pub fn item(&self, value: &T::Value) -> Option<&'a T> {
        self.index_of(value).and_then(|index| self.items.get(index))
    }

    pub fn has(&self, value: &T::Value) -> bool {
        self.index_of(value).is_some()
    }

    /// Resolve several values without building an intermediate collection.
    pub fn find_many<'b>(&'b self, values: &'b [&'b T::Value]) -> impl Iterator<Item = &'a T> + 'b {
        values.iter().filter_map(|value| self.item(*value))
    }

    pub fn first_enabled(&self) -> Option<(usize, &'a T)> {
        self.items
            .iter()
            .enumerate()
            .find(|(_, item)| !item.disabled())
    }

    pub fn last_enabled(&self) -> Option<(usize, &'a T)> {
        self.items
            .iter()
            .enumerate()
            .rev()
            .find(|(_, item)| !item.disabled())
    }

    pub fn first_value(&self) -> Option<&'a T::Value> {
        self.first_enabled().map(|(_, item)| item.value())
    }

    pub fn last_value(&self) -> Option<&'a T::Value> {
        self.last_enabled().map(|(_, item)| item.value())
    }

    /// Find the next enabled item after `index`.
    ///
    /// When `wrap` is true, traversal continues at the beginning. The current
    /// item is never returned, even when it is the only enabled item.
    pub fn next_enabled(&self, index: usize, wrap: bool) -> Option<(usize, &'a T)> {
        if self.items.is_empty() || index >= self.items.len() {
            return None;
        }

        for candidate in (index + 1)..self.items.len() {
            if !self.items[candidate].disabled() {
                return Some((candidate, &self.items[candidate]));
            }
        }

        if wrap {
            for candidate in 0..index {
                if !self.items[candidate].disabled() {
                    return Some((candidate, &self.items[candidate]));
                }
            }
        }

        None
    }

    /// Find the previous enabled item before `index`.
    pub fn previous_enabled(&self, index: usize, wrap: bool) -> Option<(usize, &'a T)> {
        if self.items.is_empty() || index >= self.items.len() {
            return None;
        }

        for candidate in (0..index).rev() {
            if !self.items[candidate].disabled() {
                return Some((candidate, &self.items[candidate]));
            }
        }

        if wrap {
            for candidate in ((index + 1)..self.items.len()).rev() {
                if !self.items[candidate].disabled() {
                    return Some((candidate, &self.items[candidate]));
                }
            }
        }

        None
    }

    pub fn next_value(&self, value: &T::Value, wrap: bool) -> Option<&'a T::Value> {
        self.index_of(value)
            .and_then(|index| self.next_enabled(index, wrap))
            .map(|(_, item)| item.value())
    }

    pub fn previous_value(&self, value: &T::Value, wrap: bool) -> Option<&'a T::Value> {
        self.index_of(value)
            .and_then(|index| self.previous_enabled(index, wrap))
            .map(|(_, item)| item.value())
    }

    /// Match an enabled item by case-insensitive ASCII prefix.
    ///
    /// Search begins after `start_after`, wraps once, and avoids allocating a
    /// lowercase copy of either string.
    pub fn typeahead(&self, query: &str, start_after: Option<usize>) -> Option<(usize, &'a T)> {
        if query.is_empty() || self.items.is_empty() {
            return None;
        }

        let start = start_after
            .filter(|index| *index < self.items.len())
            .map_or(0, |index| (index + 1) % self.items.len());

        for offset in 0..self.items.len() {
            let index = (start + offset) % self.items.len();
            let item = &self.items[index];
            if !item.disabled() && starts_with_ignore_ascii_case(item.text(), query) {
                return Some((index, item));
            }
        }

        None
    }

    pub fn enabled(&self) -> impl Iterator<Item = (usize, &'a T)> {
        self.items
            .iter()
            .enumerate()
            .filter(|(_, item)| !item.disabled())
    }
}

/// Mutable view for collection reordering without reallocating.
#[derive(Debug)]
pub struct ListCollectionMut<'a, T> {
    items: &'a mut [T],
}

impl<'a, T> ListCollectionMut<'a, T> {
    pub fn new(items: &'a mut [T]) -> Self {
        Self { items }
    }

    pub fn items(&self) -> &[T] {
        self.items
    }

    pub fn as_collection(&self) -> ListCollection<'_, T> {
        ListCollection::new(self.items)
    }

    /// Move one item to another index while preserving all other ordering.
    ///
    /// Returns `false` when either index is outside the collection.
    pub fn reorder(&mut self, from: usize, to: usize) -> bool {
        if from >= self.items.len() || to >= self.items.len() {
            return false;
        }
        if from < to {
            self.items[from..=to].rotate_left(1);
        } else if from > to {
            self.items[to..=from].rotate_right(1);
        }
        true
    }
}

fn starts_with_ignore_ascii_case(value: &str, prefix: &str) -> bool {
    value
        .get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
}

/// The node contract used by tree-shaped components.
pub trait TreeCollectionItem: Sized {
    type Value: PartialEq + ?Sized;

    fn value(&self) -> &Self::Value;
    fn text(&self) -> &str;
    fn children(&self) -> &[Self];

    fn disabled(&self) -> bool {
        false
    }
}

/// A borrowed tree collection with path and relationship queries.
#[derive(Debug, Clone, Copy)]
pub struct TreeCollection<'a, T> {
    roots: &'a [T],
}

impl<'a, T> TreeCollection<'a, T> {
    pub const fn new(roots: &'a [T]) -> Self {
        Self { roots }
    }

    pub const fn roots(&self) -> &'a [T] {
        self.roots
    }

    pub const fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }
}

impl<'a, T: TreeCollectionItem> TreeCollection<'a, T> {
    pub fn item(&self, value: &T::Value) -> Option<&'a T> {
        find_item(self.roots, value)
    }

    pub fn parent(&self, value: &T::Value) -> Option<&'a T> {
        find_parent(self.roots, value, None)
    }

    pub fn depth(&self, value: &T::Value) -> Option<usize> {
        find_depth(self.roots, value, 0)
    }

    pub fn is_descendant(&self, ancestor: &T::Value, candidate: &T::Value) -> bool {
        self.item(ancestor)
            .is_some_and(|node| find_item(node.children(), candidate).is_some())
    }

    pub fn item_at_path(&self, path: &[usize]) -> Option<&'a T> {
        let (&first, rest) = path.split_first()?;
        let mut current = self.roots.get(first)?;
        for &index in rest {
            current = current.children().get(index)?;
        }
        Some(current)
    }

    /// Return an index path without heap allocation.
    pub fn path_of<const N: usize>(&self, value: &T::Value) -> Option<heapless::Vec<usize, N>> {
        let mut path = heapless::Vec::new();
        find_path(self.roots, value, &mut path).then_some(path)
    }

    /// Visit every node in depth-first order until the callback breaks.
    pub fn visit(
        &self,
        mut visitor: impl FnMut(usize, &'a T) -> ControlFlow<()>,
    ) -> ControlFlow<()> {
        visit_nodes(self.roots, 0, &mut visitor)
    }
}

fn find_item<'a, T: TreeCollectionItem>(nodes: &'a [T], value: &T::Value) -> Option<&'a T> {
    for node in nodes {
        if node.value() == value {
            return Some(node);
        }
        if let Some(found) = find_item(node.children(), value) {
            return Some(found);
        }
    }
    None
}

fn find_parent<'a, T: TreeCollectionItem>(
    nodes: &'a [T],
    value: &T::Value,
    parent: Option<&'a T>,
) -> Option<&'a T> {
    for node in nodes {
        if node.value() == value {
            return parent;
        }
        if let Some(found) = find_parent(node.children(), value, Some(node)) {
            return Some(found);
        }
    }
    None
}

fn find_depth<T: TreeCollectionItem>(nodes: &[T], value: &T::Value, depth: usize) -> Option<usize> {
    for node in nodes {
        if node.value() == value {
            return Some(depth);
        }
        if let Some(found) = find_depth(node.children(), value, depth + 1) {
            return Some(found);
        }
    }
    None
}

fn find_path<T: TreeCollectionItem, const N: usize>(
    nodes: &[T],
    value: &T::Value,
    path: &mut heapless::Vec<usize, N>,
) -> bool {
    for (index, node) in nodes.iter().enumerate() {
        if path.push(index).is_err() {
            return false;
        }
        if node.value() == value || find_path(node.children(), value, path) {
            return true;
        }
        path.pop();
    }
    false
}

fn visit_nodes<'a, T: TreeCollectionItem>(
    nodes: &'a [T],
    depth: usize,
    visitor: &mut impl FnMut(usize, &'a T) -> ControlFlow<()>,
) -> ControlFlow<()> {
    for node in nodes {
        visitor(depth, node)?;
        visit_nodes(node.children(), depth + 1, visitor)?;
    }
    ControlFlow::Continue(())
}
