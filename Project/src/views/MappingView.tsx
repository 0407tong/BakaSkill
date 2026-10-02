import { LinkMappingView } from "@/components/settings/LinkMappingView";

/**
 * 映射矩阵页。
 *
 * **链接映射矩阵的独立入口**：它必须是一个可以直达的页面，
 * 而不是藏在卡片或弹窗里的小徽章——用户需要能"打开一个地方，
 * 一眼看清所有链接的真实状态"。
 */
export function MappingView() {
  return <LinkMappingView />;
}
