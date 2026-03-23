/*
 * //  Copyright 2026 The Tari Project
 * //  SPDX-License-Identifier: BSD-3-Clause
 */

/**
 * A parsed Server-Sent Event.
 */
export interface SseEvent {
  event: string;
  data: string;
  id?: string;
  retry?: number;
}

/**
 * Callback-based SSE stream handle. Call `close()` to stop the stream.
 */
export interface SseStream {
  close(): void;
}

export interface SseStreamOptions {
  /** Called for each received SSE event */
  onEvent: (event: SseEvent) => void;
  /** Called when the stream ends or encounters an error */
  onError?: (error: Error) => void;
  /** Called when the stream closes cleanly */
  onClose?: () => void;
  /** AbortSignal to cancel the stream externally */
  signal?: AbortSignal;
}

/**
 * Connect to an SSE endpoint using fetch and parse the streaming response.
 * Returns an SseStream handle that can be closed.
 */
export function connectSse(url: string, options: SseStreamOptions): SseStream {
  const controller = new AbortController();

  // Link external signal if provided
  if (options.signal) {
    if (options.signal.aborted) {
      controller.abort();
    } else {
      options.signal.addEventListener("abort", () => controller.abort(), { once: true });
    }
  }

  const run = async () => {
    const response = await fetch(url, {
      method: "GET",
      headers: { Accept: "text/event-stream" },
      signal: controller.signal,
    });

    if (!response.ok) {
      const text = await response.text();
      throw new Error(`HTTP ${response.status}: ${response.statusText}${text ? ` - ${text}` : ""}`);
    }

    if (!response.body) {
      throw new Error("Response body is not readable");
    }

    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    let buffer = "";

    // Current event being built
    let eventType = "";
    let dataLines: string[] = [];
    let eventId: string | undefined;
    let retry: number | undefined;

    const dispatch = () => {
      if (dataLines.length === 0) {
        // Reset but don't dispatch
        eventType = "";
        eventId = undefined;
        retry = undefined;
        return;
      }

      const event: SseEvent = {
        event: eventType || "message",
        data: dataLines.join("\n"),
        id: eventId,
        retry,
      };

      options.onEvent(event);

      // Reset for next event
      eventType = "";
      dataLines = [];
      eventId = undefined;
      retry = undefined;
    };

    const processLine = (line: string) => {
      if (line === "") {
        // Empty line = end of event
        dispatch();
        return;
      }

      if (line.startsWith(":")) {
        // Comment, ignore
        return;
      }

      const colonIndex = line.indexOf(":");
      let field: string;
      let value: string;

      if (colonIndex === -1) {
        field = line;
        value = "";
      } else {
        field = line.slice(0, colonIndex);
        value = line.slice(colonIndex + 1);
        if (value.startsWith(" ")) {
          value = value.slice(1);
        }
      }

      switch (field) {
        case "event":
          eventType = value;
          break;
        case "data":
          dataLines.push(value);
          break;
        case "id":
          eventId = value;
          break;
        case "retry": {
          const parsed = parseInt(value, 10);
          if (!isNaN(parsed)) {
            retry = parsed;
          }
          break;
        }
        // Unknown fields are ignored per spec
      }
    };

    try {
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;

        buffer += decoder.decode(value, { stream: true });

        // Process complete lines, splitting on \r\n, \r, or \n per the SSE spec
        let match: RegExpExecArray | null;
        while ((match = /\r\n|\r|\n/.exec(buffer)) !== null) {
          const line = buffer.slice(0, match.index);
          buffer = buffer.slice(match.index + match[0].length);
          processLine(line);
        }
      }

      // Process any remaining data
      if (buffer.length > 0) {
        processLine(buffer);
        dispatch();
      }
    } finally {
      reader.releaseLock();
    }
  };

  run()
    .then(() => options.onClose?.())
    .catch((err) => {
      if (controller.signal.aborted) {
        options.onClose?.();
      } else {
        options.onError?.(err instanceof Error ? err : new Error(String(err)));
      }
    });

  return {
    close() {
      controller.abort();
    },
  };
}
