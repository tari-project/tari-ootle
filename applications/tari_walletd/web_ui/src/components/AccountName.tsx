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
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

import React, { useState } from "react";
import { Box, TextField, IconButton } from "@mui/material";
import { IoCheckmark, IoClose } from "react-icons/io5";
import { ComponentAddress } from "@tari-project/ootle-ts-bindings";
import { useAccountsRename } from "../services/api/hooks/useAccounts";
import { LuPencilLine } from "react-icons/lu";
import { useTheme } from "@mui/material/styles";

export interface AccountNameProps {
  accountAddress: ComponentAddress;
  currentName?: string | null;
  showRenameButton?: boolean;
  onRenameSuccess?: (newName: string) => void;
  onRenameError?: (error: any) => void;
}

const AccountName: React.FC<AccountNameProps> = ({
  accountAddress,
  currentName,
  showRenameButton = true,
  onRenameSuccess,
  onRenameError,
}) => {
  const [isEditingName, setIsEditingName] = useState(false);
  const [newName, setNewName] = useState("");

  const renameAccountMutation = useAccountsRename();

  const theme = useTheme();

  const handleStartEdit = () => {
    setIsEditingName(true);
    setNewName(currentName || "");
  };

  const handleCancelEdit = () => {
    setIsEditingName(false);
    setNewName("");
  };

  const handleSaveRename = () => {
    if (newName.trim() && newName !== currentName) {
      renameAccountMutation.mutate(
        {
          account: accountAddress,
          newName: newName.trim(),
        },
        {
          onSuccess: () => {
            const trimmedName = newName.trim();
            setIsEditingName(false);
            setNewName("");
            onRenameSuccess?.(trimmedName);
          },
          onError: (error: any) => {
            console.error("Error renaming account:", error);
            onRenameError?.(error);
          },
        },
      );
    } else {
      handleCancelEdit();
    }
  };

  const handleKeyPress = (event: React.KeyboardEvent) => {
    if (event.key === "Enter") {
      handleSaveRename();
    } else if (event.key === "Escape") {
      handleCancelEdit();
    }
  };

  return (
    <Box display="flex" alignItems="center" gap={1}>
      {isEditingName ? (
        <TextField
          value={newName}
          onChange={(e) => setNewName(e.target.value)}
          onKeyDown={handleKeyPress}
          size="small"
          autoFocus
          disabled={renameAccountMutation.isPending}
          placeholder="Account name"
        />
      ) : (
        <span>{currentName || "<No Name>"}</span>
      )}
      {showRenameButton && (
        <>
          {isEditingName ? (
            <Box display="flex" gap={0.5}>
              <IconButton
                size="small"
                onClick={handleSaveRename}
                disabled={renameAccountMutation.isPending}
                title="Save"
              >
                <IoCheckmark />
              </IconButton>
              <IconButton
                size="small"
                onClick={handleCancelEdit}
                disabled={renameAccountMutation.isPending}
                title="Cancel"
              >
                <IoClose />
              </IconButton>
            </Box>
          ) : (
            <IconButton size="small" onClick={handleStartEdit} title="Rename account">
              <LuPencilLine
                style={{
                  color: theme.palette.primary.main,
                }}
              />
            </IconButton>
          )}
        </>
      )}
    </Box>
  );
};

export default AccountName;
