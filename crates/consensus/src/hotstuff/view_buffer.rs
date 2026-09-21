//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::{BTreeMap, VecDeque, btree_map::Entry},
    fmt::Display,
    mem,
};

use tari_ootle_common_types::{Epoch, NodeHeight};

/// A consensus view, ordered by epoch then height.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct View {
    pub epoch: Epoch,
    pub height: NodeHeight,
}

impl View {
    pub fn new(epoch: Epoch, height: NodeHeight) -> Self {
        Self { epoch, height }
    }
}

impl Display for View {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.epoch, self.height)
    }
}

/// What an [`insert`](ViewBuffer::insert) did to make room for the new item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Inserted {
    /// Items dropped from the furthest views to admit this one.
    pub num_evicted: usize,
}

/// A buffer of items belonging to views we have not reached yet, keyed by view and FIFO within a view.
///
/// Two budgets bound it, both across every view at once: the size the items hold, and how many there are. A
/// per-view budget would bound nothing, because the view an item names is chosen by its sender. Size is the
/// primary budget — items range from a vote to a block-sized proposal, so a count that is generous to ordinary
/// traffic admits that many of the largest items; the count is the secondary guard, covering the per-item
/// overhead that size alone misses.
///
/// Within either budget, the item furthest in the future is evicted to make room for a nearer one, and an item
/// that is itself the furthest is refused. Consensus consumes views in order, so the nearest views are the
/// ones it is about to need and a flood aimed at distant views cannot crowd them out. Views are ordered
/// epoch-major, so everything in the next epoch counts as further away than anything in this one: a caller
/// that admits arbitrary heights within the current epoch must bound them itself.
pub struct ViewBuffer<T> {
    buffer: BTreeMap<View, VecDeque<(usize, T)>>,
    len: usize,
    size: usize,
    max_items: usize,
    max_size: usize,
}

