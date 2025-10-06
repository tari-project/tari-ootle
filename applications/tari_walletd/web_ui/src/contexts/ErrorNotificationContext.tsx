//  Copyright 2025. The Tari Project
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

import React, { createContext, useContext, useState, useRef } from "react";
import { Snackbar, Alert, AlertColor } from "@mui/material";

interface ErrorNotification {
  message: string;
  severity?: AlertColor;
  duration?: number;
}

interface ErrorNotificationContextType {
  showError: (message: string, severity?: AlertColor, duration?: number) => void;
  showSuccess: (message: string, duration?: number) => void;
  showWarning: (message: string, duration?: number) => void;
  showInfo: (message: string, duration?: number) => void;
  clearNotification: () => void;
}

const ErrorNotificationContext = createContext<ErrorNotificationContextType | undefined>(undefined);

export const ErrorNotificationProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  const [notification, setNotification] = useState<ErrorNotification | null>(null);
  const timeoutRef = useRef<NodeJS.Timeout | null>(null);

  const showNotification = (message: string, severity: AlertColor = "error", duration: number = 8000) => {
    if (timeoutRef.current) {
      clearTimeout(timeoutRef.current);
    }

    setNotification({ message, severity, duration });

    timeoutRef.current = setTimeout(() => {
      setNotification(null);
    }, duration);
  };

  const clearNotification = () => {
    if (timeoutRef.current) {
      clearTimeout(timeoutRef.current);
    }
    setNotification(null);
  };

  const contextValue: ErrorNotificationContextType = {
    showError: (message, severity = "error", duration = 8000) => showNotification(message, severity, duration),
    showSuccess: (message, duration = 4000) => showNotification(message, "success", duration),
    showWarning: (message, duration = 6000) => showNotification(message, "warning", duration),
    showInfo: (message, duration = 4000) => showNotification(message, "info", duration),
    clearNotification,
  };

  return (
    <ErrorNotificationContext.Provider value={contextValue}>
      {children}
      {notification && (
        <Snackbar
          open={!!notification}
          autoHideDuration={null}
          onClose={(_, reason) => {
            if (reason === "clickaway") {
              return;
            }
            clearNotification();
          }}
          anchorOrigin={{ vertical: "bottom", horizontal: "center" }}
        >
          <Alert
            onClose={clearNotification}
            severity={notification.severity}
            variant="filled"
            sx={{
              width: "100%",
              borderRadius: 6,
            }}
          >
            {notification.message}
          </Alert>
        </Snackbar>
      )}
    </ErrorNotificationContext.Provider>
  );
};

export const useErrorNotification = () => {
  const context = useContext(ErrorNotificationContext);
  if (context === undefined) {
    throw new Error("useErrorNotification must be used within an ErrorNotificationProvider");
  }
  return context;
};
