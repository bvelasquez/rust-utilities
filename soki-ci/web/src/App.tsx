import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { BrowserRouter, Route, Routes } from "react-router-dom";

import { AppLayout } from "./components/Layout";
import { AuthGate } from "./components/Login";
import { ToastProvider } from "./components/Toast";
import { EmptyState } from "./components/UI";
import { ConfigPage } from "./features/ConfigPage";
import { DeploymentsPage } from "./features/DeploymentsPage";
import { JobPage } from "./features/JobPage";
import { OverviewPage } from "./features/OverviewPage";
import { ProjectDetailPage, ProjectsPage } from "./features/ProjectsPage";
import { QueuePage } from "./features/QueuePage";
import { SettingsPage } from "./features/SettingsPage";

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 5_000,
      retry: 1,
      refetchOnWindowFocus: false,
    },
  },
});

function NotFoundPage() {
  return (
    <EmptyState
      title="Page not found"
      description="The page you requested does not exist in this build."
    />
  );
}

export default function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <ToastProvider>
        <BrowserRouter>
          <AuthGate>
            <Routes>
              <Route element={<AppLayout />}>
                <Route index element={<OverviewPage />} />
                <Route path="projects" element={<ProjectsPage />} />
                <Route path="projects/:projectId" element={<ProjectDetailPage />} />
                <Route path="deployments" element={<DeploymentsPage />} />
                <Route path="deployments/:jobId" element={<JobPage />} />
                <Route path="queue" element={<QueuePage />} />
                <Route path="config" element={<ConfigPage />} />
                <Route path="settings" element={<SettingsPage />} />
                <Route path="*" element={<NotFoundPage />} />
              </Route>
            </Routes>
          </AuthGate>
        </BrowserRouter>
      </ToastProvider>
    </QueryClientProvider>
  );
}
