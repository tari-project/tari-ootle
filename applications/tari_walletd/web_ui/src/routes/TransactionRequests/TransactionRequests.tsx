//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import { useAccountsList } from "@api/hooks/useAccounts";
import {
  useApproveTransactionRequest,
  useListTransactionRequests,
  useRejectTransactionRequest,
  useSubmitTransactionRequest,
} from "@api/hooks/useTransactionRequests";
import { Accordion, AccordionDetails, AccordionSummary } from "@components/Accordion";
import ComponentUpgradeWarning from "@components/ComponentUpgradeWarning";
import FetchStatusCheck from "@components/FetchStatusCheck";
import PageHeading from "@components/PageHeading";
import { StyledPaper } from "@components/StyledComponents";
import Alert from "@mui/material/Alert";
import Button from "@mui/material/Button";
import Chip from "@mui/material/Chip";
import CircularProgress from "@mui/material/CircularProgress";
import Divider from "@mui/material/Divider";
import Grid from "@mui/material/Grid";
import Stack from "@mui/material/Stack";
import Typography from "@mui/material/Typography";
import type {
  EffectiveStatus,
  KeyId,
  TransactionRequestInfo,
  TransactionRequestMainSigner,
} from "@tari-project/ootle-ts-bindings";
import { TARI_TOKEN } from "@tari-project/ootle-ts-bindings";
import { useEffect, useState } from "react";
import type { AccountWithdrawal, InstructionsDescription, WithdrawalDestination } from "../SigningRequests/describe";
import { describeInstructions } from "../SigningRequests/describe";
import Inputs from "../Transactions/Inputs";
import Instructions, { BlobContents } from "../Transactions/Instructions";

function formatKeyId(keyId: KeyId): string {
  if ("Derived" in keyId) {
    return `${keyId.Derived.key_branch.replace(/_/g, " ")} key #${keyId.Derived.index}`;
  }
  return `imported key #${keyId.Imported.local_key_id}`;
}

function describeMainSigner(mainSigner: TransactionRequestMainSigner): {
  label: string;
  publicKey?: string;
  color: "default" | "warning" | "error";
} {
  if (mainSigner === "SealSigner") {
    return { label: "the seal signer", color: "warning" };
  }
  if (mainSigner === "None") {
    return { label: "none", color: "default" };
  }
  if ("WalletKey" in mainSigner) {
    return { label: "a key of this wallet", publicKey: mainSigner.WalletKey.public_key, color: "warning" };
  }
  return { label: "a key outside this wallet", publicKey: mainSigner.External.public_key, color: "error" };
}

function statusColor(status: EffectiveStatus): "default" | "warning" | "success" | "error" | "info" {
  switch (status) {
    case "Pending":
      return "warning";
    case "Approved":
      return "info";
    case "Submitting":
      return "info";
    case "Submitted":
      return "success";
    case "Rejected":
      return "error";
    default:
      return "default";
  }
}

function Countdown({ expiresAt }: { expiresAt: bigint }) {
  const [now, setNow] = useState(() => Date.now() / 1000);
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now() / 1000), 1000);
    return () => clearInterval(t);
  }, []);

  const remaining = Math.max(0, Math.floor(Number(expiresAt) - now));
  const mins = Math.floor(remaining / 60);
  const secs = remaining % 60;
  return <Chip label={`expires in ${mins}:${secs.toString().padStart(2, "0")}`} size="small" variant="outlined" />;
}

function ValueSummaries({ request }: { request: TransactionRequestInfo }) {
  // A stealth transfer's instructions are commitments and range proofs, so a
  // person cannot read an amount out of them. The wallet owns the masks and is
  // the only party that can say what of its stealth outputs leaves. Revealed
  // amounts are read from the instructions themselves (`InstructionSummary`).
  return (
    <>
      {request.value_summaries.map(({ amount_leaving, inputs_total, change_total, resource_address }) => {
        const unit = unitOf(resource_address);
        return (
          <Alert key={resource_address} severity="warning" icon={false} sx={{ mb: 2 }}>
            <Typography variant="h6" sx={{ fontWeight: 600 }}>
              {amount_leaving.toLocaleString()} {unit} of stealth funds leaves this wallet
            </Typography>
            <Typography variant="body2" sx={{ opacity: 0.85 }}>
              {inputs_total.toLocaleString()} {unit} spent, {change_total.toLocaleString()} {unit} returned as change
            </Typography>
            <Typography variant="body2" sx={{ opacity: 0.7, wordBreak: "break-all" }}>
              {resource_address}
            </Typography>
          </Alert>
        );
      })}
    </>
  );
}

