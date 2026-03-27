//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

import { AuthNone } from "@components/auth/AuthNone";
import WebAuthn from "@components/auth/web_authn/Webauthn";
import { Lock } from "@mui/icons-material";
import Dialog from "@mui/material/Dialog";
import DialogContent from "@mui/material/DialogContent";
import DialogTitle from "@mui/material/DialogTitle";
import Typography from "@mui/material/Typography";
import { AuthMethod } from "@tari-project/ootle-ts-bindings";

interface LoginDialogProps {
  open: boolean;
  authMethod: AuthMethod;
  onAuthenticated: () => void;
}

export default function AuthDialog(props: LoginDialogProps) {
  const { open, authMethod, onAuthenticated } = props;

  let content;
  switch (authMethod) {
    case "none":
      content = <AuthNone onAuthenticated={onAuthenticated} />;
      break;
    case "webauthn":
      content = <WebAuthn onAuthenticated={onAuthenticated} />;
      break;
    default:
      const message = `Unsupported authentication method: ${authMethod}`;
      console.error(message);
      content = <Typography color="error">{message}</Typography>;
      break;
  }

  return (
    <>
      <Dialog
        open={open}
        fullWidth
        maxWidth="md"
        aria-labelledby="auth-dialog-title"
        aria-describedby="auth-dialog-description"
      >
        <DialogTitle id="alert-dialog-title">
          <Lock />
          Authentication Required
        </DialogTitle>
        <DialogContent>{content}</DialogContent>
      </Dialog>
    </>
  );
}
