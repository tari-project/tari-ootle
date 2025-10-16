//  Copyright 2025. The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, SUCH DAMAGE.

import { FormEvent, useState } from "react";
import { Form } from "react-router-dom";
import Button from "@mui/material/Button";
import TextField from "@mui/material/TextField";
import Select from "@mui/material/Select";
import MenuItem from "@mui/material/MenuItem";
import CheckBox from "@mui/material/Checkbox";
import FormControlLabel from "@mui/material/FormControlLabel";
import { Divider, InputLabel, Stack, InputAdornment, Typography } from "@mui/material";
import { SelectChangeEvent } from "@mui/material/Select/Select";
import { ResourceType, ResourceAddress, validateOotleAddress } from "@tari-project/typescript-bindings";
import { formatDisplayCurrency } from "@utils/helpers";
import { CURRENCY } from "@utils/constants";

export interface SendMoneyFormState {
  address: string;
  outputToConfidential: boolean;
  inputSelection: string;
  amount: string;
  fee: string;
  badge: string | null;
  memo: string;
}

interface FormStepProps {
  resource_address?: ResourceAddress;
  resource_type: ResourceType;
  badges?: string[];
  transferFormState: SendMoneyFormState;
  disabled: boolean;
  useBadge: boolean;
  isEstimatingFee: boolean;
  availableBalance?: number;
  token_symbol: string;
  divisibility: number;
  formError?: FormError | null;
  onSubmit: (e: FormEvent) => void;
  onCancel: () => void;
  onFormValueChange: (e: React.ChangeEvent<HTMLInputElement>) => void;
  onSelectFormValueChange: (e: SelectChangeEvent<unknown>) => void;
  onCheckboxFormValueChange: (e: React.ChangeEvent<HTMLInputElement>) => void;
  onUseBadgeChange: (e: React.ChangeEvent<HTMLInputElement>) => void;
}

export type FormError = {
  type: "general" | "address" | "amount" | "fee";
  message: string;
};

