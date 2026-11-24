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

import ContentCopyIcon from "@mui/icons-material/ContentCopy";
import IconButton from "@mui/material/IconButton";
import Tooltip from "@mui/material/Tooltip";
import { useEffect, useRef, useState } from "react";

interface CopyProps {
  copy: string;
  title?: string;
  iconWidth?: string;
  iconHeight?: string;
}

const TOOLTIP_RESET_MS = 2000;

const CopyToClipboard = ({ copy, title, iconWidth = "14px", iconHeight = "14px" }: CopyProps) => {
  const [tooltip, setTooltip] = useState<string | null>(null);
  const resetTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const copyIdRef = useRef(0);

  const clearResetTimer = () => {
    if (resetTimerRef.current !== null) {
      clearTimeout(resetTimerRef.current);
      resetTimerRef.current = null;
    }
  };

  useEffect(() => {
    setTooltip(null);
    return () => {
      copyIdRef.current += 1;
      clearResetTimer();
    };
  }, [copy]);

  const handleClick = async (copyThis: string) => {
    const requestId = ++copyIdRef.current;
    clearResetTimer();

    if (!navigator.clipboard?.writeText) {
      setTooltip("Clipboard unavailable");
    } else {
      try {
        await navigator.clipboard.writeText(copyThis);
        if (copyIdRef.current !== requestId) {
          return;
        }
        setTooltip("Copied to clipboard");
      } catch (err) {
        if (copyIdRef.current !== requestId) {
          return;
        }
        setTooltip("Permission denied to copy");
        console.error(err);
      }
    }

    resetTimerRef.current = setTimeout(() => {
      resetTimerRef.current = null;
      setTooltip(null);
    }, TOOLTIP_RESET_MS);
  };

  return (
    <Tooltip title={tooltip ?? (title || copy)} arrow>
      <IconButton onClick={() => handleClick(copy)} size="small" aria-label="copy to clipboard">
        <ContentCopyIcon
          color="primary"
          style={{
            width: iconWidth,
            height: iconHeight,
          }}
        />
      </IconButton>
    </Tooltip>
  );
};

export default CopyToClipboard;
