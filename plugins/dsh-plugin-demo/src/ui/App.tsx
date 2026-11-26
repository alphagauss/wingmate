import React, { useEffect, useRef, useState } from 'react';
import { statisticsPath } from '../types.ts';
import type { Statistics } from '../types.ts';
import { StatisticsIcon } from './controls.tsx';
import type { ButtonComponent } from './controls.tsx';

export function App({
  openDetails,
  Button,
}: {
  openDetails?: () => void;
  Button: ButtonComponent;
}) {
  const [text, setText] = useState('你好，Wingmate!\n让插件独立生长。');
  const [result, setResult] = useState<{
    statistics: Statistics;
    text: string;
  }>();
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
      setResult({ statistics: data, text });
    } catch (cause) {
      if (!controller.signal.aborted)
        setError(cause instanceof Error ? cause.message : '统计失败');
    } finally {
      if (!controller.signal.aborted) setPending(false);
    }
  }

  return (
    <div className="plugin-demo-shell">
      <section className="plugin-demo">
        <header className="plugin-demo-header">
          <div className="plugin-demo-title">
            <span className="plugin-demo-app-icon">
              <StatisticsIcon />
            </span>
            <div>
              <h1>文本统计</h1>
              <p>快速了解一段文本的字数与结构</p>
            </div>
          </div>
          {openDetails && (
            <Button variant="outline" size="sm" onClick={openDetails}>
              插件详情
            </Button>
          )}
        </header>
        <form
          className="plugin-demo-editor"
          onSubmit={submit}
          aria-busy={pending}
        >
          <div className="plugin-demo-editor-heading">
            <label htmlFor="plugin-demo-text">输入文本</label>
            <span>支持多行文本</span>
          </div>
          <textarea
            id="plugin-demo-text"
            value={text}
            maxLength={100_000}
            onChange={(event) => setText(event.target.value)}
            rows={8}
            placeholder="在这里输入或粘贴文本…"
            spellCheck={false}
          />
          <div className="plugin-demo-editor-footer">
            <span>文本仅用于本次计算，不会保存</span>
            <Button type="submit" variant="primary" disabled={pending}>
              {pending ? '统计中…' : '开始统计'}
            </Button>
          </div>
        </form>
        {error && (
          <p className="plugin-demo-error" role="alert">
            {error}
          </p>
        )}
        <section
          className="plugin-demo-results"
          aria-label="统计结果"
          aria-live="polite"
          aria-busy={pending}
        >
          <div className="plugin-demo-results-heading">
            <h2>统计结果</h2>
            <span>
              {pending
                ? '正在计算…'
                : result
                  ? result.text === text
                    ? '已更新'
                    : '文本已修改，请重新统计'
                  : '输入文本后点击开始统计'}
            </span>
          </div>
          <dl>
            {(
              [
                ['字符', result?.statistics.characters, '包含空格与换行'],
                [
                  '非空白字符',
                  result?.statistics.nonWhitespace,
                  '不含空格与换行',
                ],
                ['行数', result?.statistics.lines, '按换行符划分'],
                ['词组', result?.statistics.words, '按空白字符分隔'],
              ] as const
            ).map(([label, value, note]) => (
              <div className="plugin-demo-metric" key={label}>
                <dt>{label}</dt>
                <dd>
                  <span
                    className={
                      value === undefined
                        ? 'plugin-demo-value-empty'
                        : undefined
                    }
                  >
                    {value === undefined ? '—' : value.toLocaleString()}
                  </span>
                  <small>{note}</small>
                </dd>
              </div>
            ))}
          </dl>
        </section>
        <p className="plugin-demo-note">
          字符按 Unicode 码点计数，空文本为零行。
        </p>
      </section>
    </div>
  );
}
