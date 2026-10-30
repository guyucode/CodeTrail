import router from '../demo/agent/model_router.py?raw';
import oldRouter from '../demo/baseline/model_router.py?raw';
import tools from '../demo/agent/tool_runner.py?raw';
import chat from '../demo/agent/chat_state.ts?raw';
import tests from '../demo/agent/test_router.py?raw';
import entry from '../demo/agent/chat_entry.py?raw';
import readme from '../demo/agent/README.md?raw';
import type { AnchorBinding, BindRequest, ChangeSummary, CodeRelation, Feature, FeatureDraft, FeatureImpact, FileContent, FileDiff, ProjectSnapshot, ReviewState, Symbol, SymbolRelations } from './types';

const storageKey = 'codetrail-demo-features-v2';
const sources: Record<string, string> = { 'agent/model_router.py': router, 'agent/tool_runner.py': tools, 'agent/chat_entry.py': entry, 'web/chat_state.ts': chat, 'tests/test_router.py': tests, 'README.md': readme };
const languages: Record<string, string> = { py: 'python', ts: 'typescript', md: 'markdown' };
const lang = (path: string) => languages[path.split('.').pop() ?? ''] ?? 'plaintext';
// Demo-only stable content digest. Desktop uses SHA-256 in reader-core.
export function demoHash(text: string): string {
  let value = 2166136261;
  for (const c of text) value = Math.imul(value ^ c.charCodeAt(0), 16777619);
  return `demo:${(value >>> 0).toString(16)}`;
}
const snippet = (path: string, start: number, end: number) => sources[path].split('\n').slice(start - 1, end).join('\n');
function define(path: string, name: string, kind = 'function'): Symbol {
  const lines = sources[path].split('\n');
  const start = lines.findIndex(line => line.startsWith(`def ${name}(`) || line.startsWith(`class ${name}`) || line.startsWith(`export function ${name}(`));
  let end = start + 1;
  if (lang(path) === 'python') {
    while (end < lines.length && (!lines[end].trim() || /^\s/.test(lines[end]))) end++;
    while (end > start + 1 && !lines[end - 1].trim()) end--;
  } else {
    while (end < lines.length && lines[end].trim() !== '}') end++;
    end++;
  }
  return { id: `${path}::${name}`, name, qualifiedName: name, kind, signature: lines[start], range: { file: path, startLine: start + 1, endLine: end } };
}
const symbols: Symbol[] = [
  define('agent/model_router.py', 'Route', 'class'),
  ...['choose_route', 'stream_with_fallback', 'build_context'].map(name => define('agent/model_router.py', name)),
  define('agent/tool_runner.py', 'ToolRequest', 'class'),
  ...['validate_request', 'execute_tool'].map(name => define('agent/tool_runner.py', name)),
  ...['prepare_prompt', 'stream_chat', 'batch_chat', 'get_route_callback', 'dispatch_plugin'].map(name => define('agent/chat_entry.py', name)),
  ...['appendToken', 'finishReply', 'failReply'].map(name => define('web/chat_state.ts', name)),
  ...['test_complex_tasks_use_cloud', 'test_system_message_is_preserved'].map(name => define('tests/test_router.py', name)),
];
function anchor(name: string, changed = false, note = ''): AnchorBinding {
  const symbol = symbols.find(s => s.name === name)!;
  return { id: `demo-anchor-${name}`, kind: 'symbol', symbolId: symbol.id, symbolName: name, range: { ...symbol.range }, contentHash: changed ? 'demo:previous-content' : demoHash(snippet(symbol.range.file, symbol.range.startLine, symbol.range.endLine)), status: changed ? 'changed' : 'exact', note };
}
const seed: Feature[] = [
  { id: 'routing', name: '模型路由', description: '根据任务复杂度选择本地或云端模型。关注输入校验与路由阈值，确认这次改动是否符合预期。', color: '#a78bfa', parentId: null, anchors: [anchor('choose_route', true, '本次增加输入校验，复杂度阈值改为 0.65。'), anchor('test_complex_tasks_use_cloud', false, '关联测试；阅读器不会执行测试。')], reviewState: 'needsReview', reviewedAt: '2026-10-28T09:00:00Z', reviewedDigest: 'demo:old-routing', currentDigest: '' },
  { id: 'fallback', name: '流式输出与降级', description: '主模型失败时切换备用模型。已经输出内容后，应保留错误并停止自动重试，避免两次回答混合。', color: '#38bdf8', parentId: null, anchors: [anchor('stream_with_fallback', true, '检查 emitted 标志与异常路径。'), anchor('appendToken'), anchor('failReply')], reviewState: 'needsReview', reviewedAt: '2026-10-28T09:00:00Z', reviewedDigest: 'demo:old-fallback', currentDigest: '' },
  { id: 'context', name: '上下文管理', description: '保留系统约束，裁剪历史消息。这里按消息条数裁剪，不代表 token 预算。', color: '#34d399', parentId: null, anchors: [anchor('build_context'), anchor('test_system_message_is_preserved', false, '此链接只表示关联，不表示覆盖率或执行结果。')], reviewState: 'verified', reviewedAt: '2026-10-28T09:00:00Z', reviewedDigest: null, currentDigest: '' },
  { id: 'tooling', name: '工具调用', description: '先检查工具是否存在且经过确认，再执行并返回结构化结果。', color: '#fbbf24', parentId: null, anchors: [anchor('validate_request'), anchor('execute_tool')], reviewState: 'reading', reviewedAt: null, reviewedDigest: null, currentDigest: '' },
  { id: 'chat-entry', name: '对话入口', description: '入口自身没有修改，但依赖的路由与降级行为变化后需要检查。相关逻辑会给出调用位置，原有阅读状态仍保留。', color: '#fb923c', parentId: null, anchors: [anchor('stream_chat')], reviewState: 'verified', reviewedAt: '2026-10-28T09:00:00Z', reviewedDigest: null, currentDigest: '' },
  { id: 'route-schema', name: '路由数据结构', description: '路由配置结构未修改。调用者变化不会自动把被调用的结构标为受影响。', color: '#94a3b8', parentId: null, anchors: [anchor('Route')], reviewState: 'verified', reviewedAt: '2026-10-28T09:00:00Z', reviewedDigest: null, currentDigest: '' },
];
const clone = <T,>(value: T): T => JSON.parse(JSON.stringify(value)) as T;
let features = clone(seed);
function persist() {
  if (typeof localStorage === 'undefined') return;
  try { localStorage.setItem(storageKey, JSON.stringify(features)); }
  catch { throw new Error('浏览器无法保存演示标注；本次会话仍可阅读。桌面模式将标注保存在仓库的 .reader/。'); }
}
function currentFeature(id: string) {
  const feature = features.find(item => item.id === id);
  if (!feature) throw new Error('功能不存在，请刷新后重试');
  return feature;
}
function hydrate() {
  if (typeof localStorage === 'undefined') return;
  try {
    const current = localStorage.getItem(storageKey);
    const saved: unknown = JSON.parse(current ?? localStorage.getItem('codetrail-demo-features-v1') ?? 'null');
    if (Array.isArray(saved) && saved.every(f => f && typeof f.id === 'string' && typeof f.name === 'string' && Array.isArray(f.anchors))) features = saved as Feature[];
    if (current === null && Array.isArray(saved)) {
      for (const addition of seed.filter(f => ['chat-entry', 'route-schema'].includes(f.id))) {
        if (!features.some(f => f.id === addition.id)) features.push(clone(addition));
      }
      persist();
    }
  } catch { features = clone(seed); }
}
function digestFeature(feature: Feature): string {
  return demoHash(JSON.stringify([feature.name, feature.description, feature.anchors.map(a => [a.id, a.range.file, a.symbolId, a.note, demoHash(snippet(a.range.file, a.range.startLine, a.range.endLine))])]));
}
function snapshot(): ProjectSnapshot {
  for (const feature of features) feature.currentDigest = digestFeature(feature);
  return clone({ root: 'demo://agent-workbench', name: 'agent-workbench', git: { available: true, branch: 'feature/safer-fallback', head: '8a42c1d (示例)' }, files: Object.entries(sources).map(([path, source]) => ({ path, language: lang(path), size: new TextEncoder().encode(source).length })), symbols, features, warnings: [] });
}

