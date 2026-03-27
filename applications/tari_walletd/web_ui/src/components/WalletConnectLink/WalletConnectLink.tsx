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

import CheckMark from "@components/WalletConnectLink/CheckMark";
import ConfirmTransaction from "@components/WalletConnectLink/ConfirmTransaction";
import ConnectorLogo from "@components/WalletConnectLink/ConnectorLogo";
import Permissions from "@components/WalletConnectLink/Permissions";
import { Error as ErrorIcon } from "@mui/icons-material";
import CloseIcon from "@mui/icons-material/Close";
import Button from "@mui/material/Button";
import CircularProgress from "@mui/material/CircularProgress";
import Dialog from "@mui/material/Dialog";
import DialogActions from "@mui/material/DialogActions";
import DialogContent from "@mui/material/DialogContent";
import DialogContentText from "@mui/material/DialogContentText";
import IconButton from "@mui/material/IconButton";
import { useTheme } from "@mui/material/styles";
import TextField from "@mui/material/TextField";
import Typography from "@mui/material/Typography";
import useMediaQuery from "@mui/material/useMediaQuery";
import { WalletKit } from "@reown/walletkit";
import useAccountStore from "@store/accountStore";
import {
  accountsCreateFreeTestCoins,
  accountsGet,
  accountsGetBalances,
  accountsGetDefault,
  accountsList,
  confidentialViewVaultBalance,
  keysCreate,
  nftList,
  substatesGet,
  substatesList,
  templatesGet,
  transactionsGetResult,
  transactionsSubmit,
  walletGetInfo,
} from "@utils/json_rpc";
import { Core } from "@walletconnect/core";
import { buildApprovedNamespaces, getSdkError } from "@walletconnect/utils";
import { useEffect, useRef, useState } from "react";
import "./ConnectorLink.css";

const projectId: string = import.meta.env.VITE_WALLET_CONNECT_PROJECT_ID || "78f3485d08b9640a087cbcea000e1f8b";

