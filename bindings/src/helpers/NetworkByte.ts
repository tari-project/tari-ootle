/*
 * //   Copyright 2025 The Tari Project
 * //   SPDX-License-Identifier: BSD-3-Clause
 */

export enum NetworkByte {
  MainNet = 0,
  StageNet = 1,
  NextNet = 2,
  LocalNet = 16,
  Igor = 36,
  Esmeralda = 38,
}

export function getNetworkName(network: NetworkByte): string {
  switch (network) {
    case NetworkByte.MainNet:
      return "mainnet";
    case NetworkByte.StageNet:
      return "stagenet";
    case NetworkByte.NextNet:
      return "nextnet";
    case NetworkByte.LocalNet:
      return "localnet";
    case NetworkByte.Igor:
      return "igor";
    case NetworkByte.Esmeralda:
      return "esmeralda";
    default:
      throw new Error(`Unknown network byte: ${network}`);
  }
}
