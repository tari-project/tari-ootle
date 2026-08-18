//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use core::ops::ControlFlow;

use crate::Value;

pub fn walk_all<V, T>(value: &Value, visitor: &mut V, max_depth: usize) -> Result<(), V::Error>
where
    V: ValueVisitor<T>,
    for<'a> V::Error: From<&'a str>,
    T: FromTagAndValue<Error = V::Error>,
{
    let _ignore = walk_all_depth(value, visitor, max_depth, 0)?;
    Ok(())
}

fn walk_all_depth<V, T>(
    value: &Value,
    visitor: &mut V,
    max_depth: usize,
    depth: usize,
) -> Result<ControlFlow<()>, V::Error>
where
    V: ValueVisitor<T>,
    for<'a> V::Error: From<&'a str>,
    T: FromTagAndValue<Error = V::Error>,
{
    if depth >= max_depth {
        return Err(V::Error::from("Maximum depth exceeded"));
    }

    match value {
        Value::Integer(_) | Value::Bytes(_) | Value::Float(_) | Value::Text(_) | Value::Bool(_) | Value::Null => {},
        Value::Tag(tag, val) => {
            // A tag the visitor does not claim still wraps a value that may contain ones it does.
            let Some(claimed) = T::try_from_tag_and_value(*tag, val)? else {
                return walk_all_depth(val, visitor, max_depth, depth + 1);
            };
            let flow = visitor.visit(claimed)?;
            return Ok(flow);
        },
        Value::Array(values) => {
            for value in values {
                if walk_all_depth(value, visitor, max_depth, depth + 1)?.is_break() {
                    return Ok(ControlFlow::Break(()));
                }
            }
        },
        Value::Map(value_pairs) => {
            for (key, value) in value_pairs {
                if walk_all_depth(key, visitor, max_depth, depth + 1)?.is_break() {
                    return Ok(ControlFlow::Break(()));
                }
                if walk_all_depth(value, visitor, max_depth, depth + 1)?.is_break() {
                    return Ok(ControlFlow::Break(()));
                }
            }
        },
    }

    Ok(ControlFlow::Continue(()))
}

pub trait ValueVisitor<T> {
    type Error;

    fn visit(&mut self, value: T) -> Result<ControlFlow<()>, Self::Error>;
}

impl<F: FnMut(T) -> Result<ControlFlow<()>, E>, T, E> ValueVisitor<T> for F {
    type Error = E;

    fn visit(&mut self, value: T) -> Result<ControlFlow<()>, Self::Error> {
        self(value)
    }
}

pub trait FromTagAndValue {
    type Error;

    /// `None` when `tag` is not one this type represents — a standard CBOR tag, say — leaving the
    /// walker to descend into the tagged value rather than fail on it.
    fn try_from_tag_and_value(tag: u64, value: &Value) -> Result<Option<Self>, Self::Error>
    where Self: Sized;
}