function unitOf(resource: string): string {
  // Amounts are in the resource's smallest unit, which is µT only for TARI.
  return resource === TARI_TOKEN ? "µT" : "base units";
}

function withdrawnText({ resource, value }: AccountWithdrawal): string {
  switch (value.kind) {
    case "amount":
      return `${value.amount.toLocaleString()} ${unitOf(resource)}`;
    case "all":
      return "The entire balance";
    case "non_fungibles":
      return `${value.count} non-fungible token(s)`;
    case "confidential":
      return "A confidential amount";
  }
}

function destinationText(destination: WithdrawalDestination): string {
  switch (destination.kind) {
    case "deposit":
      return destination.isOwnAccount
        ? `to your account ${destination.component}`
        : `to ${destination.component}, which is not an account of this wallet`;
    case "fee":
      return "to pay the transaction fee";
    case "unknown":
      return "to a destination these instructions do not show directly; review them below";
  }
}

function needsReview(description: InstructionsDescription): boolean {
  return description.undescribed > 0 || description.withdrawals.some((w) => w.destination.kind === "unknown");
}

/// The panel to open for a pending request: the first instruction list the
/// summary does not fully account for.
function reviewPanel(main: InstructionsDescription, fee: InstructionsDescription): string | null {
  if (needsReview(main)) {
    return "instructions";
  }
  if (needsReview(fee)) {
    return "fees";
  }
  return null;
}

/// What the transaction moves out of this wallet's accounts, for the
/// instructions this page knows how to read, and how many it does not.
function InstructionSummary({ main, fee }: { main: InstructionsDescription; fee: InstructionsDescription }) {
  const withdrawals = [...fee.withdrawals, ...main.withdrawals];
  const fees = [...fee.fees, ...main.fees];
  return (
    <>
      {withdrawals.map((withdrawal, i) => (
        <Alert key={`w${i}`} severity="warning" icon={false} sx={{ mb: 2 }}>
          <Typography variant="h6" sx={{ fontWeight: 600 }}>
            {withdrawnText(withdrawal)} leaves account {withdrawal.account}
          </Typography>
          <Typography variant="body2" sx={{ opacity: 0.85, wordBreak: "break-all" }}>
            {destinationText(withdrawal.destination)}
          </Typography>
          <Typography variant="body2" sx={{ opacity: 0.7, wordBreak: "break-all" }}>
            {withdrawal.resource}
          </Typography>
        </Alert>
      ))}
      {fees.length > 0 && (
        <Alert severity="info" icon={false} sx={{ mb: 2 }}>
          {fees.map(({ account, amount }, i) => (
            <Typography key={i} variant="body2" sx={{ wordBreak: "break-all" }}>
              Fee of up to {amount.toLocaleString()} µT, paid by account {account}
            </Typography>
          ))}
        </Alert>
      )}
      {(main.undescribed > 0 || fee.undescribed > 0) && (
        <Alert severity="warning" sx={{ mb: 2 }}>
          {main.undescribed > 0 && (
            <Typography variant="body2">
              {main.undescribed} instruction(s) not described above; review them below.
            </Typography>
          )}
          {fee.undescribed > 0 && (
            <Typography variant="body2">
              {fee.undescribed} fee instruction(s) not described above; review them below.
            </Typography>
          )}
        </Alert>
      )}
    </>
  );
}

