// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import Alert from "@mui/material/Alert";
import Typography from "@mui/material/Typography";
import { ValueScanCoverage } from "@tari-project/ootle-ts-bindings";

interface SearchedRangeProps {
  searched: ValueScanCoverage;
}

/**
 * States which values the daemon actually searched. A balance reported as undecryptable is only
 * meaningful next to this: outside the searched range, nobody looked.
 */
function SearchedRange({ searched }: SearchedRangeProps) {
  const range = `${searched.min.toString()} - ${searched.max.toString()}`;

  if (searched.clamped) {
    return (
      <Alert severity="warning" sx={{ marginBottom: 2 }}>
        Only values {range} were searched, which is less than you asked for. A balance shown as undecryptable may simply
        be above {searched.max.toString()}. Configure a value lookup table file to search further.
      </Alert>
    );
  }

  return (
    <Typography variant="body2" sx={{ marginBottom: 2 }}>
      Searched values {range}. A balance outside this range cannot be found.
    </Typography>
  );
}

export default SearchedRange;
