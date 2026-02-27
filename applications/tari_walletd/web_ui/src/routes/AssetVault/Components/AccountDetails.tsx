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

import AccountName from "@/components/AccountName";
import CopyAddress from "@components/CopyAddress";
import { DataTableCell } from "@components/StyledComponents";
import {
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogContentText,
  DialogTitle,
  Stack,
  TextField,
  Tooltip,
} from "@mui/material";
import IconButton from "@mui/material/IconButton";
import Table from "@mui/material/Table";
import TableBody from "@mui/material/TableBody";
import TableCell from "@mui/material/TableCell";
import TableContainer from "@mui/material/TableContainer";
import TableHead from "@mui/material/TableHead";
import TableRow from "@mui/material/TableRow";
import Typography from "@mui/material/Typography";
import useAccountStore from "@store/accountStore";
import {
  decodeOotleAddress,
  encodeOotleAddress,
  OotleAddress,
  substateIdToString,
} from "@tari-project/ootle-ts-bindings";
import { useState } from "react";
import { IoPaperPlaneOutline } from "react-icons/io5";
import QRCode from "react-qr-code";

function AccountDetails() {
  const [payRefDialogOpen, setPayRefDialogOpen] = useState(false);
  const { account, address, setAccount } = useAccountStore();

  if (!account) {
    return <>Loading...</>;
  }

  const handleRenameSuccess = (newName: string) => {
    setAccount({ ...account, name: newName });
  };

  return (
    <TableContainer>
      <PayRefDialog address={address} open={payRefDialogOpen} onClose={() => setPayRefDialogOpen(false)} />
      <Table>
        <TableHead>
          <TableRow>
            <TableCell>Name</TableCell>
            <TableCell>Component</TableCell>
            <TableCell>Address</TableCell>
          </TableRow>
        </TableHead>
        <TableBody>
          <TableRow>
            <DataTableCell>
              <AccountName
                accountAddress={substateIdToString(account.component_address)}
                currentName={account?.name}
                onRenameSuccess={handleRenameSuccess}
              />
            </DataTableCell>
            <DataTableCell>
              <CopyAddress address={substateIdToString(account.component_address)} />
            </DataTableCell>
            <DataTableCell>
              <Stack direction="row" gap={1}>
                <CopyAddress address={address} />
                <Tooltip title="PayRef address">
                  <IconButton size="small" onClick={(_e) => setPayRefDialogOpen(true)} color="primary">
                    <IoPaperPlaneOutline />
                  </IconButton>
                </Tooltip>
              </Stack>
            </DataTableCell>
          </TableRow>
        </TableBody>
      </Table>
    </TableContainer>
  );
}

type PayRefDialogProps = {
  address: OotleAddress;
  open: boolean;
  onClose: () => void;
};

function PayRefDialog(props: PayRefDialogProps) {
  const { address, open, onClose } = props;

  const [currentAddress, setCurrentAddress] = useState(address);

  const handleOnChange = (event: React.ChangeEvent<HTMLInputElement>) => {
    const decoded = decodeOotleAddress(address);
    decoded.payRef = event.target.value;
    const addr = encodeOotleAddress(decoded);
    setCurrentAddress(addr);
  };

  return (
    <Dialog
      open={open}
      onClose={onClose}
      aria-labelledby="alert-dialog-title"
      aria-describedby="alert-dialog-description"
    >
      <form onSubmit={() => {}}>
        <DialogTitle id="alert-dialog-title">Pay Ref Address</DialogTitle>
        <DialogContent>
          <DialogContentText id="alert-dialog-description">
            Generate an address with an embedded payment reference
          </DialogContentText>
          <div
            style={{
              marginTop: "1rem",
              display: "flex",
              flexDirection: "column",
              gap: "1rem",
            }}
          >
            <TextField
              name="link"
              placeholder="Inv12345...."
              label="Payment Ref."
              fullWidth
              onChange={handleOnChange}
            />
            <QRCode
              size={256}
              style={{ height: "auto", maxWidth: "100%", width: "100%" }}
              value={currentAddress}
              viewBox={`0 0 256 256`}
            />

            <Typography variant="subtitle1">
              <CopyAddress address={currentAddress} />
            </Typography>
          </div>
        </DialogContent>
        <DialogActions>
          <Button variant="outlined" onClick={onClose}>
            Close
          </Button>
        </DialogActions>
      </form>
    </Dialog>
  );
}

export default AccountDetails;