// The browser demo has immutable example source. Its small evidence fixture is
// checked against real source text here; desktop relationships are computed by
// reader-core, not by this fixture. Unknown dynamic targets remain unknown.
function demoRelations(): CodeRelation[] {
  const edges: CodeRelation[] = [];
  function add(sourceName: string, targetName: string, needle: string, kind: 'call' | 'reference' = 'call', resolution: CodeRelation['resolution'] = 'resolved', calleeText = targetName) {
    const source = symbols.find(s => s.name === sourceName)!;
    const target = resolution === 'resolved' ? symbols.find(s => s.name === targetName) : undefined;
    if (!source || (resolution === 'resolved' && !target)) throw new Error(`Invalid demo relation: ${sourceName} -> ${targetName}`);
    const lines = sources[source.range.file].split('\n');
    const offset = lines.slice(source.range.startLine - 1, source.range.endLine).findIndex(line => line.includes(needle));
    if (offset < 0) throw new Error(`Missing demo evidence: ${needle}`);
    const line = source.range.startLine + offset;
    const column = Math.max(1, lines[line - 1].indexOf(calleeText) + 1);
    edges.push({ id: `demo-relation:${source.range.file}:${line}:${column}:${kind}`, kind,
      sourceSymbolId: source.id, sourceName: source.name, targetSymbolId: target?.id ?? null, targetName,
      site: { file: source.range.file, startLine: line, endLine: line }, siteColumn: column,
      target: target ? { ...target.range } : null, resolution, origin: 'syntax',
      detail: resolution === 'resolved' ? (kind === 'reference' ? '示例中的函数值引用；此处没有调用。' : '示例源码中的直接绑定；不是运行时执行证明。') : resolution === 'external' ? '外部或内建目标，不在本仓库中。' : '动态目标由运行时参数决定，未猜测具体实现。',
    });
  }
  add('choose_route', 'Route', 'return Route("local-small"');
  add('choose_route', 'Route', 'return Route("cloud-large"');
  add('choose_route', 'ValueError', 'raise ValueError(', 'call', 'external');
  add('stream_with_fallback', 'providers[model].stream', 'providers[model].stream(', 'call', 'unresolved');
  add('execute_tool', 'validate_request', 'validate_request(request, registry)');
  add('execute_tool', 'registry[request.name]', 'registry[request.name](', 'call', 'unresolved');
  add('prepare_prompt', 'build_context', 'build_context(messages, limit=12)');
  add('stream_chat', 'choose_route', 'select_route(complexity)', 'call', 'resolved', 'select_route');
  add('stream_chat', 'prepare_prompt', 'prepare_prompt(messages)');
  add('stream_chat', 'stream_with_fallback', 'stream_with_fallback(route, providers, prompt)');
  add('batch_chat', 'stream_chat', 'stream_chat(item[');
  add('get_route_callback', 'choose_route', 'return select_route', 'reference', 'resolved', 'select_route');
  add('dispatch_plugin', 'handler', 'handler(payload)', 'call', 'unresolved');
  add('test_complex_tasks_use_cloud', 'choose_route', 'choose_route(0.9)');
  add('test_system_message_is_preserved', 'build_context', 'build_context(messages, limit=3)');
  return edges;
}

