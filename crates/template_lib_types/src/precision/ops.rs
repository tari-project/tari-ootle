//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_abi::rust::ops;

use crate::{op_assign_impl, op_impl, precision::PrecisionAmount};

op_impl!(PrecisionAmount, Add, add, checked_add, "attempt to add with overflow");
op_impl!(
    PrecisionAmount,
    Sub,
    sub,
    checked_sub,
    "attempt to subtract with overflow"
);
op_impl!(
    PrecisionAmount,
    Mul,
    mul,
    checked_mul,
    "attempt to multiply with overflow"
);
op_impl!(
    PrecisionAmount,
    Div,
    div,
    checked_div,
    "attempt to divide with overflow",
    "attempt to divide by zero"
);
op_impl!(
    PrecisionAmount,
    Rem,
    rem,
    checked_rem,
    "attempt to calculate the remainder with overflow",
    "attempt to calculate the remainder with a divisor of zero"
);

op_assign_impl!(PrecisionAmount, AddAssign, add_assign, Add, add);
op_assign_impl!(PrecisionAmount, SubAssign, sub_assign, Sub, sub);
op_assign_impl!(PrecisionAmount, MulAssign, mul_assign, Mul, mul);
op_assign_impl!(PrecisionAmount, DivAssign, div_assign, Div, div);
op_assign_impl!(PrecisionAmount, RemAssign, rem_assign, Rem, rem);

impl ops::Neg for PrecisionAmount {
    type Output = Self;

    fn neg(self) -> Self::Output {
        match self.checked_neg() {
            Some(value) => value,
            None => panic!("attempt to negate with overflow"),
        }
    }
}