impl<T> ViewBuffer<T> {
    /// Creates a buffer holding at most `max_items` items totalling at most `max_size`.
    ///
    /// # Panics
    /// Panics if either budget is zero.
    pub fn new(max_items: usize, max_size: usize) -> Self {
        assert!(max_items > 0, "ViewBuffer max_items must be non-zero");
        assert!(max_size > 0, "ViewBuffer max_size must be non-zero");
        Self {
            buffer: BTreeMap::new(),
            len: 0,
            size: 0,
            max_items,
            max_size,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn max_items(&self) -> usize {
        self.max_items
    }

    pub fn max_size(&self) -> usize {
        self.max_size
    }

    /// Buffers `item` for `view`, charging `size` against the size budget. Returns `Err(item)` if the items
    /// held for views at or nearer than `view` already fill a budget, or if `size` alone exceeds the budget.
    pub fn insert(&mut self, view: View, item: T, size: usize) -> Result<Inserted, T> {
        if size > self.max_size {
            return Err(item);
        }

        let mut num_evicted = 0;
        while self.len >= self.max_items || self.size.saturating_add(size) > self.max_size {
            let Some(mut furthest) = self.buffer.last_entry() else {
                break;
            };
            if *furthest.key() <= view {
                return Err(item);
            }
            let Some((evicted_size, _)) = furthest.get_mut().pop_back() else {
                break;
            };
            self.len -= 1;
            self.size -= evicted_size;
            num_evicted += 1;
            if furthest.get().is_empty() {
                furthest.remove();
            }
        }

        self.buffer.entry(view).or_default().push_back((size, item));
        self.len += 1;
        self.size += size;
        Ok(Inserted { num_evicted })
    }

    /// Removes and returns the oldest item buffered for `view`.
    pub fn pop_front(&mut self, view: &View) -> Option<T> {
        let Entry::Occupied(mut entry) = self.buffer.entry(*view) else {
            return None;
        };
        let (size, item) = entry.get_mut().pop_front()?;
        self.len -= 1;
        self.size -= size;
        if entry.get().is_empty() {
            entry.remove();
        }
        Some(item)
    }

    /// Drops every item buffered for a view before `view`, returning the number dropped.
    pub fn discard_before(&mut self, view: View) -> usize {
        let mut discarded = mem::take(&mut self.buffer);
        self.buffer = discarded.split_off(&view);
        let mut num_discarded = 0;
        for (size, _) in discarded.into_values().flatten() {
            num_discarded += 1;
            self.size -= size;
        }
        self.len -= num_discarded;
        num_discarded
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
        self.len = 0;
        self.size = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NO_SIZE: usize = 0;
    const UNLIMITED: usize = usize::MAX;

    fn view(height: u64) -> View {
        View::new(Epoch(1), NodeHeight(height))
    }

    fn insert(buffer: &mut ViewBuffer<&'static str>, view: View, item: &'static str) -> Result<Inserted, &'static str> {
        buffer.insert(view, item, NO_SIZE)
    }

    #[test]
    fn it_pops_items_for_a_view_in_order() {
        let mut buffer = ViewBuffer::new(10, UNLIMITED);
        insert(&mut buffer, view(1), "a").unwrap();
        insert(&mut buffer, view(2), "b").unwrap();
        insert(&mut buffer, view(1), "c").unwrap();

        assert_eq!(buffer.pop_front(&view(1)), Some("a"));
        assert_eq!(buffer.pop_front(&view(1)), Some("c"));
        assert_eq!(buffer.pop_front(&view(1)), None);
        assert_eq!(buffer.pop_front(&view(2)), Some("b"));
        assert_eq!(buffer.len(), 0);
    }

    #[test]
    fn it_discards_items_before_a_view() {
        let mut buffer = ViewBuffer::new(10, UNLIMITED);
        buffer.insert(view(1), "a", 10).unwrap();
        buffer.insert(view(1), "b", 10).unwrap();
        buffer.insert(view(2), "c", 10).unwrap();
        buffer.insert(View::new(Epoch(2), NodeHeight(1)), "d", 10).unwrap();

        assert_eq!(buffer.discard_before(view(2)), 2);
        assert_eq!(buffer.len(), 2);
        assert_eq!(buffer.size(), 20);
        assert_eq!(buffer.pop_front(&view(1)), None);
        assert_eq!(buffer.pop_front(&view(2)), Some("c"));
    }

    #[test]
    fn a_single_view_cannot_exceed_the_item_budget() {
        let mut buffer = ViewBuffer::new(3, UNLIMITED);
        for i in 0..3 {
            buffer.insert(view(1), i, NO_SIZE).unwrap();
        }
        assert_eq!(buffer.insert(view(1), 3, NO_SIZE), Err(3));
        assert_eq!(buffer.len(), 3);
    }

    #[test]
    fn many_views_cannot_exceed_the_item_budget() {
        let mut buffer = ViewBuffer::new(3, UNLIMITED);
        for height in 1..=100 {
            let _ignore = buffer.insert(view(height), height, NO_SIZE);
        }
        assert_eq!(buffer.len(), 3);
    }

    #[test]
    fn many_small_items_cannot_exceed_the_size_budget() {
        let mut buffer = ViewBuffer::new(UNLIMITED, 100);
        for height in 1..=100 {
            let _ignore = buffer.insert(view(height), height, 10);
        }
        assert_eq!(buffer.size(), 100);
        assert_eq!(buffer.len(), 10);
    }

    #[test]
    fn a_few_large_items_cannot_exceed_the_size_budget() {
        let mut buffer = ViewBuffer::new(UNLIMITED, 100);
        buffer.insert(view(10), "large", 60).unwrap();
        buffer.insert(view(11), "large", 40).unwrap();

        // Nearer, so it evicts both of the above to fit.
        let inserted = buffer.insert(view(1), "largest", 100).unwrap();

        assert_eq!(inserted.num_evicted, 2);
        assert_eq!(buffer.size(), 100);
        assert_eq!(buffer.len(), 1);
        assert_eq!(buffer.pop_front(&view(1)), Some("largest"));
        assert_eq!(buffer.size(), 0);
    }

    #[test]
    fn an_item_larger_than_the_size_budget_is_refused() {
        let mut buffer = ViewBuffer::new(UNLIMITED, 100);
        assert_eq!(buffer.insert(view(1), "too large", 101), Err("too large"));
        assert_eq!(buffer.len(), 0);
        assert_eq!(buffer.size(), 0);
    }

    #[test]
    fn a_nearer_view_evicts_the_furthest_item() {
        let mut buffer = ViewBuffer::new(2, UNLIMITED);
        insert(&mut buffer, view(10), "far").unwrap();
        insert(&mut buffer, view(11), "further").unwrap();

        let inserted = insert(&mut buffer, view(1), "near").unwrap();

        assert_eq!(inserted.num_evicted, 1);
        assert_eq!(buffer.len(), 2);
        assert_eq!(buffer.pop_front(&view(1)), Some("near"));
        assert_eq!(buffer.pop_front(&view(10)), Some("far"));
        assert_eq!(buffer.pop_front(&view(11)), None);
    }

    #[test]
    fn an_item_for_the_furthest_view_is_refused() {
        let mut buffer = ViewBuffer::new(2, UNLIMITED);
        insert(&mut buffer, view(1), "near").unwrap();
        insert(&mut buffer, view(10), "far").unwrap();

        assert_eq!(insert(&mut buffer, view(10), "same"), Err("same"));
        assert_eq!(insert(&mut buffer, view(11), "furthest"), Err("furthest"));
        assert_eq!(buffer.len(), 2);
        assert_eq!(buffer.pop_front(&view(1)), Some("near"));
    }
}
