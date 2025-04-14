 

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./theme/theme.css";
import App from "./App.tsx";

import { createBrowserRouter, createRoutesFromElements, RouterProvider } from "react-router-dom";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

const queryClient = new QueryClient();

const router = createBrowserRouter(
  createRoutesFromElements(App()),
);

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  </StrictMode>,
);
