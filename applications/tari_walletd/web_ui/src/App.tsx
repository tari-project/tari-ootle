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
import SettingsPage from "@routes/Settings/Settings";
import Webauthn from "@routes/Webauthn/Webauthn";
import { useEffect, useState } from "react";
import { useAuthMethod } from "@api/hooks/useAuth";
import Templates from "@routes/Templates/Templates";
import Manifest from "@routes/Manifest/Manifest";
// import FlowEditor from "@routes/FlowEditor/FlowEditor";
import StealthUtxoListPage from "@/routes/StealthUtxoList/StealthUtxoListPage";
import { ErrorNotificationProvider } from "./contexts/ErrorNotificationContext";
import Loading from "@components/Loading";
import Onboarding from "@routes/Onboarding/Onboarding";
import MyAssets from "@routes/AssetVault/Components/MyAssets";
import { getClientInstance, isValidJwt } from "@utils/json_rpc";
import { AuthNone } from "@routes/AuthNone";
import useAuthStore from "@store/authStore";

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
  // {
  //   label: "Flow Editor",
  //   path: "/flow-editor",
  //   dynamic: false,
  // },
  {
    label: "Stealth UTXOs",
    path: "/stealth-utxos/:resource_address",
    dynamic: true,
  },
];

interface GuardedRouteProps {
  component: React.ComponentType<any>;
  redirect?: string;

  [key: string]: any;
}

// @ts-ignore
const GuardedRoute = ({ component: Component, redirect = "/", ...rest }: GuardedRouteProps) => {
  const { loggedIn, setLoggedIn } = useAuthStore();
  const { data: authMethod, isError: authMethodsIsError, error: authMethodsError, isLoading } = useAuthMethod();
  const [hasToken, setHasToken] = useState<boolean | null>(null);

  useEffect(() => {
    getClientInstance().then((client) => {
      let token = client.getToken();
      const isAuthenticated = Boolean(token) && isValidJwt(token);
      setHasToken(isAuthenticated);
      setLoggedIn(isAuthenticated);
    });
  }, []);

  if (isLoading || !authMethod || hasToken === null) {
    return <Loading />;
  }

  if (authMethodsIsError) {
    console.error("Error fetching auth method:", authMethodsError);
    return <div>Error fetching authentication method: {authMethodsError?.message || "Unknown error"}</div>;
  }

  if (!hasToken || !loggedIn) {
    console.log(`User not authenticated, redirecting to auth method: ${authMethod.method}`);
    return <Navigate replace to={`/auth/${authMethod.method}?redirect=${redirect}`} />;
  }

  return <Component {...rest} />;
};

function App() {
  return (
    <ErrorNotificationProvider>
      <Routes>
        <Route path="/" element={<Layout />}>
          <Route index element={<GuardedRoute component={MyAssets} />} />
          <Route path="onboarding" element={<GuardedRoute component={Onboarding} />} />
          <Route path="auth/webauthn" element={<Webauthn />} />
          <Route path="auth/none" element={<AuthNone />} />
          <Route path="accounts" element={<GuardedRoute redirect="/accounts" component={Accounts} />} />
          <Route path="accounts/:id" element={<GuardedRoute redirect="/accounts" component={AccountDetails} />} />
          <Route path="keys" element={<GuardedRoute redirect="/keys" component={Keys} />} />
          <Route
            path="access-tokens"
            element={<GuardedRoute redirect="/access-tokens" component={AccessTokensLayout} />}
          />
          <Route path="transactions" element={<GuardedRoute redirect="/transactions" component={Transactions} />} />
          <Route path="wallet" element={<GuardedRoute redirect="/wallet" component={Wallet} />} />
          <Route
            path="transactions/:id"
            element={<GuardedRoute redirect="/transactions" component={TransactionDetails} />}
          />
          <Route path="settings" element={<GuardedRoute redirect="/settings" component={SettingsPage} />} />
          <Route path="templates" element={<GuardedRoute redirect="/templates" component={Templates} />} />
          <Route path="manifest" element={<GuardedRoute redirect="/manifest" component={Manifest} />} />
          {/*<Route path="flow-editor" element={<GuardedRoute redirect="/flow-editor" component={FlowEditor} />} />*/}
          <Route
            path="stealth-utxos/:resource_address"
            element={<GuardedRoute redirect="/stealth-utxos" component={StealthUtxoListPage} />}
          />
          <Route path="*" element={<ErrorPage />} />
        </Route>
      </Routes>
    </ErrorNotificationProvider>
  );
}

export default App;
