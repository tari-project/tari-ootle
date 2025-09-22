// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { Stack, Typography, Button } from "@mui/material";
import { OutputStatus } from "@tari-project/typescript-bindings";

interface PlaceHolderProps {
  status: "empty" | "fetching";
  utxoStatus?: OutputStatus;
  onManualRefresh?: () => void;
}

function PlaceHolder({ status, utxoStatus, onManualRefresh }: PlaceHolderProps) {
  const EmptyPlaceHolder = () => {
    const getStatusDisplayName = (status: OutputStatus) => {
      switch (status) {
        case "LockedForSpend":
          return "Locked for Spend";
        case "LockedUnconfirmed":
          return "Locked Unconfirmed";
        default:
          return status;
      }
    };

    const getStatusMessage = () => {
      if (utxoStatus) {
        return `No ${getStatusDisplayName(utxoStatus)} UTXOs found`;
      }
      return "No UTXOs found";
    };

    const getStatusDescription = () => {
      if (utxoStatus) {
        return `You don't have any stealth UTXOs with "${getStatusDisplayName(utxoStatus)}" status in this account.`;
      }
      return "You don't have any Stealth UTXOs in this account yet.";
    };

    return (
      <Stack alignItems="center" justifyContent="center" sx={{ py: 8 }}>
        <Typography variant="h6" color="text.secondary" gutterBottom>
          {getStatusMessage()}
        </Typography>
        <Typography variant="body2" color="text.secondary">
          {getStatusDescription()}
        </Typography>
      </Stack>
    );
  };

  const FetchingPlaceHolder = () => (
    <Stack alignItems="center" justifyContent="center" sx={{ py: 8 }}>
      <Typography variant="h6" color="text.secondary" gutterBottom>
        UTXOs loading...
      </Typography>
      <Typography variant="body2" color="text.secondary">
        Please wait while we fetch your UTXOs from the wallet.
      </Typography>
      {onManualRefresh && (
        <Button variant="outlined" onClick={onManualRefresh} size="small" sx={{ mt: 2 }}>
          Click here to manually refresh
        </Button>
      )}
    </Stack>
  );

  return status === "empty" ? <EmptyPlaceHolder /> : <FetchingPlaceHolder />;
}

export default PlaceHolder;