function RequestCard({ request }: { request: TransactionRequestInfo }) {
  const approve = useApproveTransactionRequest();
  const reject = useRejectTransactionRequest();
  const submit = useSubmitTransactionRequest();
  const { data: accountsData } = useAccountsList(0, 1000);
  // `undefined` until the user opens or closes a panel; until then the panel
  // holding instructions this card cannot describe is open.
  const [expanded, setExpanded] = useState<string | null | undefined>(undefined);
  const isActionable = request.status === "Pending";
  // Approve and submit are separate permissions and separate RPCs, but this UI
  // holds both, so approving chains straight into submitting. If the submit
  // half fails (or another principal beat it), the request stays Approved and
  // this standalone button finishes the job.
  const isSubmittable = request.status === "Approved";
  const busy = approve.isPending || reject.isPending || submit.isPending;
  // A mutation error is only worth showing while the decision is still open.
  // Two principals can race (an approve here vs. a tool's submit); the loser's
  // error describes a state this card no longer displays once the list
  // refetches, so it would read as the action having failed outright.
  const error = isActionable || isSubmittable ? (approve.error ?? reject.error ?? submit.error) : null;

  const params = { request_id: request.request_id };

  const v1 = request.transaction.V1;
  const instructions = v1?.instructions ?? [];
  const feeInstructions = v1?.fee_instructions ?? [];
  const inputs = v1?.inputs ?? [];
  const blobs = v1?.blobs ?? [];
  const isSealSignerAuthorized = v1?.main_signer === "SealSigner";
  const mainSigner = describeMainSigner(request.main_signer);

  const ownAccounts = new Set((accountsData?.accounts ?? []).map(({ account }) => account.component_address));
  const mainDescription = describeInstructions(instructions, blobs, ownAccounts);
  const feeDescription = describeInstructions(feeInstructions, blobs, ownAccounts);
  const defaultPanel = isActionable ? reviewPanel(mainDescription, feeDescription) : null;
  const openPanel = expanded === undefined ? defaultPanel : expanded;

  const togglePanel = (panel: string) => (_event: React.SyntheticEvent, isExpanded: boolean) =>
    setExpanded(isExpanded ? panel : null);

  return (
    <StyledPaper sx={{ mb: 2 }}>
      <Stack direction="row" justifyContent="space-between" alignItems="center" sx={{ mb: 1 }} flexWrap="wrap" gap={1}>
        <Typography variant="h5">
          {request.requested_by === "local-cookie"
            ? "A local client"
            : request.requested_by
              ? `"${request.requested_by}"`
              : "A wallet session"}{" "}
          requests approval
        </Typography>
        <Stack direction="row" spacing={1} alignItems="center">
          {isActionable && <Countdown expiresAt={request.expires_at} />}
          <Chip label={request.status} size="small" color={statusColor(request.status)} />
        </Stack>
      </Stack>

      <ComponentUpgradeWarning upgrades={request.component_upgrades} />
      <ValueSummaries request={request} />
      <InstructionSummary main={mainDescription} fee={feeDescription} />

      <Grid container spacing={2} sx={{ mb: 1 }}>
        <Grid size={12}>
          <Typography variant="body2" sx={{ opacity: 0.7 }}>
            Seal signer
          </Typography>
          <Stack direction="row" spacing={1} alignItems="center" flexWrap="wrap">
            <Typography variant="body2">{formatKeyId(request.seal_signer)}</Typography>
            {/* Not an implementation detail: when set, the seal signature also
                authorises the transaction, lending the sealer's account
                authority to these instructions. */}
            <Chip
              label={isSealSignerAuthorized ? "authorized" : "not authorized"}
              size="small"
              color={isSealSignerAuthorized ? "warning" : "default"}
              variant="outlined"
            />
          </Stack>
        </Grid>
        <Grid size={12}>
          <Typography variant="body2" sx={{ opacity: 0.7 }}>
            Main signer
          </Typography>
          {/* Whatever the transaction creates as "owned by signer" belongs to
              this key, and templates that ask for the transaction's signer
              see it. */}
          <Stack direction="row" spacing={1} alignItems="center" flexWrap="wrap">
            <Chip label={mainSigner.label} size="small" color={mainSigner.color} variant="outlined" />
            {mainSigner.publicKey && (
              <Typography variant="body2" sx={{ fontFamily: "monospace", wordBreak: "break-all" }}>
                {mainSigner.publicKey}
              </Typography>
            )}
          </Stack>
        </Grid>
        {request.external_signers.length > 0 && (
          <Grid size={12}>
            <Typography variant="body2" sx={{ opacity: 0.7 }}>
              Also signed by ({request.external_signers.length} key(s) outside this wallet)
            </Typography>
            {request.external_signers.map((publicKey) => (
              <Typography key={publicKey} variant="body2" sx={{ fontFamily: "monospace", wordBreak: "break-all" }}>
                {publicKey}
              </Typography>
            ))}
          </Grid>
        )}
      </Grid>

      <Accordion expanded={openPanel === "instructions"} onChange={togglePanel("instructions")}>
        <AccordionSummary aria-controls="request-instructions-content">
          <Typography variant="h6">Instructions ({instructions.length})</Typography>
        </AccordionSummary>
        <AccordionDetails>
          <Instructions data={instructions} blobs={blobs} />
        </AccordionDetails>
      </Accordion>
      <Accordion expanded={openPanel === "fees"} onChange={togglePanel("fees")}>
        <AccordionSummary aria-controls="request-fee-instructions-content">
          <Typography variant="h6">Fee Instructions ({feeInstructions.length})</Typography>
        </AccordionSummary>
        <AccordionDetails>
          <Instructions data={feeInstructions} blobs={blobs} />
        </AccordionDetails>
      </Accordion>
      <Accordion expanded={openPanel === "blobs"} onChange={togglePanel("blobs")}>
        <AccordionSummary aria-controls="request-blobs-content">
          <Typography variant="h6">Blobs ({blobs.length})</Typography>
        </AccordionSummary>
        <AccordionDetails>
          <BlobContents blobs={blobs} />
        </AccordionDetails>
      </Accordion>
      <Accordion expanded={openPanel === "inputs"} onChange={togglePanel("inputs")}>
        <AccordionSummary aria-controls="request-inputs-content">
          <Typography variant="h6">Inputs ({inputs.length})</Typography>
        </AccordionSummary>
        <AccordionDetails>
          <Inputs data={inputs} />
        </AccordionDetails>
      </Accordion>

      {error && (
        <Alert severity="error" sx={{ mt: 1 }}>
          {error.message}
        </Alert>
      )}

      {(isActionable || isSubmittable) && (
        <>
          <Divider sx={{ my: 2 }} />
          <Stack direction="row" spacing={1} justifyContent="flex-end">
            {isActionable && (
              <>
                <Button
                  variant="outlined"
                  color="error"
                  disabled={busy}
                  startIcon={reject.isPending ? <CircularProgress size={16} color="inherit" /> : undefined}
                  onClick={() => reject.mutate(params)}
                >
                  Reject
                </Button>
                <Button
                  variant="contained"
                  disabled={busy}
                  startIcon={
                    approve.isPending || submit.isPending ? <CircularProgress size={16} color="inherit" /> : undefined
                  }
                  onClick={() => approve.mutate(params, { onSuccess: () => submit.mutate(params) })}
                >
                  Approve &amp; Submit
                </Button>
              </>
            )}
            {isSubmittable && (
              <Button
                variant="contained"
                disabled={busy}
                startIcon={submit.isPending ? <CircularProgress size={16} color="inherit" /> : undefined}
                onClick={() => submit.mutate({ request_id: request.request_id })}
              >
                Submit
              </Button>
            )}
          </Stack>
        </>
      )}
    </StyledPaper>
  );
}

export default function TransactionRequests() {
  const { data, isFetching, isError, error } = useListTransactionRequests();

  const requests = data?.requests ?? [];
  const pending = requests.filter((r) => r.status === "Pending");
  const rest = requests.filter((r) => r.status !== "Pending");

  return (
    <Grid container spacing={5}>
      <Grid size={12}>
        <PageHeading>Transaction Requests</PageHeading>
      </Grid>
      <Grid size={12}>
        <FetchStatusCheck isLoading={isFetching && !data} isError={isError} errorMessage={error?.message ?? ""}>
          {pending.length === 0 && (
            <StyledPaper>
              <Typography variant="body1" sx={{ opacity: 0.7 }}>
                No pending requests.
              </Typography>
            </StyledPaper>
          )}
          {pending.map((r) => (
            <RequestCard key={r.request_id} request={r} />
          ))}
          {rest.length > 0 && (
            <>
              <Typography variant="h5" sx={{ mt: 4, mb: 2 }}>
                History
              </Typography>
              {rest.map((r) => (
                <RequestCard key={r.request_id} request={r} />
              ))}
            </>
          )}
        </FetchStatusCheck>
      </Grid>
    </Grid>
  );
}
