// Copyright 2024 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { TariTypeTag } from "@tari-project/ootle-ts-bindings";

export function getValueByPath(cborRepr: object, path: string): any {
  let value = cborRepr;
  for (const part of path.split(".")) {
    if (part == "$") {
      continue;
    }
    if ("Map" in value) {
      // @ts-ignore
      value = value.Map.find((v) => convertCborValue(v[0]) === part)?.[1];
      if (!value) {
        return null;
      }
      continue;
    }

    if ("Array" in value) {
      // @ts-ignore
      value = value.Array[parseInt(part)];
      continue;
    }

    return null;
  }
  return convertCborValue(value);
}

export function convertCborValue(value: any): any {
  // TODO: The value === "Null" case should be fixed
  if (value === null || value === "Null") {
    return null;
  }

  if ("Map" in value) {
    const result = {};
    for (const [key, val] of value.Map) {
      // @ts-ignore
      result[convertCborValue(key)] = convertCborValue(val);
    }
    return result;
  }
  if ("Tag" in value) {
    return convertTaggedValue(value.Tag[0], value.Tag[1]);
  }
  if ("Text" in value) {
    return value.Text;
  }
  if ("Bytes" in value) {
    return value.Bytes;
  }

  if ("Array" in value) {
    return value.Array.map(convertCborValue);
  }
  if ("Integer" in value) {
    return value.Integer;
  }
  if ("Bool" in value) {
    return value.Bool;
  }
  return value;
}

function bytesToAddressString(type: String, tag: ArrayLike<number>): string {
  const hex = Array.from(tag, function(byte) {
    return ("0" + (byte & 0xff).toString(16)).slice(-2);
  }).join("");

  return `${type}_${hex}`;
}

export function convertTaggedValue(tag: number, value: any): string | any {
  switch (tag) {
    case TariTypeTag.VaultId:
      return bytesToAddressString("vault", value.Bytes!);
    case TariTypeTag.ComponentAddress:
      return bytesToAddressString("component", value.Bytes!);
    case TariTypeTag.ResourceAddress:
      return bytesToAddressString("resource", value.Bytes!);
    case TariTypeTag.NonFungibleAddress:
      return bytesToAddressString("non_fungible", value.Bytes!);
    case TariTypeTag.ClaimedOutputTombstoneAddress:
      return bytesToAddressString("tombstone", value.Bytes!);
    case TariTypeTag.TemplateAddress:
      return bytesToAddressString("template", value.Bytes!);
    case TariTypeTag.Utxo:
      return bytesToAddressString("utxo", value.Bytes!);
    case TariTypeTag.Metadata:
      return convertCborValue(value);
    default:
      return value;
  }
}
