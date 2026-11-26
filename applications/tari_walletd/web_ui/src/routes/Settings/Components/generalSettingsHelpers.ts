// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

export function formatSettingsError(error: unknown): string {
  if (error instanceof Error && error.message.trim()) {
    return error.message.trim();
  }
  if (typeof error === "string" && error.trim()) {
    return error.trim();
  }
  return "Failed to load settings.";
}

export function createSettingsLoader<T>(options: {
  fetchSettings: () => Promise<T>;
  onStart?: () => void;
  onSuccess: (settings: T) => void;
  onError: (message: string) => void;
}) {
  let latestRequestId = 0;

  return {
    load(): Promise<void> {
      const requestId = ++latestRequestId;
      options.onStart?.();
      return options
        .fetchSettings()
        .then((res) => {
          if (requestId === latestRequestId) {
            options.onSuccess(res);
          }
        })
        .catch((err) => {
          if (requestId === latestRequestId) {
            options.onError(formatSettingsError(err));
          }
        });
    },
    invalidate(): void {
      latestRequestId++;
    },
  };
}
