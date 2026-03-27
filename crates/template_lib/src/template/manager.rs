//   Copyright 2023. The Tari Project
//
//   Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//   following conditions are met:
//
//   1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//   disclaimer.
//
//   2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//   following disclaimer in the documentation and/or other materials provided with the distribution.
//
//   3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//   products derived from this software without specific prior written permission.
//
//   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//   INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//   DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//   SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//   SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//   WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//   USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
use serde::de::DeserializeOwned;
use tari_template_abi::{EngineOp, call_engine, rust::prelude::*};
use tari_template_lib_types::{TemplateAddress, bytes::Bytes};

use crate::args::{CallAction, CallFunctionArg, CallInvokeArg, InvokeResult};

/// Utility to allow template code to call functions from other templates (i.e., composability)
#[derive(Debug)]
pub struct TemplateManager {
    template_address: TemplateAddress,
}

impl TemplateManager {
    /// Returns a new `TemplateManager` for the template specified as argument
    pub fn get(template_address: TemplateAddress) -> Self {
        Self { template_address }
    }

    /// Executes a function in the template.
    /// Template functions can be called from another template function or from component methods.
    pub fn call<F: Into<String>, T: DeserializeOwned, B: Into<Bytes>>(&self, function: F, args: Vec<B>) -> T {
        self.call_internal(CallFunctionArg {
            template_address: self.template_address,
            function: function.into(),
            args: args.into_iter().map(Into::into).collect(),
        })
    }

    fn call_internal<T: DeserializeOwned>(&self, arg: CallFunctionArg) -> T {
        let result = call_engine::<_, InvokeResult>(EngineOp::CallInvoke, &CallInvokeArg {
            action: CallAction::CallFunction,
            args: invoke_args![arg],
        });

        result
            .decode()
            .expect("failed to decode template function call result from engine")
    }

    /// Calls a function in the template. The invoked function must return a unit type or a panic will occur.
    /// Equivalent to `call::<_, ()>(function, args)`.
    pub fn invoke<F: Into<String>, B: Into<Bytes>>(&self, function: F, args: Vec<B>) {
        self.call(function, args)
    }
}
