//    Copyright 2025 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use tari_engine_types::limits::StealthLimits;
use tari_template_lib::models::{StealthOutputsStatement, StealthTransferStatement};

use crate::runtime::error::ArgumentValidationError;

pub(crate) fn check_stealth_transfer_limits(
    limits: &StealthLimits,
    statement: &StealthTransferStatement,
) -> Result<(), ArgumentValidationError> {
    if statement.inputs_statement.inputs.len() > limits.max_inputs {
        return Err(ArgumentValidationError::MaxStealthInputsExceeded {
            max_inputs: limits.max_inputs,
            actual_inputs: statement.inputs_statement.inputs.len(),
        });
    }
    check_stealth_outputs_limits(limits, &statement.outputs_statement)?;
    Ok(())
}

pub(crate) fn check_stealth_outputs_limits(
    limits: &StealthLimits,
    statement: &StealthOutputsStatement,
) -> Result<(), ArgumentValidationError> {
    if statement.outputs.len() > limits.max_outputs {
        return Err(ArgumentValidationError::MaxStealthOutputsExceeded {
            max_outputs: limits.max_outputs,
            actual_outputs: statement.outputs.len(),
        });
    }
    Ok(())
}
