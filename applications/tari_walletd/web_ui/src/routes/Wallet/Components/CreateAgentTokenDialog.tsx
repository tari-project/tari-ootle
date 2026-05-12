//  Copyright 2026 The Tari Project
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
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

import { useCreateAgentToken } from "@api/hooks/useAgentTokens";
import { CodeBlock } from "@components/StyledComponents";
import WarningAmberRoundedIcon from "@mui/icons-material/WarningAmberRounded";
import {
  Alert,
  Box,
  Button,
  Chip,
  Checkbox,
  Dialog,
  DialogActions,
  DialogContent,
  DialogContentText,
  DialogTitle,
  Divider,
  FormControlLabel,
  Slider,
  Stack,
  TextField,
  ToggleButton,
  ToggleButtonGroup,
  Typography,
} from "@mui/material";
import { alpha, useTheme } from "@mui/material/styles";
import type { JrpcPermission } from "@tari-project/ootle-ts-bindings";
import { useEffect, useMemo, useState } from "react";

type ExpiryPreset = "24h" | "7d" | "30d" | "custom";

interface PermissionOption {
  scope: string;
  access: "read" | "write";
  disabled?: boolean;
  description?: string;
  buildPermission?: (context: { accountBalanceSubstateId: string }) => JrpcPermission | null;
}

const PERMISSION_OPTIONS: PermissionOption[] = [
  { scope: "accounts.info", access: "read", buildPermission: () => "AccountInfo" },
  {
    scope: "nfts.ownership_proof",
    access: "read",
    buildPermission: () => ({ NftGetOwnershipProof: null }),
  },
  {
    scope: "accounts.balance",
    access: "read",
    description: "Requires a specific account substate id.",
    buildPermission: ({ accountBalanceSubstateId }) =>
      accountBalanceSubstateId.trim() ? { AccountBalance: accountBalanceSubstateId.trim() } : null,
  },
  { scope: "accounts.list", access: "read", buildPermission: () => ({ AccountList: null }) },
  { scope: "substates.read", access: "read", buildPermission: () => "SubstatesRead" },
  { scope: "templates.read", access: "read", buildPermission: () => "TemplatesRead" },
  { scope: "keys.view", access: "read", buildPermission: () => "KeyList" },
  { scope: "transactions.get", access: "read", buildPermission: () => "TransactionGet" },
  { scope: "transfer.send", access: "write", buildPermission: () => ({ TransactionSend: null }) },
  { scope: "nfts.get", access: "read", buildPermission: () => ({ GetNft: [null, null] }) },
  { scope: "webrtc.start", access: "write", buildPermission: () => "StartWebrtc" },
  { scope: "address_book.read", access: "read", buildPermission: () => ({ AddressBook: "Read" }) },
  { scope: "address_book.create", access: "write", buildPermission: () => ({ AddressBook: "Create" }) },
  { scope: "address_book.update", access: "write", buildPermission: () => ({ AddressBook: "Update" }) },
  { scope: "address_book.delete", access: "write", buildPermission: () => ({ AddressBook: "Delete" }) },
  { scope: "admin", access: "write", buildPermission: () => "Admin" },
];

const DEFAULT_MAX_JWT_MINUTES = 15;
const EXPIRY_CONTROLS_ENABLED = false;

interface CreateAgentTokenDialogProps {
  open: boolean;
  onClose: () => void;
  onCreated: () => void;
}

