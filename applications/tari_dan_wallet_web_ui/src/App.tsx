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

import {Navigate, Route, Routes} from "react-router-dom";
import Accounts from "./routes/Accounts/Accounts";
import AccountDetails from "./routes/AccountDetails/AccountDetails";
import Keys from "./routes/Keys/Keys";
import ErrorPage from "./routes/ErrorPage";
import Wallet from "./routes/Wallet/Wallet";
import Layout from "./theme/LayoutMain";
import AccessTokensLayout from "./routes/AccessTokens/AccessTokens";
import Transactions from "./routes/Transactions/TransactionsLayout";
import TransactionDetails from "./routes/Transactions/TransactionDetails";
import AssetVault from "./routes/AssetVault/AssetVault";
import SettingsPage from "./routes/Settings/Settings";
import Auth from "./routes/Auth/Auth";
import Webauthn from "./routes/WebauthnRegistration/Webauthn";
import {useState} from "react";
import useAuthStore from "./store/authStore";

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
];

const GuardedRoute = ({ component: Component, redirect = "/auth", auth, ...rest }) => (
        auth === true
            ? <Component {...rest} />
            : <Navigate replace to={redirect} />
)

function App() {
  const [auth, setAuth] = useState(false);
  const {authToken} = useAuthStore();
  if (authToken) {
    setAuth(true);
  }

  return (
    <div>
      <Routes>
        <Route path="/" element={<Layout />}>
          <Route index element={<GuardedRoute component={AssetVault} auth={auth} />} />
          <Route path="auth" element={<Auth />} />
          <Route path="auth/webauthn" element={<Webauthn />} />
          <Route path="accounts" element={<GuardedRoute auth={auth} component={Accounts} />} />
          <Route path="accounts/:id" element={<GuardedRoute auth={auth} component={AccountDetails} />} />
          <Route path="keys" element={<GuardedRoute auth={auth} component={Keys} />} />
          <Route path="access-tokens" element={<GuardedRoute auth={auth} component={AccessTokensLayout}/>} />
          <Route path="transactions" element={<GuardedRoute auth={auth} component={Transactions} />} />
          <Route path="wallet" element={<GuardedRoute auth={auth} component={Wallet} />} />
          <Route path="transactions/:id" element={<GuardedRoute auth={auth} component={TransactionDetails} />} />
          <Route path="settings" element={<GuardedRoute auth={auth} component={SettingsPage} />} />
          <Route path="*" element={<ErrorPage />} />
        </Route>
      </Routes>
    </div>
  );
}

export default App;
