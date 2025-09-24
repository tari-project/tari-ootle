//  Copyright 2022. The Tari Project
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

import { Navigate, Route, Routes } from "react-router-dom";
import Accounts from "@routes/Accounts/Accounts";
import AccountDetails from "@routes/AccountDetails/AccountDetails";
import Keys from "@routes/Keys/Keys";
import ErrorPage from "@routes/ErrorPage";
import Wallet from "@routes/Wallet/Wallet";
import Layout from "@theme/LayoutMain";
import AccessTokensLayout from "@routes/AccessTokens/AccessTokens";
import Transactions from "@routes/Transactions/TransactionsLayout";
import TransactionDetails from "@routes/Transactions/TransactionDetails";
import AssetVault from "@routes/AssetVault/AssetVault";
import SettingsPage from "@routes/Settings/Settings";
import Auth, { AUTH_TOKEN_FOR_NONE_AUTH } from "@routes/Auth/Auth";
import Webauthn from "@routes/WebauthnRegistration/Webauthn";
import useAuthStore from "./services/store/authStore";
import { useEffect } from "react";
import { useAuthMethod } from "@api/hooks/useAuth";
import AccessToken from "@routes/AccessToken/AccessToken";
import { jwtDecode } from "jwt-decode";
import Templates from "@routes/Templates/Templates";
import Manifest from "@routes/Manifest/Manifest";
import FlowEditor from "@routes/FlowEditor/FlowEditor";
import StealthUtxoListPage from "@/routes/StealthUtxoList/StealthUtxoListPage";
import { useCurrencySync } from "@store/hooks/useCurrencySync";
import { ErrorNotificationProvider } from "./contexts/ErrorNotificationContext";

export const breadcrumbRoutes = [
  {
    label: "Home",
    path: "/",
    dynamic: false,
  },
  {
    label: "Authentication",
    path: "/auth",
    dynamic: false,
  },
  {
    label: "Webauthn",
    path: "/auth/webauthn",
    dynamic: false,
  },
  {
    label: "Accounts",
    path: "/accounts",
    dynamic: false,
  },
  {
    label: "Keys",
    path: "/keys",
    dynamic: false,
  },
  {
    label: "Access Tokens",
    path: "/access-tokens",
    dynamic: false,
  },
  {
    label: "Get access token",
    path: "/access-token",
    dynamic: false,
  },
  {
    label: "Account Details",
    path: "/accounts/:id",
    dynamic: true,
  },
  {
    label: "Transactions",
    path: "/transactions",
    dynamic: false,
  },
  {
    label: "Transaction Details",
    path: "/transactions/:id",
    dynamic: true,
  },
  {
    label: "Wallet",
    path: "/wallet",
    dynamic: false,
  },
  {
    label: "Settings",
    path: "/settings",
    dynamic: false,
  },
  {
    label: "Templates",
    path: "/templates",
    dynamic: false,
  },
  {
    label: "Manifest",
    path: "/manifest",
    dynamic: false,
  },
  {
    label: "Flow Editor",
    path: "/flow-editor",
    dynamic: false,
  },
  {
    label: "Stealth UTXOs",
    path: "/stealth-utxos",
    dynamic: false,
  },
];

const isTokenExpired = (token: any) => {
  if (!token) return true;
  try {
    const decodedToken = jwtDecode(token);
    if (!decodedToken.exp) {
      return true;
    }
    const currentTime = Date.now() / 1000;
    return decodedToken.exp < currentTime;
  } catch (error) {
    console.warn("Failed to decode token:", error);
    return true;
  }
};

interface GuardedRouteProps {
  component: React.ComponentType<any>;
  redirect?: string;
  isAuthenticated: boolean;

  [key: string]: any;
}

// @ts-ignore
const GuardedRoute = ({
  component: Component,
  redirect = "/",
  isAuthenticated = false,
  ...rest
}: GuardedRouteProps) => {
  return isAuthenticated ? <Component {...rest} /> : <Navigate replace to={"/auth?redirect=" + redirect} />;
};