export default function CreateAgentTokenDialog({ open, onClose, onCreated }: CreateAgentTokenDialogProps) {
  const theme = useTheme();
  const [label, setLabel] = useState("");
  const [selectedScopes, setSelectedScopes] = useState<string[]>(["accounts.info", "nfts.ownership_proof"]);
  const [accountBalanceSubstateId, setAccountBalanceSubstateId] = useState("");
  const [expiryPreset, setExpiryPreset] = useState<ExpiryPreset>("7d");
  const [customHours, setCustomHours] = useState("168");
  const [maxJwtMinutes, setMaxJwtMinutes] = useState(DEFAULT_MAX_JWT_MINUTES);
  const [adminConfirmOpen, setAdminConfirmOpen] = useState(false);
  const [copied, setCopied] = useState(false);
  const [inlineError, setInlineError] = useState<string | null>(null);
  const [createdKey, setCreatedKey] = useState<string | null>(null);
  const { mutateAsync, isPending } = useCreateAgentToken();

  const selectedPermissions = useMemo(
    () =>
      PERMISSION_OPTIONS.flatMap((option) => {
        if (!selectedScopes.includes(option.scope)) {
          return [];
        }
        const permission = option.buildPermission?.({ accountBalanceSubstateId });
        return permission ? [permission] : [];
      }),
    [accountBalanceSubstateId, selectedScopes],
  );
  const hasAdmin = selectedScopes.includes("admin");
  const requiresAccountBalanceScope = selectedScopes.includes("accounts.balance");
  const isSubmitDisabled =
    !label.trim() ||
    selectedScopes.length === 0 ||
    selectedPermissions.length !== selectedScopes.length ||
    isPending;

  useEffect(() => {
    if (!open) {
      resetState();
    }
  }, [open]);

  const helperText = useMemo(() => {
    if (expiryPreset === "custom") {
      return "Expiry is UI-only for now. The wallet daemon does not yet accept max_lifetime_secs.";
    }
    return "Expiry is UI-only for now. Server-side max_lifetime_secs is not yet supported.";
  }, [expiryPreset]);

  const handleTogglePermission = (permission: JrpcPermission) => {
    const option = PERMISSION_OPTIONS.find((item) => item.buildPermission?.({ accountBalanceSubstateId: "" }) === permission);
    if (!option) {
      return;
    }
    handleToggleScope(option.scope);
  };

  const handleToggleScope = (scope: string) => {
    setInlineError(null);
    setSelectedScopes((current) =>
      current.includes(scope) ? current.filter((item) => item !== scope) : [...current, scope],
    );
    if (scope === "accounts.balance" && selectedScopes.includes(scope)) {
      setAccountBalanceSubstateId("");
    }
  };

  const handleSubmit = async () => {
    setInlineError(null);
    try {
      const response = await mutateAsync({
        name: label.trim(),
        permissions: selectedPermissions,
        grantAdmin: hasAdmin,
      });
      setCreatedKey(response.key);
      setCopied(false);
    } catch (error) {
      const message = error instanceof Error ? error.message : "Failed to create agent token";
      setInlineError(message);
    }
  };

  const handleGenerateClick = async () => {
    if (hasAdmin) {
      setAdminConfirmOpen(true);
      return;
    }
    await handleSubmit();
  };

  const handleCopy = async () => {
    if (!createdKey) return;
    try {
      await navigator.clipboard.writeText(createdKey);
      setCopied(true);
    } catch (error) {
      const message = error instanceof Error ? error.message : "Failed to copy key";
      setInlineError(message);
    }
  };

  const handleDone = () => {
    onCreated();
    onClose();
    resetState();
  };

  const resetState = () => {
    setLabel("");
    setSelectedScopes(["accounts.info", "nfts.ownership_proof"]);
    setAccountBalanceSubstateId("");
    setExpiryPreset("7d");
    setCustomHours("168");
    setMaxJwtMinutes(DEFAULT_MAX_JWT_MINUTES);
    setAdminConfirmOpen(false);
    setCopied(false);
    setInlineError(null);
    setCreatedKey(null);
  };

  return (
    <>
      <Dialog open={open} onClose={createdKey ? undefined : onClose} fullWidth maxWidth="sm">
        <DialogTitle sx={{ fontSize: "2rem", pb: 1 }}>
          {createdKey ? "Agent token created" : "Create agent token"}
          <Typography variant="body1" color="text.secondary" sx={{ mt: 0.5 }}>
            Generate a scoped API token for programmatic access
          </Typography>
        </DialogTitle>
        <Divider />
        <DialogContent sx={{ py: 3 }}>
          {createdKey ? (
            <Stack spacing={2.5}>
              <Alert severity="warning" icon={<WarningAmberRoundedIcon fontSize="inherit" />}>
                <strong>This key will never be shown again. Copy it now.</strong>
              </Alert>
              <CodeBlock
                sx={{
                  fontFamily: "'Courier New', Courier, monospace",
                  fontSize: "0.95rem",
                  wordBreak: "break-all",
                  maxHeight: "unset",
                }}
              >
                {createdKey}
              </CodeBlock>
              <Button variant={copied ? "contained" : "outlined"} onClick={handleCopy}>
                {copied ? "Copied" : "Copy to clipboard"}
              </Button>
            </Stack>
          ) : (
            <Stack spacing={3}>
              <TextField
                label="Label"
                placeholder="claude-agent-payments"
                value={label}
                onChange={(event) => setLabel(event.target.value.slice(0, 64))}
                required
                fullWidth
                slotProps={{ htmlInput: { maxLength: 64 } }}
              />

              <Box>
                <Typography variant="subtitle1" sx={{ mb: 1.25 }}>
                  Permissions
                </Typography>
                <Stack spacing={1.25}>
                  {PERMISSION_OPTIONS.map((option) => {
                    const checked = selectedScopes.includes(option.scope);
                    const showAccountBalanceInput = option.scope === "accounts.balance" && checked;
                    return (
                      <Stack key={option.scope} spacing={1}>
                        <Box
                          onClick={() => {
                            if (!option.disabled) {
                              handleToggleScope(option.scope);
                            }
                          }}
                          sx={{
                            display: "flex",
                            alignItems: "center",
                            justifyContent: "space-between",
                            gap: 2,
                            px: 1.25,
                            py: 0.75,
                            borderRadius: 2,
                            border: `1px solid ${
                              checked ? alpha(theme.palette.primary.main, 0.4) : theme.palette.divider
                            }`,
                            backgroundColor: checked
                              ? alpha(theme.palette.primary.main, 0.12)
                              : theme.palette.background.paper,
                            cursor: option.disabled ? "not-allowed" : "pointer",
                            opacity: option.disabled ? 0.65 : 1,
                            transition: "background-color 0.2s ease, border-color 0.2s ease",
                          }}
                        >
                          <FormControlLabel
                            control={
                              <Checkbox
                                checked={checked}
                                disabled={option.disabled}
                                onChange={() => handleToggleScope(option.scope)}
                              />
                            }
                            label={
                              <Stack spacing={0.5}>
                                <Typography
                                  sx={{
                                    color: checked ? theme.palette.primary.main : theme.palette.text.primary,
                                    fontWeight: checked ? 600 : 400,
                                  }}
                                >
                                  {option.scope}
                                </Typography>
                                {option.description && (
                                  <Typography variant="caption" color="text.secondary">
                                    {option.description}
                                  </Typography>
                                )}
                              </Stack>
                            }
                            sx={{ m: 0, flexGrow: 1 }}
                            onClick={(event) => event.stopPropagation()}
                          />
                          <Stack direction="row" spacing={1} alignItems="center">
                            <Box
                              sx={{
                                minWidth: 58,
                                textAlign: "right",
                                color: "text.secondary",
                                fontSize: "0.85rem",
                                textTransform: "lowercase",
                              }}
                            >
                              {option.access}
                            </Box>
                          </Stack>
                        </Box>
                        {showAccountBalanceInput && (
                          <TextField
                            label="Account substate id"
                            placeholder="component_..."
                            value={accountBalanceSubstateId}
                            onChange={(event) => {
                              setInlineError(null);
                              setAccountBalanceSubstateId(event.target.value);
                            }}
                            required
                            fullWidth
                            helperText="Paste the account component substate id to scope balance access."
                          />
                        )}
                      </Stack>
                    );
                  })}
                </Stack>
                {hasAdmin && (
                  <Alert severity="warning" sx={{ mt: 1.5 }}>
                    Granting Admin scope gives full wallet access
                  </Alert>
                )}
                {requiresAccountBalanceScope && !accountBalanceSubstateId.trim() && (
                  <Alert severity="info" sx={{ mt: 1.5 }}>
                    Account balance access requires an account substate id before this token can be created.
                  </Alert>
                )}
              </Box>

              <Box>
                <Typography variant="subtitle1" sx={{ mb: 1.25 }}>
                  Expiry
                </Typography>
                <Stack spacing={1.5}>
                  <ToggleButtonGroup
                    value={expiryPreset}
                    exclusive
                    disabled={!EXPIRY_CONTROLS_ENABLED}
                    fullWidth
                    onChange={(_, value: ExpiryPreset | null) => {
                      if (value) setExpiryPreset(value);
                    }}
                    color="primary"
                  >
                    <ToggleButton value="24h">24h</ToggleButton>
                    <ToggleButton value="7d">7 days</ToggleButton>
                    <ToggleButton value="30d">30 days</ToggleButton>
                    <ToggleButton value="custom">Custom</ToggleButton>
                  </ToggleButtonGroup>
                  {expiryPreset === "custom" && (
                    <TextField
                      label="Custom hours"
                      type="number"
                      disabled={!EXPIRY_CONTROLS_ENABLED}
                      value={customHours}
                      onChange={(event) => setCustomHours(event.target.value)}
                      slotProps={{ htmlInput: { min: 1 } }}
                    />
                  )}
                  <Typography variant="caption" color="text.secondary">
                    {helperText}
                  </Typography>
                </Stack>
              </Box>

              <Box>
                <Stack direction="row" justifyContent="space-between" alignItems="center" sx={{ mb: 1 }}>
                  <Typography variant="subtitle1">Max JWT lifetime</Typography>
                  <Typography variant="subtitle1">{maxJwtMinutes} min</Typography>
                </Stack>
                <Slider
                  min={1}
                  max={60}
                  step={1}
                  disabled={!EXPIRY_CONTROLS_ENABLED}
                  value={maxJwtMinutes}
                  onChange={(_, value) => setMaxJwtMinutes(value as number)}
                  valueLabelDisplay="auto"
                />
                <Typography variant="caption" color="text.secondary">
                  Agents re-exchange the token for a new JWT after this window
                </Typography>
                <Typography variant="caption" color="text.secondary" display="block" sx={{ mt: 0.5 }}>
                  Max JWT lifetime is UI-only for now.
                </Typography>
              </Box>

              {inlineError && <Alert severity="error">{inlineError}</Alert>}
            </Stack>
          )}
        </DialogContent>
        <Divider />
        <DialogActions sx={{ px: 3, py: 2 }}>
          {createdKey ? (
            <Button variant="contained" onClick={handleDone}>
              Done
            </Button>
          ) : (
            <>
              <Button variant="outlined" onClick={onClose}>
                Cancel
              </Button>
              <Button variant="contained" onClick={handleGenerateClick} disabled={isSubmitDisabled}>
                {isPending ? "Generating..." : "Generate token"}
              </Button>
            </>
          )}
        </DialogActions>
      </Dialog>

      <Dialog open={adminConfirmOpen} onClose={() => setAdminConfirmOpen(false)} maxWidth="xs" fullWidth>
        <DialogTitle>Confirm Admin scope</DialogTitle>
        <DialogContent>
          <DialogContentText>
            You are granting full Admin access to this agent. Continue?
          </DialogContentText>
        </DialogContent>
        <DialogActions>
          <Button variant="outlined" onClick={() => setAdminConfirmOpen(false)}>
            Cancel
          </Button>
          <Button
            variant="contained"
            onClick={async () => {
              setAdminConfirmOpen(false);
              await handleSubmit();
            }}
          >
            Continue
          </Button>
        </DialogActions>
      </Dialog>
    </>
  );
}
