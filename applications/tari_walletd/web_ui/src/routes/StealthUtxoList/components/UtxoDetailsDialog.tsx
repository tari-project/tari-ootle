// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import CopyToClipboard from "@components/CopyToClipboard";
import PopupTitle from "@components/PopupTitle";
import { Box, Dialog, Stack } from "@mui/material";
import { styled } from "@mui/material/styles";
import type { UtxoInfo } from "@tari-project/ootle-ts-bindings";
import { renderJson } from "@utils/helpers";

interface UtxoDetailsDialogProps {
  utxo: UtxoInfo | null;
  open: boolean;
  onClose: () => void;
}

const JsonBox = styled(Box)(({ theme }) => ({
  backgroundColor: theme.palette.accent.background,
  border: `1px solid ${theme.palette.accent.border}`,
  borderRadius: theme.spacing(1),
  padding: theme.spacing(2),
  maxHeight: "60vh",
  overflow: "auto",
  fontFamily: "monospace",
  fontSize: "0.8rem",
  wordBreak: "break-all",
}));

// `Amount` may arrive as a bigint, which neither JSON.stringify nor renderJson
// can serialise. Round-tripping through a bigint-aware replacer yields a plain,
// render-safe object whose JSON text matches what the copy button provides.
const bigintReplacer = (_key: string, value: unknown) => (typeof value === "bigint" ? value.toString() : value);

export default function UtxoDetailsDialog({ utxo, open, onClose }: UtxoDetailsDialogProps) {
  const jsonText = utxo ? JSON.stringify(utxo, bigintReplacer, 2) : "";
  const safeUtxo = jsonText ? JSON.parse(jsonText) : null;

  return (
    <Dialog open={open} onClose={onClose} maxWidth="md" fullWidth>
      <PopupTitle title="UTXO Details" onClose={onClose} />
      {safeUtxo && (
        <Box sx={{ px: 3, pb: 3 }}>
          <Stack direction="row" justifyContent="flex-end" sx={{ mb: 1 }}>
            <CopyToClipboard copy={jsonText} title="Copy JSON" iconWidth="18px" iconHeight="18px" />
          </Stack>
          <JsonBox className="json">{renderJson(safeUtxo)}</JsonBox>
        </Box>
      )}
    </Dialog>
  );
}
