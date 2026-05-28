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

import AddressAutocomplete from "@components/AddressAutocomplete";
import CopyAddress from "@components/CopyAddress";
import HelpOutlineIcon from "@mui/icons-material/HelpOutline";
import { Alert, Chip, CircularProgress, Divider, InputAdornment, InputLabel, Stack, Typography } from "@mui/material";
import Button from "@mui/material/Button";
import CheckBox from "@mui/material/Checkbox";
import FormControlLabel from "@mui/material/FormControlLabel";
import MenuItem from "@mui/material/MenuItem";
import Select, { SelectChangeEvent } from "@mui/material/Select";
import TextField from "@mui/material/TextField";
import Tooltip from "@mui/material/Tooltip";
import TypeChip from "@routes/AssetVault/Components/ResourceTypeChip";
import {
  Amount,
  ResourceAddress,
  ResourceType,
  SwapPoolInfo,
  TARI_TOKEN,
  validateOotleAddress,
} from "@tari-project/ootle-ts-bindings";
import { XTR_CURRENCY } from "@utils/currency";
import { formatCurrency, parseAmountToBaseUnits } from "@utils/helpers";
import { SyntheticEvent, useState } from "react";
import { Form } from "react-router";

export interface SendMoneyFormState {
  address: string;
  outputToRevealed: boolean;
  inputSelection: string;
  amount: string;
  fee: string;
  badge: string | null;
  memo: string;
  attachSenderAddress: boolean;
  payRef: string;
  swapPoolAddress: string;
  // Calculated from fee estimate + pool ratio, not user-entered
  swapInputAmount: string;
}

export interface PoolRateInfo {
  resource_a: string;
  balance_a: bigint;
  resource_b: string;
  balance_b: bigint;
}

interface FormStepProps {
  resource_address?: ResourceAddress;
  resource_type: ResourceType;
  badges?: string[];
  transferFormState: SendMoneyFormState;
  disabled: boolean;
  useBadge: boolean;
  isEstimatingFee: boolean;
  availableBalance?: Amount;
  token_symbol: string;
  divisibility: number;
  formError?: FormError | null;
  knownPools: SwapPoolInfo[];
  isLoadingPools: boolean;
  poolRate: PoolRateInfo | null;
  poolError: string | null;
  isLoadingPoolRate: boolean;
  onSubmit: (e: SyntheticEvent) => void;
  onCancel: () => void;
  onFormValueChange: (name: string, value: string) => void;
  onSelectFormValueChange: (e: SelectChangeEvent<unknown>) => void;
  onCheckboxFormValueChange: (e: React.ChangeEvent<HTMLInputElement>) => void;
  onUseBadgeChange: (e: React.ChangeEvent<HTMLInputElement>) => void;
  onPoolSelect: (poolAddress: string) => void;
  hasTariBalance?: boolean;
}

export type FormError = {
  type: "general" | "address" | "amount" | "fee";
  message: string;
};

