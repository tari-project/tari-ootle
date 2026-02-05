//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_abi::rust::ops;

use crate::{Amount, op_impl};

op_impl!(Amount, Add, add);
op_impl!(Amount, Sub, sub);
op_impl!(Amount, Mul, mul);
op_impl!(Amount, Div, div);
op_impl!(Amount, Rem, rem);

impl ops::AddAssign<Amount> for Amount {
    fn add_assign(&mut self, other: Amount) {
        let this = self;
        this.0.add_assign(other.0)
    }
}

impl ops::SubAssign<Amount> for Amount {
    fn sub_assign(&mut self, other: Amount) {
        self.0.sub_assign(other.0)
    }
}
impl ops::MulAssign<Amount> for Amount {
    fn mul_assign(&mut self, other: Amount) {
        let this = self;
        this.0.mul_assign(other.0)
    }
}
impl ops::DivAssign<Amount> for Amount {
    fn div_assign(&mut self, other: Amount) {
        let this = self;
        this.0.div_assign(other.0)
    }
}
impl ops::RemAssign<Amount> for Amount {
    fn rem_assign(&mut self, other: Amount) {
        let this = self;
        this.0.rem_assign(other.0)
    }
}