export default function FormStep({
  resource_address,
  resource_type,
  badges,
  transferFormState,
  disabled,
  useBadge,
  isEstimatingFee,
  availableBalance,
  token_symbol,
  divisibility,
  formError,
  onSubmit,
  onCancel,
  onFormValueChange,
  onSelectFormValueChange,
  onCheckboxFormValueChange,
  onUseBadgeChange,
}: FormStepProps) {
  const isConfidential = resource_type === "Confidential";
  const isStealth = resource_type === "Stealth";

  // Track if the user is currently typing in the amount field
  const [isFocusedAmount, setIsFocusedAmount] = useState(false);

  const enteredAmount = parseFloat(transferFormState.amount) || 0;
  const hasInsufficientFunds = availableBalance !== undefined && enteredAmount > availableBalance;

  const isFormValid =
    validateOotleAddress(transferFormState.address) && transferFormState.amount && !hasInsufficientFunds;

  // Format amount for display
  const formatAmountValue = (amount: string) => {
    if (!amount) return "";
    const num = parseFloat(amount);
    if (isNaN(num)) return amount;

    // If user is currently typing, show raw value to avoid cursor jumping
    if (isFocusedAmount) {
      return amount;
    }

    // Otherwise, show formatted value
    return num.toLocaleString("en-US", {
      minimumFractionDigits: 0,
      maximumFractionDigits: divisibility,
    });
  };

  return (
    <Form onSubmit={onSubmit}>
      <Stack direction="column" spacing={2} sx={{ py: 2 }}>
        {resource_address && (
          <Stack direction="column" spacing={0.5}>
            <Typography variant="subtitle2" color="text.secondary">
              Resource Address:
            </Typography>
            <Typography variant="body1">{resource_address}</Typography>
          </Stack>
        )}

        {badges && (
          <>
            <FormControlLabel
              control={<CheckBox name="useBadge" checked={useBadge} onChange={onUseBadgeChange} />}
              label="Use Badge"
            />
            {useBadge && (
              <>
                <InputLabel id="select-badge">Badge</InputLabel>
                <Select
                  id="select-badge"
                  name="badge"
                  disabled={disabled}
                  displayEmpty
                  value={transferFormState.badge || ""}
                  onChange={onSelectFormValueChange}
                >
                  {badges.map((b, i) => (
                    <MenuItem key={i} value={b}>
                      {b}
                    </MenuItem>
                  ))}
                </Select>
              </>
            )}
          </>
        )}
        <Stack direction="column" spacing={0.5}>
          <DisplayFormError forType="address" formError={formError} />
          <TextField
            name="address"
            label="To Address"
            value={transferFormState.address}
            required
            onChange={onFormValueChange}
            style={{ flexGrow: 1 }}
            disabled={disabled}
          />
        </Stack>

        {(isConfidential || isStealth) && (
          <>
            <FormControlLabel
              control={
                <CheckBox
                  name="outputToConfidential"
                  checked={transferFormState.outputToConfidential}
                  onChange={onCheckboxFormValueChange}
                  disabled={disabled}
                />
              }
              label="Send Confidential Outputs"
            />
            {transferFormState.outputToConfidential ? (
              <TextField
                name="memo"
                label="Memo message (optional, max 253 characters)"
                inputProps={{ maxLength: 253 }}
                value={transferFormState.memo}
                onChange={onFormValueChange}
                style={{ flexGrow: 1 }}
                disabled={disabled}
              />
            ) : null}
            <InputLabel id="select-input-selection">Input Selection</InputLabel>
            <Select
              name="inputSelection"
              disabled={disabled}
              displayEmpty
              value={transferFormState.inputSelection}
              onChange={onSelectFormValueChange}
            >
              <MenuItem value="PreferRevealed">Spend revealed funds first, then confidential</MenuItem>
              <MenuItem value="PreferConfidential">Spend confidential funds first, then revealed</MenuItem>
              <MenuItem value="ConfidentialOnly">Only spend confidential funds</MenuItem>
              <MenuItem value="RevealedOnly">Only spend revealed funds</MenuItem>
            </Select>
          </>
        )}

        <DisplayFormError forType="amount" formError={formError} />
        <TextField
          name="amount"
          label="Amount"
          value={formatAmountValue(transferFormState.amount)}
          type="text"
          required
          onChange={onFormValueChange}
          onFocus={() => setIsFocusedAmount(true)}
          onBlur={() => setIsFocusedAmount(false)}
          style={{ flexGrow: 1 }}
          disabled={disabled}
          error={hasInsufficientFunds}
          helperText={
            hasInsufficientFunds
              ? `Insufficient funds. Available balance: ${formatDisplayCurrency(availableBalance || 0, divisibility, token_symbol)}`
              : availableBalance !== undefined
                ? `Available balance: ${formatDisplayCurrency(availableBalance, divisibility, token_symbol)}`
                : undefined
          }
          InputProps={{
            placeholder: "0" + (divisibility > 0 ? "." + "0".repeat(divisibility) : ""),
            endAdornment: token_symbol ? <InputAdornment position="end">{token_symbol}</InputAdornment> : undefined,
          }}
        />

        <TextField
          name="fee"
          label="Fee"
          value={
            isEstimatingFee
              ? "Estimating..."
              : transferFormState.fee
                ? (parseInt(transferFormState.fee) / CURRENCY.DIVISOR).toString()
                : ""
          }
          placeholder={isEstimatingFee ? "Estimating..." : "Auto-calculated"}
          onChange={onFormValueChange}
          disabled={true}
          style={{ flexGrow: 1 }}
          InputProps={{
            endAdornment:
              !isEstimatingFee && token_symbol ? <InputAdornment position="end">{token_symbol}</InputAdornment> : null,
          }}
        />

        <Divider />

        <DisplayFormError forType="general" formError={formError} />
        <Stack direction="row" justifyContent="space-between" sx={{ mt: 3 }}>
          <Button variant="outlined" onClick={onCancel} disabled={disabled}>
            Cancel
          </Button>
          <Button variant="contained" type="submit" disabled={disabled || !isFormValid}>
            {isEstimatingFee ? "Estimating..." : transferFormState.fee ? "Continue" : "Continue"}
          </Button>
        </Stack>
      </Stack>
    </Form>
  );
}

function DisplayFormError({ forType, formError }: { forType: FormError["type"]; formError?: FormError | null }) {
  if (!formError) return null;
  if (formError.type !== forType) return null;
  return (
    <Typography color="error" sx={{ mb: 2 }}>
      {formError.message}
    </Typography>
  );
}
