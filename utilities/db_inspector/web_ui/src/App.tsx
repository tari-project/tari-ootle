//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

import "./App.css";
import { Route } from "react-router-dom";
import Layout from "./theme/LayoutMain.tsx";
import Home from "./routes/Home.tsx";
import ErrorPage from "./routes/ErrorPage.tsx";
import ListColumnFamilies from "./routes/ListColumnFamilies.tsx";
import InspectCf from "./routes/InspectCf.tsx";

function App() {
  return (
    <Route path="/" element={<Layout />}>
      <Route index element={<Home />} />
      <Route path="databases/:dbName" element={<ListColumnFamilies />} />
      <Route path="databases/:dbName/column-families/:cfName" element={<InspectCf />} />
      <Route path="*" element={<ErrorPage />} />
    </Route>
  );
}

export default App;
