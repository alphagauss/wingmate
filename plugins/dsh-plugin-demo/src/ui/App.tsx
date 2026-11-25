import React, { useEffect, useRef, useState } from 'react';
import { statisticsPath } from '../types.ts';
import type { Statistics } from '../types.ts';

export function App({ openDetails }: { openDetails?: () => void }) {
  const [text, setText] = useState('你好，Wingmate!\n让插件独立生长。');
  const [result, setResult] = useState<Statistics>();
  const [error, setError] = useState('');
  const [pending, setPending] = useState(false);
  const request = useRef<AbortController | null>(null);
  useEffect(() => () => request.current?.abort(), []);

  async function submit(event: React.FormEvent) {
    event.preventDefault();
    request.current?.abort();
    const controller = new AbortController();
    request.current = controller;
    setPending(true);
    setError('');
    try {
      const response = await fetch(statisticsPath, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ text }),
        signal: controller.signal,
      });
      const data = await response.json();
      if (!response.ok) throw new Error(data.error || '统计失败');
      setResult(data);
    } catch (cause) {
      if (!controller.signal.aborted)
        setError(cause instanceof Error ? cause.message : '统计失败');
    } finally {
      if (!controller.signal.aborted) setPending(false);
    }
  }

  return (
    <section className="plugin-demo">
      <header>
        <p className="plugin-demo-eyebrow">插件示例</p>
        <h1>文本统计</h1>
        {openDetails && (
          <button type="button" onClick={openDetails}>
            插件详情
          </button>
        )}
        <p>输入一段文本，查看字符、行数和词组数量。</p>
      </header>
      <form onSubmit={submit}>
        <label htmlFor="plugin-demo-text">待统计文本</label>
        <textarea
          id="plugin-demo-text"
          value={text}
          maxLength={100_000}
          onChange={(event) => setText(event.target.value)}
          rows={8}
        />
        <button type="submit" disabled={pending}>
          {pending ? '统计中…' : '开始统计'}
        </button>
      </form>
      {error && <p role="alert">{error}</p>}
      {result && (
        <dl aria-live="polite">
          {(
            [
              ['字符', result.characters],
              ['非空白字符', result.nonWhitespace],
              ['行', result.lines],
              ['词组', result.words],
            ] as const
          ).map(([label, value]) => (
            <div key={label}>
              <dt>{label}</dt>
              <dd>{value}</dd>
            </div>
          ))}
        </dl>
      )}
      <p className="plugin-demo-note">
        字符按 Unicode 码点计数；词组以空白分隔。文本仅用于本次计算，不保存。
      </p>
    </section>
  );
}