export const demo = {
  async load(): Promise<ProjectSnapshot> { hydrate(); return snapshot(); },
  async refresh(): Promise<ProjectSnapshot> { return snapshot(); },
  async readFile(path: string): Promise<FileContent> {
    if (!(path in sources)) throw new Error('示例中没有这个文件');
    return { path, language: lang(path), content: sources[path], hash: demoHash(sources[path]) };
  },
  async saveFeature(draft: FeatureDraft): Promise<ProjectSnapshot> {
    if (!draft.name.trim()) throw new Error('请填写功能名称');
    if (draft.id) {
      const feature = currentFeature(draft.id);
      feature.name = draft.name.trim(); feature.description = draft.description; feature.color = draft.color;
      if (feature.reviewState === 'verified') feature.reviewState = 'needsReview';
    } else features.push({ id: crypto.randomUUID(), name: draft.name.trim(), description: draft.description, color: draft.color, parentId: draft.parentId ?? null, anchors: [], reviewState: 'unread', reviewedAt: null, reviewedDigest: null, currentDigest: '' });
    persist(); return snapshot();
  },
  async deleteFeature(id: string): Promise<ProjectSnapshot> { currentFeature(id); features = features.filter(f => f.id !== id); persist(); return snapshot(); },
  async bindAnchor(request: BindRequest): Promise<ProjectSnapshot> {
    const feature = currentFeature(request.featureId);
    const source = sources[request.path];
    if (source === undefined || request.startLine < 1 || request.endLine < request.startLine || request.endLine > source.split('\n').length) throw new Error('无效的代码范围');
    const symbol = symbols.find(s => s.id === request.symbolId && s.range.file === request.path);
    const wholeSymbol = symbol && symbol.range.startLine === request.startLine && symbol.range.endLine === request.endLine;
    const binding: AnchorBinding = { id: request.anchorId ?? crypto.randomUUID(), kind: wholeSymbol ? 'symbol' : 'range', symbolId: symbol?.id ?? null, symbolName: symbol?.name ?? null, range: { file: request.path, startLine: request.startLine, endLine: request.endLine }, contentHash: '', status: 'exact', note: request.note ?? '' };
    binding.contentHash = demoHash(snippet(binding.range.file, binding.range.startLine, binding.range.endLine));
    if (request.anchorId) {
      const index = feature.anchors.findIndex(a => a.id === request.anchorId);
      if (index < 0) throw new Error('待重绑的片段不存在');
      feature.anchors[index] = binding;
    } else feature.anchors.push(binding);
    feature.reviewState = feature.reviewedAt ? 'needsReview' : 'reading';
    persist(); return snapshot();
  },
  async removeAnchor(featureId: string, anchorId: string): Promise<ProjectSnapshot> {
    const feature = currentFeature(featureId); feature.anchors = feature.anchors.filter(a => a.id !== anchorId); feature.reviewState = feature.reviewedAt ? 'needsReview' : 'reading'; persist(); return snapshot();
  },
  async setReview(id: string, state: ReviewState, expectedDigest?: string): Promise<ProjectSnapshot> {
    const feature = currentFeature(id);
    if (state === 'verified') {
      if (!expectedDigest || expectedDigest !== digestFeature(feature)) throw new Error('代码或标注已变化，请刷新后重新审阅');
      if (!feature.anchors.length) throw new Error('先绑定代码，再确认此功能');
      if (feature.anchors.some(a => a.status === 'missing' || a.status === 'ambiguous')) throw new Error('请先修复失效或有歧义的绑定');
      feature.anchors.forEach(a => { a.contentHash = demoHash(snippet(a.range.file, a.range.startLine, a.range.endLine)); a.status = 'exact'; });
      feature.reviewedAt = new Date().toISOString();
      feature.reviewedDigest = demoHash(feature.anchors.map(a => a.contentHash).join('|'));
    }
    feature.reviewState = state; persist(); return snapshot();
  },
  async getChanges(base: string, head: string): Promise<ChangeSummary> {
    if (base !== 'HEAD' || head !== 'WORKTREE') throw new Error('演示仅提供 HEAD → WORKTREE。桌面模式可以比较真实 Git 引用。');
    const ranges = [symbols.find(s => s.name === 'choose_route')!.range, symbols.find(s => s.name === 'stream_with_fallback')!.range];
    return { base, head, warnings: [], files: [{ path: 'agent/model_router.py', oldPath: null, status: 'modified', additions: 9, deletions: 4, hunks: ranges.map((r, i) => ({ oldStart: i === 0 ? 13 : 20, oldLines: i === 0 ? 5 : 9, newStart: r.startLine, newLines: r.endLine - r.startLine + 1, header: i === 0 ? 'choose_route' : 'stream_with_fallback', lines: [] })), affectedSymbols: symbols.filter(s => ['choose_route', 'stream_with_fallback'].includes(s.name)).map(s => s.id), featureIds: features.filter(f => f.anchors.some(a => ranges.some(r => a.range.file === r.file && a.range.startLine <= r.endLine && a.range.endLine >= r.startLine))).map(f => f.id) }] };
  },
  async getFileDiff(path: string, base: string, head: string): Promise<FileDiff> {
    if (base !== 'HEAD' || head !== 'WORKTREE') throw new Error('演示仅提供 HEAD → WORKTREE');
    if (!(path in sources)) throw new Error('文件不存在');
    return { path, oldPath: null, oldContent: path === 'agent/model_router.py' ? oldRouter : sources[path], newContent: sources[path], language: lang(path) };
  },
  async getRelations(symbolId: string, base: string, head: string, semantic = false): Promise<SymbolRelations> {
    const symbol = symbols.find(s => s.id === symbolId);
    if (!symbol) throw new Error('当前源码中没有这个符号，请刷新后重新选择');
    const hashes = Object.fromEntries(Object.entries(sources).map(([path, text]) => [path, demoHash(text)]));
    const isPython = lang(symbol.range.file) === 'python';
    const currentComparison = base === 'HEAD' && head === 'WORKTREE';
    const edges = isPython ? demoRelations() : [];
    const changes = currentComparison ? await demo.getChanges(base, head) : null;
    const changed = new Set(changes?.files.flatMap(f => f.affectedSymbols) ?? []);
    const direct = new Set(changes?.files.flatMap(f => f.featureIds) ?? []);
    const impacts: FeatureImpact[] = [];
    for (const feature of features) {
      if (direct.has(feature.id)) impacts.push({ featureId: feature.id, kind: 'direct', reason: '直接绑定的代码位于本次修改范围。', evidence: [] });
      else {
        const evidence = edges.filter(edge => edge.kind === 'call' && edge.resolution === 'resolved' && edge.targetSymbolId && changed.has(edge.targetSymbolId) && feature.anchors.some(anchor => !['missing', 'ambiguous'].includes(anchor.status) && anchor.range.file === edge.site.file && anchor.range.startLine <= edge.site.startLine && anchor.range.endLine >= edge.site.endLine));
        if (evidence.length) impacts.push({ featureId: feature.id, kind: 'possible', reason: '本功能中的调用指向已修改符号；这是潜在影响，原阅读状态未被改变。', evidence });
      }
    }
    return clone({ symbol, sourceDigest: demoHash(JSON.stringify(hashes)), fileHashes: hashes,
      incoming: edges.filter(e => e.kind === 'call' && e.targetSymbolId === symbol.id),
      outgoing: edges.filter(e => e.kind === 'call' && e.sourceSymbolId === symbol.id),
      references: edges.filter(e => e.kind === 'reference' && e.targetSymbolId === symbol.id),
      changedSymbolIds: [...changed], impacts: isPython ? impacts : [], base, head,
      engine: { name: '内置示例 · 固定源码证据', status: isPython ? 'partial' : 'unavailable', detail: isPython ? '关系绑定到可查看的示例代码；桌面版使用 Rust 实时分析 Python。' : 'v0.2 关系分析先支持 Python；此文件仍可阅读、绑定与比较。' },
      warnings: [
        '关系图表示源码关联，不代表具体执行顺序、完整依赖或测试已通过。',
        ...(currentComparison ? [] : ['当前关系来自演示工作区，不表示所选历史版本；历史影响未计算。']),
        ...(semantic ? ['浏览器演示不启动语言服务器；请在桌面真实项目中使用可选 Pyright 增强。'] : []),
      ],
    } satisfies SymbolRelations);
  },
  async exportReview(): Promise<string> {
    const labels: Record<ReviewState, string> = { unread: '未阅读', reading: '阅读中', verified: '已确认', needsReview: '需重新审阅' };
    return '# CodeTrail 阅读报告\n\n项目：agent-workbench（内置演示）\n\n这是阅读记录，不代表自动验证了程序行为。\n\n' + features.map(f => `## ${f.name}\n\n状态：${labels[f.reviewState]}\n\n${f.description}\n\n${f.anchors.map(a => `- ${a.range.file}:${a.range.startLine}-${a.range.endLine} · ${a.symbolName ?? '代码片段'} · ${a.status}`).join('\n')}\n`).join('\n');
  },
};

