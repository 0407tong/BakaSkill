import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { Toaster } from "sonner";

import App from "@/App";
import { ErrorBoundary } from "@/components/ErrorBoundary";
import { TooltipProvider } from "@/components/ui/Tooltip";
import { useViewStore } from "@/store/viewStore";

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      // 桌面端数据来自本地磁盘，窗口聚焦重取没有意义；
      // 重试策略交给各 query 覆盖（例如 ping 探针不重试）。
      refetchOnWindowFocus: false,
      staleTime: 30_000,
      retry: 1,
    },
  },
});

/** 应用根组件：组合错误边界、数据层 Provider、全局 UI 与主界面。 */
export function Root() {
  const themeMode = useViewStore((s) => s.themeMode);

  return (
    <ErrorBoundary>
      <QueryClientProvider client={queryClient}>
        <TooltipProvider delayDuration={300} skipDelayDuration={150}>
          <App />
          <Toaster
            theme={themeMode}
            position="bottom-right"
            closeButton
            richColors
          />
        </TooltipProvider>
      </QueryClientProvider>
    </ErrorBoundary>
  );
}
