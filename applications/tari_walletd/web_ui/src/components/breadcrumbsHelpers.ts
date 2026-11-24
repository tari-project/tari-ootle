//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import React from "react";

export interface BreadcrumbRouteConfig {
  label: string;
  path: string;
  dynamic: boolean;
}

export interface BreadcrumbMatchLike {
  pathname?: string;
  route?: Partial<BreadcrumbRouteConfig>;
}

export interface ResolvedBreadcrumbItem {
  key: string;
  label: string;
  to: string | null;
  isLast: boolean;
}

export function extractBreadcrumbText(breadcrumb: React.ReactNode): string {
  if (typeof breadcrumb === "string" || typeof breadcrumb === "number") {
    return String(breadcrumb);
  }
  if (React.isValidElement<{ children?: React.ReactNode }>(breadcrumb)) {
    return extractBreadcrumbText(breadcrumb.props.children);
  }
  if (Array.isArray(breadcrumb)) {
    return breadcrumb.map(extractBreadcrumbText).join("");
  }
  return "";
}

export function resolveBreadcrumbItem(
  match: BreadcrumbMatchLike,
  breadcrumb: React.ReactNode,
  index: number,
  total: number,
): ResolvedBreadcrumbItem {
  const rawLabel = extractBreadcrumbText(breadcrumb);
  const key = match.pathname ? `${match.pathname}-${index}` : `breadcrumb-${index}`;
  const isLast = index === total - 1;

  if (isLast) {
    return {
      key,
      label: rawLabel.toLowerCase(),
      to: null,
      isLast: true,
    };
  }

  if (match.route) {
    const { label, path, dynamic } = match.route;
    return {
      key,
      label: dynamic ? rawLabel.toLowerCase() : (label ?? rawLabel),
      to: match.pathname || path || "/",
      isLast: false,
    };
  }

  return {
    key,
    label: rawLabel.toLowerCase(),
    to: "/",
    isLast: false,
  };
}
