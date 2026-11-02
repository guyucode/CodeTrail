import { useEffect, useMemo, useState } from 'react';
import { api } from '../api';
import type { CodeMatch, CodeSearchResult, ProjectSnapshot } from '../types';

export default function CodeSearch({ project, query, revision, changedPaths, onNavigate }: {
  project: ProjectSnapshot; query: string; revision: number; changedPaths?: Set<string>;
  onNavigate: (match: CodeMatch) => void;
}) {
  const [state, setState] = useState<{ query: string; result?: CodeSearchResult; error?: string }>({ query: '' });
  const needle = query.trim();
  const symbols = useMemo(() => project.symbols.filter(s =>
    `${s.name} ${s.qualifiedName}`.toLowerCase().includes(needle.toLowerCase()) && (!changedPaths || changedPaths.has(s.range.file))
  ), [project.symbols, needle, changedPaths]);
  useEffect(() => {
    let cancelled = false;
    setState({ query: needle });
    const timer = setTimeout(() => {
      api.searchCode(needle, project.root).then(result => {
        if (!cancelled) setState({ query: needle, result });
      }).catch(error => { if (!cancelled) setState({ query: needle, error: String(error) }); });
    }, 400);
    return () => { cancelled = true; clearTimeout(timer); };
  }, [needle, project.root, revision]);
  const current = state.query === needle ? state : undefined;
  const matches = current?.result?.matches.filter(m => !changedPaths || changedPaths.has(m.path)) || [];
  return <section className="code-search" aria-label="代码搜索结果">
    <div className="sidebar-section-label">定义匹配 <span>{symbols.length}</span></div>
    {symbols.slice(0, 40).map(s => <button className="file-nav-item" key={s.id} title={`${s.signature}\n${s.range.file}:${s.range.startLine}`}
      onClick={() => onNavigate({ path: s.range.file, line: s.range.startLine, preview: s.signature, fileHash: '' })}>
      <span><strong>{s.qualifiedName}</strong><small>{s.kind} · {s.range.file}:{s.range.startLine}</small></span>
    </button>)}
    {symbols.length > 40 && <p>仅展示前40个定义，请缩小关键词。</p>}
    <div className="sidebar-section-label">代码中的文字匹配 <span>{matches.length}</span></div>
    {!current?.result && !current?.error && <p role="status">正在搜索代码…</p>}
    {current?.error && <p role="alert">搜索失败：{current.error}</p>}
    {matches.map(m => <button className="file-nav-item" key={`${m.path}:${m.line}`} title={`${m.path}:${m.line}\n${m.preview}`} onClick={() => onNavigate(m)}>
      <span><strong>{m.preview || '（空白行）'}</strong><small>{m.path}:{m.line}</small></span>
    </button>)}
    {current?.result && !matches.length && <p>当前搜索范围内没有文字匹配。</p>}
    {current?.result?.warnings.map(w => <p role="status" key={w}>{w}</p>)}
    <p>文字匹配可找到变量及调用位置，也可能命中注释；定义来自上次索引，修改代码后请刷新。</p>
  </section>;
}
