//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use curve25519_dalek::{RistrettoPoint, Scalar, constants::RISTRETTO_BASEPOINT_TABLE};

pub fn public_key_from_scalar(secret_key: &Scalar) -> RistrettoPoint {
    secret_key * RISTRETTO_BASEPOINT_TABLE
}
