import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";

import { api, clearAuth, setCsrfToken } from "../api/client";
import type { SessionResponse } from "../api/types";

export function useSession() {
  const query = useQuery<SessionResponse>({
    queryKey: ["session"],
    queryFn: api.session,
    retry: false,
    staleTime: 30_000,
  });

  useEffect(() => {
    if (query.data?.csrf_token) setCsrfToken(query.data.csrf_token);
  }, [query.data?.csrf_token]);

  useEffect(() => {
    const onAuthRequired = () => {
      clearAuth();
      void query.refetch();
    };
    window.addEventListener("soki:auth-required", onAuthRequired);
    return () => window.removeEventListener("soki:auth-required", onAuthRequired);
  }, [query]);

  return query;
}

export function useLogin() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (token: string) => api.login(token),
    onSuccess: async () => {
      await queryClient.invalidateQueries();
    },
  });
}

export function useLogout() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: api.logout,
    onSuccess: async () => {
      await queryClient.invalidateQueries();
    },
  });
}
