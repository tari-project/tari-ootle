//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use crate::{Amount, op_assign_impl, op_impl};

op_impl!(Amount, Add, add, checked_add, "attempt to add with overflow");
op_impl!(Amount, Sub, sub, checked_sub, "attempt to subtract with overflow");
op_impl!(Amount, Mul, mul, checked_mul, "attempt to multiply with overflow");
op_impl!(
    Amount,
    Div,
    div,
    checked_div,
    "attempt to divide with overflow",
    "attempt to divide by zero"
);
op_impl!(
    Amount,
    Rem,
    rem,
    checked_rem,
    "attempt to calculate the remainder with overflow",
    "attempt to calculate the remainder with a divisor of zero"
);

op_assign_impl!(Amount, AddAssign, add_assign, Add, add);
op_assign_impl!(Amount, SubAssign, sub_assign, Sub, sub);
op_assign_impl!(Amount, MulAssign, mul_assign, Mul, mul);
op_assign_impl!(Amount, DivAssign, div_assign, Div, div);
op_assign_impl!(Amount, RemAssign, rem_assign, Rem, rem);
