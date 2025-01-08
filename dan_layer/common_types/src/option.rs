//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::{BTreeSet, HashSet},
    fmt,
    fmt::{Debug, Display},
};

/// Implements a method that returns an allocation-free Display impl for container types such as Option<T>, Vec<T>, [T],
/// HashSet<T>, BTreeSet<T>.
///
/// # Example
/// ```rust
/// use tari_dan_common_types::option::DisplayContainer;
///
/// let some_value = Some(42);
/// let none_value: Option<i32> = None;
///
/// // The usual way to do this is verbose and has a heap allocation
/// let _bad = println!(
///     "answer: {}",
///     some_value
///         .as_ref()
///          // Heap allocation
///         .map(|v| v.to_string())
///         .unwrap_or_else(|| "None".to_string())
/// );
///
/// assert_eq!(format!("answer: {}", some_value.display()), "answer: 42");
/// assert_eq!(format!("answer: {}", none_value.display()), "answer: None");
/// assert_eq!(
///     format!("list: {:.2}", vec![1.01f32, 2f32, 3f32].display()),
///     "list: 1.01, 2.00, 3.00"
/// );
/// ```
pub trait DisplayContainer {
    type Item: ?Sized;
    fn display(&self) -> DisplayCont<&'_ Self::Item>;
}

#[derive(Debug, Clone, Copy)]
pub struct DisplayCont<T> {
    value: T,
}

impl<T: Display> Display for DisplayCont<&'_ Option<T>> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.value {
            Some(value) => Display::fmt(value, f),
            None => write!(f, "None"),
        }
    }
}

impl<T: Display> Display for DisplayCont<&'_ [T]> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let len = self.value.len();
        for (i, item) in self.value.iter().enumerate() {
            Display::fmt(item, f)?;
            if i < len - 1 {
                write!(f, ", ")?;
            }
        }
        Ok(())
    }
}

impl<T: Display> Display for DisplayCont<&'_ HashSet<T>> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let len = self.value.len();
        for (i, item) in self.value.iter().enumerate() {
            Display::fmt(item, f)?;
            if i < len - 1 {
                write!(f, ", ")?;
            }
        }
        Ok(())
    }
}

impl<T: Display> Display for DisplayCont<&'_ BTreeSet<T>> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let len = self.value.len();
        for (i, item) in self.value.iter().enumerate() {
            Display::fmt(item, f)?;
            if i < len - 1 {
                write!(f, ", ")?;
            }
        }
        Ok(())
    }
}

impl<T: Display> DisplayContainer for Option<T> {
    type Item = Self;

    fn display(&self) -> DisplayCont<&'_ Self> {
        DisplayCont { value: self }
    }
}

impl<T: Display> DisplayContainer for [T] {
    type Item = Self;

    fn display(&self) -> DisplayCont<&'_ Self> {
        DisplayCont { value: self }
    }
}

impl<T: Display> DisplayContainer for Vec<T> {
    type Item = [T];

    fn display(&self) -> DisplayCont<&'_ [T]> {
        (*self.as_slice()).display()
    }
}

impl<T: Display> DisplayContainer for HashSet<T> {
    type Item = Self;

    fn display(&self) -> DisplayCont<&'_ Self> {
        DisplayCont { value: self }
    }
}

impl<T: Display> DisplayContainer for BTreeSet<T> {
    type Item = Self;

    fn display(&self) -> DisplayCont<&'_ Self> {
        DisplayCont { value: self }
    }
}
