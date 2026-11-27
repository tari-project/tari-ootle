//  Copyright 2024 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

/**
 * Strips the prefix every name in a set shares, so a list of "Validator node-#00" …
 * "Validator node-#06" reads as "#00" … "#06". In a list where every row is the same
 * kind of thing, the shared part is noise and the distinguishing part is the name.
 */
export function distinguish(names: string[]): (name: string) => string {
  if (names.length < 2) {
    return (name) => name;
  }

  let prefix = names[0];
  for (const name of names.slice(1)) {
    let i = 0;
    while (i < prefix.length && i < name.length && prefix[i] === name[i]) {
      i += 1;
    }
    prefix = prefix.slice(0, i);
  }

  // Cut back to a word boundary so a shared prefix never lops off half a token. A "#"
  // stays with the number it marks; separators belong to the prefix and are dropped.
  const boundary = prefix.search(/#?[A-Za-z0-9]+$/);
  if (boundary >= 0) {
    prefix = prefix.slice(0, boundary);
  }

  // A prefix that leaves nothing behind is worse than no shortening at all.
  if (prefix.length < 3 || names.some((name) => name.length - prefix.length < 2)) {
    return (name) => name;
  }
  return (name) => (name.startsWith(prefix) ? name.slice(prefix.length) : name);
}