function App() {
  const { data: authMethod, isError: authMethodsIsError, error: authMethodsError } = useAuthMethod();
  const authStore = useAuthStore();
  const { authToken } = authStore;
  let isAuthenticated = !!authToken;

  useCurrencySync();

  useEffect(() => {
    if (isTokenExpired(authToken) && authToken !== AUTH_TOKEN_FOR_NONE_AUTH) {
      authStore.clearToken();
    }
  }, [authToken]);

  useEffect(() => {
    const interval = setInterval(() => {
      if (authToken !== AUTH_TOKEN_FOR_NONE_AUTH && isTokenExpired(authToken)) {
        authStore.clearToken();
      }
    }, 10000);

    return () => clearInterval(interval);
  }, [authToken]);

  useEffect(() => {
    if (!authMethodsIsError && authMethod) {
      if (authMethod.method !== "none" && authToken === AUTH_TOKEN_FOR_NONE_AUTH) {
        authStore.clearToken();
      }

      if (authMethod.method === "none") {
        authStore.setAuthToken(AUTH_TOKEN_FOR_NONE_AUTH);
      }
    }

    if (authMethodsError) {
      console.error(authMethodsError);
    }
  }, [authMethod, authMethodsIsError]);

  return (
    <ErrorNotificationProvider>
      <div>
        <Routes>
          <Route path="/" element={<Layout />}>
            <Route index element={<GuardedRoute component={AssetVault} isAuthenticated={isAuthenticated} />} />
            <Route path="auth" element={<Auth />} />
            <Route path="auth/webauthn" element={<Webauthn />} />
            <Route
              path="access-token"
              element={
                <GuardedRoute isAuthenticated={isAuthenticated} redirect="/access-token" component={AccessToken} />
              }
            />
            <Route
              path="accounts"
              element={<GuardedRoute isAuthenticated={isAuthenticated} redirect="/accounts" component={Accounts} />}
            />
            <Route
              path="accounts/:id"
              element={
                <GuardedRoute isAuthenticated={isAuthenticated} redirect="/accounts" component={AccountDetails} />
              }
            />
            <Route
              path="keys"
              element={<GuardedRoute isAuthenticated={isAuthenticated} redirect="/keys" component={Keys} />}
            />
            <Route
              path="access-tokens"
              element={
                <GuardedRoute
                  redirect="/access-tokens"
                  isAuthenticated={isAuthenticated}
                  component={AccessTokensLayout}
                />
              }
            />
            <Route
              path="transactions"
              element={
                <GuardedRoute isAuthenticated={isAuthenticated} redirect="/transactions" component={Transactions} />
              }
            />
            <Route
              path="wallet"
              element={<GuardedRoute isAuthenticated={isAuthenticated} redirect="/wallet" component={Wallet} />}
            />
            <Route
              path="transactions/:id"
              element={
                <GuardedRoute
                  isAuthenticated={isAuthenticated}
                  redirect="/transactions"
                  component={TransactionDetails}
                />
              }
            />
            <Route
              path="settings"
              element={<GuardedRoute isAuthenticated={isAuthenticated} redirect="/settings" component={SettingsPage} />}
            />
            <Route
              path="templates"
              element={<GuardedRoute isAuthenticated={isAuthenticated} redirect="/templates" component={Templates} />}
            />
            <Route
              path="manifest"
              element={<GuardedRoute isAuthenticated={isAuthenticated} redirect="/manifest" component={Manifest} />}
            />
            <Route
              path="flow-editor"
              element={
                <GuardedRoute isAuthenticated={isAuthenticated} redirect="/flow-editor" component={FlowEditor} />
              }
            />
            <Route
              path="stealth-utxos"
              element={
                <GuardedRoute
                  isAuthenticated={isAuthenticated}
                  redirect="/stealth-utxos"
                  component={StealthUtxoListPage}
                />
              }
            />
            <Route path="*" element={<ErrorPage />} />
          </Route>
        </Routes>
      </div>
    </ErrorNotificationProvider>
  );
}

export default App;
