//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::{BTreeSet, HashMap, HashSet},
    fmt,
    fmt::{Debug, Display},
};

/// Implements a method that returns an allocation-free Display impl for container types such as Option<T>, Vec<T>, [T],
/// HashSet<T>, BTreeSet<T>.
///
/// # Example
/// ```rust
/// use tari_dan_common_types::option::Displayable;
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
pub trait Displayable {
    type Item: ?Sized;
    fn display(&self) -> DisplayContainer<&'_ Self::Item>;
}

#[derive(Debug, Clone, Copy)]
pub struct DisplayContainer<T> {
    value: T,
}

impl<T: Display> Display for DisplayContainer<&'_ Option<T>> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.value {
            Some(value) => Display::fmt(value, f),
            None => write!(f, "None"),
        }
    }
}

impl<T: Display> Display for DisplayContainer<&'_ [T]> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let len = self.value.len();
        write!(f, "[")?;
        for (i, item) in self.value.iter().enumerate() {
            Display::fmt(item, f)?;
            if i < len - 1 {
                write!(f, ", ")?;
            }
        }
        write!(f, "]")?;
        Ok(())
    }
}

impl<T: Display> Display for DisplayContainer<&'_ HashSet<T>> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let len = self.value.len();
        write!(f, "{{")?;
        for (i, item) in self.value.iter().enumerate() {
            Display::fmt(item, f)?;
            if i < len - 1 {
                write!(f, ", ")?;
            }
        }
        write!(f, "}}")?;
        Ok(())
    }
}

impl<T: Display> Display for DisplayContainer<&'_ BTreeSet<T>> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let len = self.value.len();
        write!(f, "{{")?;
        for (i, item) in self.value.iter().enumerate() {
            Display::fmt(item, f)?;
            if i < len - 1 {
                write!(f, ", ")?;
            }
        }
        write!(f, "}}")?;
        Ok(())
    }
}

impl<K: Display, V: Display> Display for DisplayContainer<&'_ HashMap<K, V>> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let len = self.value.len();
        write!(f, "{{")?;
        for (i, (k, v)) in self.value.iter().enumerate() {
            write!(f, "{k}: {v}")?;
            if i < len - 1 {
                write!(f, ", ")?;
            }
        }
        write!(f, "}}")?;
        Ok(())
    }
}

impl<T: Display> Displayable for Option<T> {
    type Item = Self;

    fn display(&self) -> DisplayContainer<&'_ Self> {
        DisplayContainer { value: self }
    }
}

impl<T: Display> Displayable for [T] {
    type Item = Self;

    fn display(&self) -> DisplayContainer<&'_ Self> {
        DisplayContainer { value: self }
    }
}

impl<T: Display> Displayable for Vec<T> {
    type Item = [T];

    fn display(&self) -> DisplayContainer<&'_ [T]> {
        (*self.as_slice()).display()
    }
}

impl<T: Display> Displayable for HashSet<T> {
    type Item = Self;

    fn display(&self) -> DisplayContainer<&'_ Self> {
        DisplayContainer { value: self }
    }
}

impl<T: Display> Displayable for BTreeSet<T> {
    type Item = Self;

    fn display(&self) -> DisplayContainer<&'_ Self> {
        DisplayContainer { value: self }
    }
}

impl<K: Display, V: Display> Displayable for HashMap<K, V> {
    type Item = Self;

    fn display(&self) -> DisplayContainer<&'_ Self> {
        DisplayContainer { value: self }
    }
}
