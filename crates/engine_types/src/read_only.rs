//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

/// Like a CoW but with the invariant that we never want to mutate the data, removing the need for the Clone bound.
pub enum ReadOnly<'a, T> {
    Borrowed(&'a T),
    Owned(T),
}

impl<'a, T> ReadOnly<'a, T> {
    pub fn into_owned(self) -> T
    where T: Clone {
        match self {
            ReadOnly::Borrowed(r) => r.clone(),
            ReadOnly::Owned(o) => o,
        }
    }
}

impl<'a, T> AsRef<T> for ReadOnly<'a, T> {
    fn as_ref(&self) -> &T {
        match self {
            ReadOnly::Borrowed(r) => r,
            ReadOnly::Owned(o) => o,
        }
    }
}

impl<'a, T: Clone> Clone for ReadOnly<'a, T> {
    fn clone(&self) -> Self {
        match self {
            ReadOnly::Borrowed(r) => ReadOnly::Borrowed(r),
            ReadOnly::Owned(o) => ReadOnly::Owned(o.clone()),
        }
    }
}
impl<'a, T: std::fmt::Debug> std::fmt::Debug for ReadOnly<'a, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadOnly::Borrowed(r) => write!(f, "ReadOnly::Ref({:?})", r),
            ReadOnly::Owned(o) => write!(f, "ReadOnly::Owned({:?})", o),
        }
    }
}

impl<'a, T> std::ops::Deref for ReadOnly<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}
