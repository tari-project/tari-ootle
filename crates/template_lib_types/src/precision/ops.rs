//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_abi::rust::ops;

use crate::{op_impl, precision::PrecisionAmount};

op_impl!(PrecisionAmount, Add, add);
op_impl!(PrecisionAmount, Sub, sub);
op_impl!(PrecisionAmount, Mul, mul);
op_impl!(PrecisionAmount, Div, div);
op_impl!(PrecisionAmount, Rem, rem);

impl ops::AddAssign<PrecisionAmount> for PrecisionAmount {
    fn add_assign(&mut self, other: PrecisionAmount) {
        let this = self;
        this.0.add_assign(other.0)
    }
}

impl ops::SubAssign<PrecisionAmount> for PrecisionAmount {
    fn sub_assign(&mut self, other: PrecisionAmount) {
        self.0.sub_assign(other.0)
    }
}
impl ops::MulAssign<PrecisionAmount> for PrecisionAmount {
    fn mul_assign(&mut self, other: PrecisionAmount) {
        let this = self;
        this.0.mul_assign(other.0)
    }
}
impl ops::DivAssign<PrecisionAmount> for PrecisionAmount {
    fn div_assign(&mut self, other: PrecisionAmount) {
        let this = self;
        this.0.div_assign(other.0)
    }
}
impl ops::RemAssign<PrecisionAmount> for PrecisionAmount {
    fn rem_assign(&mut self, other: PrecisionAmount) {
        let this = self;
        this.0.rem_assign(other.0)
    }
}

impl ops::Neg for PrecisionAmount {
    type Output = Self;

    fn neg(self) -> Self::Output {
        PrecisionAmount(self.0.neg())
    }
}