function formatPoolRate(poolRate: PoolRateInfo, tokenSymbol: string): string {
  const a = poolRate.balance_a;
  const b = poolRate.balance_b;
  if (a === 0n || b === 0n) return "Pool has no liquidity";

  // Determine which side is the token and which is TARI
  if (poolRate.resource_a === TARI_TOKEN) {
    // resource_b is the token: rate = balance_a / balance_b (TARI per token)
    const rate = Number(a) / Number(b);
    return `1 ${tokenSymbol} = ${rate.toFixed(6)} TARI`;
  } else if (poolRate.resource_b === TARI_TOKEN) {
    // resource_a is the token: rate = balance_b / balance_a (TARI per token)
    const rate = Number(b) / Number(a);
    return `1 ${tokenSymbol} = ${rate.toFixed(6)} TARI`;
  }
  // Neither side is TARI — show raw ratio
  const rate = Number(a) / Number(b);
  return `1:${rate.toFixed(6)}`;
}

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
  knownPools,
  isLoadingPools,
  poolRate,
  poolError,
  isLoadingPoolRate,
  onSubmit,
  onCancel,
  onFormValueChange,
  onSelectFormValueChange,
  onCheckboxFormValueChange,
  onUseBadgeChange,
  onPoolSelect,
  hasTariBalance,
}: FormStepProps) {
  const isConfidential = resource_type === "Confidential";
  const isStealth = resource_type === "Stealth";

  // Track if the user is currently typing in the amount field
  const [isFocusedAmount, setIsFocusedAmount] = useState(false);
  const [showCustomPool, setShowCustomPool] = useState(false);

  const enteredAmount = parseFloat(transferFormState.amount);
  const isNaNAmount = isNaN(enteredAmount);
  const enteredAmountInBaseUnits = isNaNAmount ? 0n : parseAmountToBaseUnits(transferFormState.amount, divisibility);
  const hasInsufficientFunds = availableBalance !== undefined && enteredAmountInBaseUnits > BigInt(availableBalance);
  const poolHasNoLiquidity =
    !!transferFormState.swapPoolAddress &&
    poolRate !== null &&
    (poolRate.balance_a === 0n || poolRate.balance_b === 0n);

  // Pay reference is capped at 64 bytes whether it's embedded in a PayRefAndBytes or a SenderAddress memo.
  const payRefByteLength = new TextEncoder().encode(transferFormState.payRef).length;
  const payRefTooLong = payRefByteLength > 64;

  const isFormValid =
    !isNaNAmount &&
    validateOotleAddress(transferFormState.address) &&
    transferFormState.amount &&
    !hasInsufficientFunds &&
    !poolError &&
    !poolHasNoLiquidity &&
    !payRefTooLong;

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

  const currency = {
    symbol: token_symbol,
    decimals: divisibility,
  };

  // Find pools that contain our resource
  const relevantPools = knownPools.filter(
    (p) => p.resource_a === resource_address || p.resource_b === resource_address,
  );

  return (
    <Form onSubmit={onSubmit}>
      <Stack direction="column" spacing={2}>
        {resource_address && (
          <Stack direction="row" spacing={1} alignItems="center">
            <TypeChip type={resource_type} symbol={token_symbol} />
            <CopyAddress address={resource_address} />
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
          <AddressAutocomplete
            value={transferFormState.address}
            onChange={(v) => onFormValueChange("address", v)}
            disabled={disabled}
            required
          />
        </Stack>

        {(isConfidential || isStealth) && (
          <>
            <FormControlLabel
              control={
                <CheckBox
                  name="outputToRevealed"
                  checked={transferFormState.outputToRevealed}
                  onChange={onCheckboxFormValueChange}
                  disabled={disabled}
                />
              }
              label="Send Revealed Funds"
            />
            {transferFormState.outputToRevealed && (
              <Typography color="warning.main">
                ⚠️ Warning: Revealed funds are visible on the blockchain and can be viewed by anyone.
              </Typography>
            )}

            {transferFormState.outputToRevealed || transferFormState.attachSenderAddress ? null : (
              <TextField
                name="memo"
                label="Encrypted Memo (optional, max 253 characters)"
                slotProps={{
                  htmlInput: { maxLength: 253 },
                }}
                value={transferFormState.memo}
                onChange={(e) => onFormValueChange(e.target.name, e.target.value)}
                style={{ flexGrow: 1 }}
                disabled={disabled}
              />
            )}
            {isStealth && !transferFormState.outputToRevealed && (
              <TextField
                name="payRef"
                label="Encrypted Pay ref (optional, max 64 bytes)"
                value={transferFormState.payRef}
                onChange={(e) => onFormValueChange(e.target.name, e.target.value)}
                error={payRefTooLong}
                helperText={
                  payRefTooLong
                    ? `Too long: ${payRefByteLength} bytes (max 64)`
                    : "Auto-filled from the destination address if it has one."
                }
                disabled={disabled}
              />
            )}
            {isStealth && !transferFormState.outputToRevealed && (
              <FormControlLabel
                control={
                  <CheckBox
                    name="attachSenderAddress"
                    checked={transferFormState.attachSenderAddress}
                    onChange={onCheckboxFormValueChange}
                    disabled={disabled}
                  />
                }
                label="Attach my Ootle address"
              />
            )}
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
          onChange={(e) => onFormValueChange(e.target.name, e.target.value)}
          onFocus={() => setIsFocusedAmount(true)}
          onBlur={() => setIsFocusedAmount(false)}
          style={{ flexGrow: 1 }}
          disabled={disabled}
          error={hasInsufficientFunds}
          placeholder={"0" + (divisibility > 0 ? "." + "0".repeat(divisibility) : "")}
          helperText={
            hasInsufficientFunds
              ? `Insufficient funds. Available balance: ${formatCurrency(availableBalance || 0, currency)}`
              : availableBalance !== undefined
                ? `Available balance: ${formatCurrency(availableBalance, currency)}`
                : undefined
          }
          slotProps={{
            input: {
              endAdornment: token_symbol ? <InputAdornment position="end">{token_symbol}</InputAdornment> : undefined,
            },
          }}
        />

        <TextField
          name="fee"
          label="Fee"
          value={transferFormState.fee}
          placeholder={isEstimatingFee ? "Estimating..." : "Auto-calculated"}
          onChange={(e) => onFormValueChange(e.target.name, e.target.value)}
          style={{ flexGrow: 1 }}
          slotProps={{
            input: {
              endAdornment:
                !isEstimatingFee && token_symbol ? (
                  <InputAdornment position="end">µ{XTR_CURRENCY.symbol}</InputAdornment>
                ) : null,
            },
          }}
        />

        {isStealth && resource_address !== TARI_TOKEN && (
          <>
            <Divider />
            <Typography variant="subtitle2" color="text.secondary">
              Pay fee by pool swap (optional)
              <Tooltip
                title={
                  <>
                    Network fees (a.k.a gas) on Tari are paid in the native TARI token. If you don't have TARI, you can
                    optionally pay the fee by swapping a small amount of your {token_symbol} token for TARI in a swap
                    pool within the transfer transaction. The swap amount is calculated automatically from the estimated
                    fee and pool exchange rate.
                  </>
                }
                arrow
                placement="right"
              >
                <HelpOutlineIcon sx={{ fontSize: 16, color: "text.secondary", cursor: "help" }} />
              </Tooltip>
            </Typography>

            {isLoadingPools ? (
              <Stack direction="row" spacing={1} alignItems="center">
                <CircularProgress size={16} />
                <Typography variant="body2" color="text.secondary">
                  Loading pools...
                </Typography>
              </Stack>
            ) : (
              <>
                <InputLabel id="select-pool">Select Pool</InputLabel>
                <Select
                  id="select-pool"
                  value={showCustomPool ? "__custom__" : transferFormState.swapPoolAddress || ""}
                  displayEmpty
                  disabled={disabled}
                  onChange={(e) => {
                    const val = e.target.value as string;
                    if (val === "__custom__") {
                      setShowCustomPool(true);
                      onPoolSelect("");
                    } else if (val === "") {
                      setShowCustomPool(false);
                      onPoolSelect("");
                    } else {
                      setShowCustomPool(false);
                      onPoolSelect(val);
                    }
                  }}
                >
                  <MenuItem value="">
                    <Stack direction="row" spacing={1} alignItems="center">
                      <span>None (pay fee in TARI)</span>
                      {hasTariBalance && <Chip label="Recommended" size="small" color="success" variant="outlined" />}
                    </Stack>
                  </MenuItem>
                  {relevantPools.map((pool) => {
                    const isTokenA = pool.resource_a === resource_address;
                    const tokenBalance = isTokenA ? BigInt(pool.balance_a) : BigInt(pool.balance_b);
                    const tariBalance = isTokenA ? BigInt(pool.balance_b) : BigInt(pool.balance_a);
                    const rate = tokenBalance > 0n ? Number(tariBalance) / Number(tokenBalance) : 0;
                    return (
                      <MenuItem key={pool.pool_address} value={pool.pool_address}>
                        Swap: {pool.pool_address.substring(0, 16)}... (1 {token_symbol} = {rate.toFixed(4)} TARI)
                      </MenuItem>
                    );
                  })}
                  <MenuItem value="__custom__">Custom pool address...</MenuItem>
                </Select>
              </>
            )}

            {showCustomPool && (
              <TextField
                name="swapPoolAddress"
                label="Swap Pool Address"
                value={transferFormState.swapPoolAddress}
                onChange={(e) => {
                  onFormValueChange("swapPoolAddress", e.target.value);
                  // Fetch rate when user finishes typing (debounced via onBlur)
                }}
                onBlur={() => {
                  if (transferFormState.swapPoolAddress) {
                    onPoolSelect(transferFormState.swapPoolAddress);
                  }
                }}
                style={{ flexGrow: 1 }}
                disabled={disabled}
                error={!!poolError}
                helperText={poolError || "Enter a liquidity pool component address"}
              />
            )}

            {isLoadingPoolRate && (
              <Stack direction="row" spacing={1} alignItems="center">
                <CircularProgress size={16} />
                <Typography variant="body2" color="text.secondary">
                  Fetching pool rate...
                </Typography>
              </Stack>
            )}

            {poolRate && transferFormState.swapPoolAddress && !poolError && (
              <Alert severity={poolHasNoLiquidity ? "warning" : "info"} variant="outlined">
                {poolHasNoLiquidity
                  ? "This pool has no liquidity. Please select a different pool."
                  : `Pool exchange rate: ${formatPoolRate(poolRate, token_symbol)}`}
              </Alert>
            )}

            {poolError && !showCustomPool && (
              <Alert severity="error" variant="outlined">
                {poolError}
              </Alert>
            )}
          </>
        )}

        <Divider />

        <DisplayFormError forType="general" formError={formError} />
        <Stack direction="row" justifyContent="space-between" sx={{ mt: 3 }}>
          <Button variant="outlined" onClick={onCancel} disabled={disabled}>
            Cancel
          </Button>
          <Button variant="contained" type="submit" disabled={disabled || !isFormValid}>
            {isEstimatingFee ? "Estimating..." : "Continue"}
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
