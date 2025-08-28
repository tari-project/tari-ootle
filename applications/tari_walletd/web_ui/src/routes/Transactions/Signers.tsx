//  Copyright 2022. The Tari Project
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

import { useState } from "react";
import { TableContainer, Table, TableHead, TableRow, TableCell, TableBody, Collapse, Box, Typography, Chip } from "@mui/material";
import { DataTableCell, AccordionIconButton } from "../../Components/StyledComponents";
import KeyboardArrowDownIcon from "@mui/icons-material/KeyboardArrowDown";
import KeyboardArrowUpIcon from "@mui/icons-material/KeyboardArrowUp";
import { useTheme } from "@mui/material/styles";
import { TransactionSignature } from "@tari-project/typescript-bindings";
import CopyAddress from "../../Components/CopyAddress";
import CodeBlockExpand from "../../Components/CodeBlock";

interface SignatureRowProps {
  publicKey: string;
  signature?: {
    public_nonce: string;
    signature: string;
  };
  isSealer?: boolean;
  index: number;
}

function SignatureRow({ publicKey, signature, isSealer = false, index }: SignatureRowProps) {
  const [open, setOpen] = useState(false);
  const theme = useTheme();

  return (
    <>
      <TableRow key={index}>
        <DataTableCell sx={{ borderTop: 1, borderTopColor: "divider", borderBottom: "none" }}>
          <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
            <Chip 
              label={isSealer ? "Seal Signer" : "Body Signer"} 
              size="small" 
              color={isSealer ? "primary" : "secondary"}
              variant="outlined" 
            />
          </Box>
        </DataTableCell>
        <DataTableCell sx={{ borderTop: 1, borderTopColor: "divider", borderBottom: "none" }}>
          <CopyAddress address={publicKey} />
        </DataTableCell>
        <DataTableCell sx={{ borderTop: 1, borderTopColor: "divider", borderBottom: "none" }}>
          {signature ? (
            <Chip 
              label="Signed" 
              size="small" 
              color="success" 
              variant="outlined" 
            />
          ) : (
            <Chip 
              label="No Signature" 
              size="small" 
              color="default" 
              variant="outlined" 
            />
          )}
        </DataTableCell>
        <DataTableCell sx={{ borderTop: 1, borderTopColor: "divider", borderBottom: "none", textAlign: "center" }}>
          {signature && (
            <AccordionIconButton
              aria-label="expand row"
              size="small"
              onClick={() => {
                setOpen(!open);
              }}
            >
              {open ? <KeyboardArrowUpIcon /> : <KeyboardArrowDownIcon />}
            </AccordionIconButton>
          )}
        </DataTableCell>
      </TableRow>
      {signature && (
        <TableRow>
          <DataTableCell
            style={{
              paddingBottom: theme.spacing(1),
              paddingTop: 0,
              borderBottom: "none",
            }}
            colSpan={4}
          >
            <Collapse in={open} timeout="auto" unmountOnExit>
              <Box sx={{ p: 2, backgroundColor: theme.palette.accent.background, borderRadius: 1 }}>
                <Typography variant="subtitle2" sx={{ mb: 2 }}>Signature Details</Typography>
                
                <Box sx={{ display: "flex", flexDirection: "column", gap: 2 }}>
                  <Box>
                    <Typography variant="body2" color="text.secondary" sx={{ mb: 1 }}>
                      Public Nonce:
                    </Typography>
                    <CopyAddress address={signature.public_nonce} />
                  </Box>
                  
                  <Box>
                    <Typography variant="body2" color="text.secondary" sx={{ mb: 1 }}>
                      Signature:
                    </Typography>
                    <CopyAddress address={signature.signature} />
                  </Box>
                </Box>
                
                <Box sx={{ mt: 2 }}>
                  <CodeBlockExpand title="Raw Signature Data" content={signature} />
                </Box>
              </Box>
            </Collapse>
          </DataTableCell>
        </TableRow>
      )}
    </>
  );
}

interface SignersProps {
  bodySignatures?: TransactionSignature[];
  sealSignature?: {
    public_key: string;
    signature: {
      public_nonce: string;
      signature: string;
    };
  };
}

export default function Signers({ bodySignatures, sealSignature }: SignersProps) {
  const hasSignatures = (bodySignatures && bodySignatures.length > 0) || sealSignature;

  if (!hasSignatures) {
    return (
      <Box sx={{ p: 3, textAlign: "center" }}>
        <Typography variant="body2" color="text.secondary">
          No signers found for this transaction
        </Typography>
      </Box>
    );
  }

  return (
    <TableContainer>
      <Table>
        <TableHead>
          <TableRow>
            <TableCell>Type</TableCell>
            <TableCell>Public Key</TableCell>
            <TableCell>Status</TableCell>
            <TableCell width={90}>Details</TableCell>
          </TableRow>
        </TableHead>
        <TableBody>
          {sealSignature && (
            <SignatureRow
              publicKey={sealSignature.public_key}
              signature={sealSignature.signature}
              isSealer={true}
              index={-1}
            />
          )}
          {bodySignatures?.map((signature: TransactionSignature, index: number) => (
            <SignatureRow
              key={index}
              publicKey={signature.public_key}
              isSealer={false}
              index={index}
            />
          ))}
        </TableBody>
      </Table>
    </TableContainer>
  );
}