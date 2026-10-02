/** 把文本里命中关键词的部分高亮出来（大小写不敏感，按 ASCII 处理）。 */

interface HighlightProps {
  text: string;
  /** 空格分隔的多个关键词；空则原样输出 */
  keywords: string;
}

export function Highlight({ text, keywords }: HighlightProps) {
  const words = keywords.toLowerCase().split(/\s+/).filter(Boolean);

  if (words.length === 0) return <>{text}</>;

  const parts = splitByKeywords(text, words);
  return (
    <>
      {parts.map((part, index) =>
        part.hit ? (
          <mark
            key={index}
            className="rounded-sm bg-accent-subtle px-0.5 text-accent"
          >
            {part.text}
          </mark>
        ) : (
          <span key={index}>{part.text}</span>
        ),
      )}
    </>
  );
}

interface Part {
  text: string;
  hit: boolean;
}

/**
 * 按**所有**关键词切分文本，返回交替的「命中 / 未命中」段。
 *
 * 写成这样一个函数而不是在 JSX 里做替换，是因为要避免用
 * `dangerouslySetInnerHTML`：把用户内容拼成 HTML 再交给浏览器解析，
 * SKILL.md 里的 `<script>` 之类就会真的被执行。这里只产出文本片段，
 * 由 React 负责转义。
 */
function splitByKeywords(text: string, words: readonly string[]): Part[] {
  const parts: Part[] = [];
  let pending = "";
  let index = 0;

  while (index < text.length) {
    const word = words.find(
      (w) => text.slice(index, index + w.length).toLowerCase() === w,
    );

    if (word) {
      if (pending) {
        parts.push({ text: pending, hit: false });
        pending = "";
      }
      parts.push({ text: text.slice(index, index + word.length), hit: true });
      index += word.length;
    } else {
      pending += text[index];
      index += 1;
    }
  }

  if (pending) parts.push({ text: pending, hit: false });
  return parts;
}
