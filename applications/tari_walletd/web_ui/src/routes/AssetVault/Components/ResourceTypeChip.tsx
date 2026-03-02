//  Copyright 2026. The Tari Project
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

import Chip from "@mui/material/Chip";
import Typography from "@mui/material/Typography";
import { ResourceType } from "@tari-project/ootle-ts-bindings";

interface TypeChipProps {
  type: ResourceType;
  symbol?: string;
  compact?: boolean;
}

const colourOptions: Record<ResourceType, string> = {
  Fungible: "rgba(129,59,245, 0.3)",
  NonFungible: "rgba(58,157,160, 0.3)",
  Confidential: "rgba(81,125,137, 0.3)",
  Stealth: "rgba(100,95,236, 0.3)",
};
export default function TypeChip({ type, symbol, compact = false }: TypeChipProps) {
  const label = (
    <Typography
      variant="label"
      sx={{
        fontSize: compact ? undefined : 12,
      }}
    >
      {symbol && (
        <>
          <strong>{symbol}</strong>
          &#x0020;&#x0020;&#x25CA;&#x0020;&#x0020;
        </>
      )}
      {type}
    </Typography>
  );
  return (
    <Chip
      label={label}
      size={compact ? "small" : "medium"}
      style={{
        background: colourOptions[type],
        height: compact ? 20 : undefined,
        userSelect: "none",
        maxWidth: "min-content",
      }}
    />
  );
}
