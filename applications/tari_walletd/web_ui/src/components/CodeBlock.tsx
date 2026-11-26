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

import CloseIcon from "@mui/icons-material/Close";
import { Box, Fade } from "@mui/material";
import Dialog from "@mui/material/Dialog";
import IconButton from "@mui/material/IconButton";
import Tooltip from "@mui/material/Tooltip";
import Typography from "@mui/material/Typography";
import { styled, useTheme } from "@mui/material/styles";
import useMediaQuery from "@mui/material/useMediaQuery";
import { renderJson } from "@utils/helpers";
import { useEffect, useRef, useState } from "react";
import {
  IoCheckmarkOutline,
  IoContractOutline,
  IoCopyOutline,
  IoDownloadOutline,
  IoExpandOutline,
} from "react-icons/io5";

interface ICodeBlockExpand {
  title: string;
  content: unknown;
}

function bytesToHex(bytes: Uint8Array): string {
  return `0x${Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("")}`;
}

function mapKeyToString(key: unknown): string {
  if (typeof key === "string") {
    return key;
  }
  if (key instanceof Uint8Array) {
    return bytesToHex(key);
  }
  if (typeof key === "number" || typeof key === "bigint" || typeof key === "boolean" || key == null) {
    return String(key);
  }
  return stringifyContent(key);
}

export function stringifyContent(content: unknown): string {
  return (
    JSON.stringify(content, (_key, value) => {
      if (typeof value === "bigint") {
        return value.toString();
      }
      if (value instanceof Uint8Array) {
        return bytesToHex(value);
      }
      if (value instanceof Map) {
        return Object.fromEntries(Array.from(value.entries(), ([k, v]) => [mapKeyToString(k), v]));
      }
      return value;
    }) ?? "null"
  );
}

const CodeBlock = styled(Box)(({ theme }) => ({
  backgroundColor: theme.palette.accent.background,
  borderRadius: `${theme.spacing(1)} ${theme.spacing(1)} 0 0`,
  padding: theme.spacing(3),
  maxHeight: "400px",
  overflowY: "scroll",
  border: `1px solid ${theme.palette.accent.border}`,
}));

const StyledToolbar = styled(Box)(({ theme }) => ({
  display: "flex",
  flexDirection: "row",
  gap: theme.spacing(1),
  background: theme.palette.background.paper,
  borderRadius: `0 0 ${theme.spacing(1)} ${theme.spacing(1)}`,
  borderRight: `1px solid ${theme.palette.divider}`,
  padding: 4,
  border: `1px solid ${theme.palette.accent.border}`,
  borderTop: "none",
}));

interface ToolBarProps {
  title: string;
  content: unknown;
  open: boolean;
  onOpen: () => void;
  onClose: () => void;
}

function ToolBar({ title, content, open, onOpen, onClose }: ToolBarProps) {
  const [copied, setCopied] = useState(false);
  const copyTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    return () => {
      if (copyTimerRef.current !== null) {
        clearTimeout(copyTimerRef.current);
      }
    };
  }, []);

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(stringifyContent(content));
      setCopied(true);
      if (copyTimerRef.current !== null) {
        clearTimeout(copyTimerRef.current);
      }
      copyTimerRef.current = setTimeout(() => {
        setCopied(false);
        copyTimerRef.current = null;
      }, 3000);
    } catch (err) {
      console.error("Failed to copy to clipboard:", err);
    }
  };

  const handleDownload = () => {
    const element = document.createElement("a");
    const file = new Blob([stringifyContent(content)], { type: "application/json" });
    const url = URL.createObjectURL(file);
    element.href = url;
    element.download = `${title}.json`;
    document.body.appendChild(element);
    element.click();
    element.remove();
    URL.revokeObjectURL(url);
  };

  const menuItems = [
    {
      id: "copy",
      tooltip: copied ? "Copied!" : "Copy to clipboard",
      icon: copied ? (
        <IoCheckmarkOutline style={{ height: 16, width: 16 }} />
      ) : (
        <IoCopyOutline style={{ height: 16, width: 16 }} />
      ),
      onClick: () => void handleCopy(),
    },
    {
      id: "download",
      tooltip: "Download",
      icon: <IoDownloadOutline style={{ height: 16, width: 16 }} />,
      onClick: handleDownload,
    },
    open
      ? {
          id: "toggle",
          tooltip: "Close",
          icon: <IoContractOutline style={{ height: 16, width: 16 }} />,
          onClick: onClose,
        }
      : {
          id: "toggle",
          tooltip: "Expand",
          icon: <IoExpandOutline style={{ height: 16, width: 16 }} />,
          onClick: onOpen,
        },
  ];

  return (
    <StyledToolbar>
      {menuItems.map((item) => (
        <Tooltip
          TransitionComponent={Fade}
          TransitionProps={{ timeout: 300 }}
          title={item.tooltip}
          placement="top"
          arrow
          key={item.id}
        >
          <IconButton
            onClick={item.onClick}
            style={{
              borderRadius: 2,
            }}
          >
            {item.icon}
          </IconButton>
        </Tooltip>
      ))}
    </StyledToolbar>
  );
}

interface CodeDialogProps {
  title: string;
  content: unknown;
  open: boolean;
  fullScreen: boolean;
  onOpen: () => void;
  onClose: () => void;
}

function CodeDialog({ title, content, open, fullScreen, onOpen, onClose }: CodeDialogProps) {
  const theme = useTheme();
  return (
    <Dialog fullScreen={fullScreen} open={open} onClose={onClose} maxWidth="xl" fullWidth>
      <Box
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          background: theme.palette.background.paper,
          padding: "1rem 1.5rem",
          position: "sticky",
          top: 0,
          borderBottom: `1px solid ${theme.palette.divider}`,
        }}
      >
        <Typography variant="h5" component="div">
          {title}
        </Typography>
        <IconButton edge="end" color="inherit" onClick={onClose} aria-label="close">
          <CloseIcon />
        </IconButton>
      </Box>
      <Box
        sx={{
          padding: "2rem",
          background: theme.palette.background.paper,
        }}
      >
        {renderJson(content)}
      </Box>
      <Box
        style={{
          position: "sticky",
          bottom: 0,
          left: 0,
          width: "100%",
        }}
      >
        <ToolBar title={title} content={content} open={open} onOpen={onOpen} onClose={onClose} />
      </Box>
    </Dialog>
  );
}

export default function CodeBlockExpand({ title, content }: ICodeBlockExpand) {
  const [open, setOpen] = useState(false);

  const theme = useTheme();
  const matches = useMediaQuery(theme.breakpoints.down("md"));

  const handleClickOpen = () => {
    setOpen(true);
  };

  const handleClose = () => {
    setOpen(false);
  };

  return (
    <>
      <Box>
        <CodeBlock>{renderJson(content)}</CodeBlock>
        <ToolBar title={title} content={content} open={open} onOpen={handleClickOpen} onClose={handleClose} />
      </Box>
      <CodeDialog
        title={title}
        content={content}
        open={open}
        fullScreen={matches}
        onOpen={handleClickOpen}
        onClose={handleClose}
      />
    </>
  );
}