const ConnectorDialog = () => {
  const [page, setPage] = useState(1);
  const [error, setError] = useState<any | undefined>(undefined);
  const [isOpen, setIsOpen] = useState(false);
  const [linkDetected, setLinkDetected] = useState(false);
  const [link, setLink] = useState("");
  const [proposal, setProposal] = useState<any | undefined>(undefined);
  const [isLoading, setIsLoading] = useState(false);
  const linkRef = useRef<HTMLInputElement>(null);
  const accountStore = useAccountStore();
  const theme = useTheme();
  const isMd = useMediaQuery(theme.breakpoints.up("sm"));
  const [_chosenOptionalPermissions, setChosenOptionalPermissions] = useState<boolean[]>([]);

  const [web3wallet, setWeb3wallet] = useState<any | undefined>();

  async function createWallet(): Promise<any | null> {
    const core = new Core({ projectId });
    const wallet = await WalletKit.init({
      core,
      metadata: {
        name: "Tari Ootle Wallet",
        description: "Tari Ootle Wallet Daemon Web",
        url: "tari.com",
        icons: [],
      },
    });

    wallet.on("session_proposal", async (proposal) => {
      console.log({ proposal });

      const nsRequest = proposal.params.optionalNamespaces.tari || proposal.params.requiredNamespaces.tari;
      if (!nsRequest) {
        await wallet.rejectSession({
          id: proposal.id,
          reason: getSdkError("UNSUPPORTED_NAMESPACE_KEY", "No 'tari' namespace found in proposal"),
        });
        throw new Error("No Tari namespace found in proposal");
      }

      setIsLoading(false);
      setProposal(proposal);
    });

    wallet.on("session_request", async (requestEvent) => {
      console.log({ requestEvent });
      const { params, id, topic } = requestEvent;
      const { request } = params;

      const result = await executeMethod(request.method, request.params);

      const response = { id, result, jsonrpc: "2.0" };
      await wallet.respondSessionRequest({ topic, response });
    });

    return wallet;
  }

  async function executeMethod(method: string, params: any) {
    switch (method) {
      case "tari_getSubstate":
        return substatesGet(params);
      case "tari_accountsList":
        return accountsList(params);
      case "tari_getDefaultAccount":
        return accountsGetDefault(params);
      case "tari_getAccountByAddress":
        return accountsGet(params);
      case "tari_getAccountBalances":
        return accountsGetBalances(params);
      case "tari_submitTransaction":
        return transactionsSubmit(params);
      case "tari_getTransactionResult":
        return transactionsGetResult(params);
      case "tari_getTemplate":
        return templatesGet(params);
      case "tari_createKey":
        return keysCreate(params);
      case "tari_viewConfidentialVaultBalance":
        return confidentialViewVaultBalance(params);
      case "tari_createFreeTestCoins":
        return accountsCreateFreeTestCoins(params);
      case "tari_listSubstates":
        return substatesList(params);
      case "tari_getNftsList":
        return nftList(params);
      case "tari_getWalletInfo":
        return walletGetInfo();
      default:
        setError(`Unsupported method ${method}`);
    }
  }

  async function getClipboardContent() {
    if (navigator.clipboard && navigator.clipboard.readText) {
      try {
        const clipboardData = await navigator.clipboard.readText();
        if (clipboardData.startsWith("wc:")) {
          setLinkDetected(true);
          setLink(clipboardData);
          setIsOpen(true);
        } else {
          setLinkDetected(false);
          setLink("");
        }
      } catch (err) {
        console.error(`Failed to read clipboard contents: ${err}`);
      }
    } else {
      console.warn("Clipboard API not supported in this browser");
    }
  }

  const handleOpen = async () => {
    await getClipboardContent();
    setIsOpen(true);
  };

  const handleClose = () => {
    setError(undefined);
    setIsOpen(false);
    setTimeout(() => {
      setPage(1);
    }, 500);
  };

  const handleConnect = () => {
    linkRef.current && setLink(linkRef.current.value);
    setPage(page + 1);
  };

  const handleConnectWithLink = async () => {
    let wallet = web3wallet;
    if (!wallet) {
      try {
        wallet = await createWallet();
      } catch (error) {
        if (typeof error === "string") {
          setError(error);
          return;
        }

        const err = error as Error;
        setError(err.message || err);
      }
      console.log({ wallet });
      setWeb3wallet(wallet);

      setIsLoading(true);
      try {
        const result = await wallet.pair({ uri: link });
        console.log({ result });
      } catch (error) {
        setIsLoading(false);
        if (typeof error === "string") {
          setError(error);
          return;
        }

        const err = error as Error;
        setError(err.message || err);
        return;
      }
    }
    setPage(page + 1);
  };

  const handleApprove = async () => {
    if (!link) {
      console.error("No WalletConnect link found");
      return;
    }
    if (!web3wallet) {
      console.error("Web3Wallet not initialized");
      return;
    }

    const accounts = accountStore.account ? [`tari:devnet:${accountStore.account.component_address}`] : [];

    try {
      const approvedNamespaces = buildApprovedNamespaces({
        proposal: proposal.params,
        supportedNamespaces: {
          tari: {
            methods: [
              "tari_getSubstate",
              "tari_getDefaultAccount",
              "tari_getAccountBalances",
              "tari_submitTransaction",
              "tari_getTransactionResult",
              "tari_getTemplate",
              "tari_createKey",
              "tari_viewConfidentialVaultBalance",
              "tari_createFreeTestCoins",
              "tari_listSubstates",
              "tari_getNftsList",
            ],
            chains: ["tari:devnet"],
            events: ['chainChanged", "accountsChanged'],
            accounts,
          },
        },
      });

      const session = await web3wallet.approveSession({
        id: proposal.id,
        namespaces: approvedNamespaces,
      });

      // create response object
      const response = { id: proposal.id, result: { approved: true }, jsonrpc: "2.0" };

      // respond to the dapp request with the approved session's topic and response
      await web3wallet.respondSessionRequest({ topic: session.topic, response });
    } catch (error) {
      console.error("USER_REJECTED:", error);
      await web3wallet.rejectSession({
        id: proposal.id,
        reason: getSdkError("USER_REJECTED"),
      });
    }

    setPage(page + 1);
  };

  useEffect(() => {
    if (isOpen) {
      getClipboardContent();
    }
  }, [isOpen, getClipboardContent]);

  let permissions: any[] = [];
  let optionalPermissions: any[] = [];
  if (proposal) {
    try {
      permissions = JSON.parse(proposal.params.sessionProperties["required_permissions"]);
    } catch (e) {
      console.error("Error parsing required permissions:", e);
      console.error("Proposal params:", proposal.params);
    }
    try {
      optionalPermissions = JSON.parse(proposal.params.sessionProperties["optional_permissions"]);
    } catch (e) {
      console.error("Error parsing optional permissions:", e);
      console.error("Proposal params:", proposal.params);
    }
  }

  const renderPage = () => {
    switch (page) {
      case 1:
        if (linkDetected) {
          return (
            <div className="dialog-inner">
              <DialogContentText style={{ paddingBottom: "20px" }}>
                A WalletConnect link was detected. <br />
                Would you like to connect to <code style={{ color: "purple", fontSize: "14px" }}>{link}</code>?
              </DialogContentText>
              <DialogActions>
                <Button variant="outlined" onClick={handleClose}>
                  No
                </Button>
                <Button variant="contained" onClick={handleConnectWithLink}>
                  Yes, Connect
                </Button>
              </DialogActions>
            </div>
          );
        } else {
          return (
            <div className="dialog-inner">
              <DialogContentText style={{ paddingBottom: "20px" }}>
                To connect your wallet, add a wallet connect link here:
              </DialogContentText>
              <TextField name="link" label="Connector Link" inputRef={linkRef} fullWidth />
              <DialogActions>
                <Button variant="outlined" onClick={handleClose}>
                  Cancel
                </Button>
                <Button variant="contained" onClick={handleConnect}>
                  Connect
                </Button>
              </DialogActions>
            </div>
          );
        }
      case 2:
        return (
          <div className="dialog-inner">
            {isLoading ? (
              <CircularProgress />
            ) : (
              <Permissions
                requiredPermissions={permissions}
                optionalPermissions={optionalPermissions}
                setOptionalPermissions={setChosenOptionalPermissions}
              />
            )}
            <DialogActions>
              <Button onClick={handleClose} variant="outlined">
                Cancel
              </Button>
              <Button onClick={handleApprove} variant="contained">
                Authorize
              </Button>
            </DialogActions>
          </div>
        );
      case 3:
        return (
          <div className="dialog-inner">
            <div style={{ textAlign: "center", paddingBottom: "50px" }}>
              <CheckMark />
              <Typography variant="h3">Wallet Connected</Typography>
            </div>
          </div>
        );
      default:
        return <></>;
    }
  };

  // Don't render anything if wallet connect is not set up in the wallet daemon
  if (!projectId) {
    return <></>;
  } else {
    return (
      <>
        <Button variant="contained" color="primary" onClick={handleOpen} size={isMd ? "large" : "small"}>
          Connect with WalletConnect
        </Button>
        <Dialog open={isOpen} onClose={handleClose}>
          <div className="dialog-heading">
            <div style={{ height: "24px", width: "24px" }}></div>
            <ConnectorLogo fill={theme.palette.text.primary} />
            <IconButton onClick={handleClose}>
              <CloseIcon />
            </IconButton>
          </div>
          {error && (
            <div>
              <ErrorIcon />
              {error}
            </div>
          )}
          <DialogContent>{renderPage()}</DialogContent>
        </Dialog>
        <ConfirmTransaction />
      </>
    );
  }
};

export default ConnectorDialog;
