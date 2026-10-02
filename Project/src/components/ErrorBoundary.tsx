import { Component, type ErrorInfo, type ReactNode } from "react";
import { AlertTriangle } from "lucide-react";
import { Button } from "@/components/ui/Button";

interface Props {
  children: ReactNode;
}

interface State {
  error: Error | null;
}

/**
 * 顶层错误边界。渲染期异常不应导致白屏——桌面应用没有地址栏可以刷新。
 */
export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    console.error("[ErrorBoundary] 渲染异常:", error, info.componentStack);
  }

  private handleReset = (): void => {
    this.setState({ error: null });
  };

  render(): ReactNode {
    const { error } = this.state;
    if (!error) return this.props.children;

    return (
      // sh-plate：崩溃时的报错信息最需要看得清，不能直接压在插画上
      <div className="sh-plate flex h-full flex-col items-center justify-center gap-4 bg-bg p-8">
        <div className="flex size-12 items-center justify-center rounded-full bg-danger-subtle">
          <AlertTriangle className="size-6 text-danger" />
        </div>
        <div className="max-w-lg space-y-1.5 text-center">
          <h1 className="text-base font-semibold text-fg">界面渲染出错</h1>
          <p className="text-sm text-fg-muted">
            这是界面层的异常，不会影响中央库中的 Skill 文件。
          </p>
        </div>
        <pre className="max-h-48 max-w-2xl overflow-auto rounded-md border border-border bg-bg-subtle p-3 text-left font-mono text-xs text-fg-muted select-text">
          {error.message}
        </pre>
        <Button onClick={this.handleReset}>尝试恢复</Button>
      </div>
    );
  }
}
