//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

import type { APIRoute } from "astro";

export const prerender = false;

const REMOTE_ORIGIN = "https://ootle-templates-esme.tari.com";

export const ALL: APIRoute = async ({ params, request, url }) => {
  const path = params.path ? `/${params.path}` : "/";
  const remoteUrl = new URL(`/community-templates${path}`, REMOTE_ORIGIN);
  remoteUrl.search = url.search;

  const response = await fetch(remoteUrl.toString(), {
    method: request.method,
    headers: request.headers,
    body: request.body,
    redirect: "follow",
  });

  const headers = new Headers(response.headers);
  headers.delete("content-encoding");
  headers.delete("content-length");

  return new Response(response.body, {
    status: response.status,
    statusText: response.statusText,
    headers,
  });
};
