import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

beforeEach(() => {
  vi.resetModules();
  const values = new Map<string, string>();
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => values.set(key, value),
    removeItem: (key: string) => values.delete(key),
    clear: () => values.clear(),
  });
});
afterEach(() => vi.unstubAllGlobals());

describe('demonstration reading workflow', () => {
  it('keeps a partial selection partial and persists feature metadata without changing source', async () => {
    const { demo } = await import('../src/demo');
    let project = await demo.load();
    const symbol = project.symbols.find(s => s.name === 'choose_route')!;
    const before = await demo.readFile(symbol.range.file);
    project = await demo.saveFeature({ name: '输入检查', description: '只审阅输入校验片段', color: '#a78bfa' });
    const feature = project.features.find(f => f.name === '输入检查')!;
    project = await demo.bindAnchor({ featureId: feature.id, path: symbol.range.file, startLine: symbol.range.startLine + 2, endLine: symbol.range.startLine + 3, symbolId: symbol.id });
    const bound = project.features.find(f => f.id === feature.id)!;
    expect(bound.anchors[0].kind).toBe('range');
    expect(bound.anchors[0].range.endLine - bound.anchors[0].range.startLine).toBe(1);
    const reopened = await demo.load();
    expect(reopened.features.find(f => f.id === feature.id)!.anchors).toEqual(bound.anchors);
    expect(await demo.readFile(symbol.range.file)).toEqual(before);
  });

  it('rejects confirming a stale view and allows confirmation of the current view', async () => {
    const { demo } = await import('../src/demo');
    const before = await demo.load();
    const feature = before.features.find(f => f.id === 'routing')!;
    const changed = await demo.saveFeature({ id: feature.id, name: feature.name, description: '已修订的功能边界', color: feature.color });
    await expect(demo.setReview(feature.id, 'verified', feature.currentDigest)).rejects.toThrow('刷新');
    const current = changed.features.find(f => f.id === feature.id)!;
    const confirmed = await demo.setReview(feature.id, 'verified', current.currentDigest);
    expect(confirmed.features.find(f => f.id === feature.id)!.reviewState).toBe('verified');
  });

  it('does not let an empty feature imply reviewed code', async () => {
    const { demo } = await import('../src/demo');
    await demo.load();
    const project = await demo.saveFeature({ name: '待绑定', description: '', color: '#34d399' });
    const empty = project.features.find(f => f.name === '待绑定')!;
    await expect(demo.setReview(empty.id, 'verified', empty.currentDigest)).rejects.toThrow('先绑定');
  });

  it('associates changes with the relevant features and reports explicit demo scope', async () => {
    const { demo } = await import('../src/demo');
    await demo.load();
    const changes = await demo.getChanges('HEAD', 'WORKTREE');
    expect(changes.files[0].featureIds.sort()).toEqual(['fallback', 'routing']);
    expect(changes.files[0].affectedSymbols.some(id => id.endsWith('build_context'))).toBe(false);
    await expect(demo.getChanges('main', 'HEAD')).rejects.toThrow('演示');
    expect(await demo.exportReview()).toContain('不代表自动验证');
  });

  it('shows true callers, callees and function-value references without changing reviewed status', async () => {
    const { demo } = await import('../src/demo');
    const project = await demo.load();
    const router = project.symbols.find(s => s.name === 'choose_route')!;
    const relations = await demo.getRelations(router.id, 'HEAD', 'WORKTREE');
    expect(relations.incoming.some(e => e.sourceName === 'stream_chat')).toBe(true);
    expect(relations.outgoing.some(e => e.targetName === 'Route' && e.resolution === 'resolved')).toBe(true);
    expect(relations.references.some(e => e.sourceName === 'get_route_callback' && e.kind === 'reference')).toBe(true);
    expect(relations.incoming.some(e => e.sourceName === 'get_route_callback')).toBe(false);
    expect(relations.impacts.find(f => f.featureId === 'chat-entry')?.kind).toBe('possible');
    expect(relations.impacts.some(f => f.featureId === 'route-schema')).toBe(false);
    expect((await demo.refresh()).features.find(f => f.id === 'chat-entry')?.reviewState).toBe('verified');
    for (const edge of [...relations.incoming, ...relations.outgoing, ...relations.references]) {
      const source = await demo.readFile(edge.site.file);
      expect(relations.fileHashes[edge.site.file]).toBe(source.hash);
      expect(source.content.split('\n')[edge.site.startLine - 1].trim().length).toBeGreaterThan(0);
      if (edge.resolution === 'resolved') expect(edge.targetSymbolId && edge.target).toBeTruthy();
    }
  });

  it('keeps dynamic targets and historical analysis boundaries explicit', async () => {
    const { demo } = await import('../src/demo');
    const project = await demo.load();
    const dynamic = project.symbols.find(s => s.name === 'dispatch_plugin')!;
    const graph = await demo.getRelations(dynamic.id, 'HEAD', 'WORKTREE');
    expect(graph.outgoing[0].resolution).toBe('unresolved');
    expect(graph.outgoing[0].targetSymbolId).toBeNull();
    const history = await demo.getRelations(dynamic.id, 'HEAD~1', 'HEAD');
    expect(history.impacts).toEqual([]);
    expect(history.changedSymbolIds).toEqual([]);
    expect(history.warnings.some(w => w.includes('历史'))).toBe(true);
    const typescript = project.symbols.find(s => s.name === 'appendToken')!;
    expect((await demo.getRelations(typescript.id, 'HEAD', 'WORKTREE')).engine.status).toBe('unavailable');
  });

  it('preserves earlier demo annotations when adding the v0.2 example features', async () => {
    const { demo } = await import('../src/demo');
    const original = await demo.load();
    const saved = original.features.filter(f => !['chat-entry', 'route-schema'].includes(f.id));
    saved[0].description = '原版本留下的阅读说明';
    localStorage.setItem('codetrail-demo-features-v1', JSON.stringify(saved));
    localStorage.removeItem('codetrail-demo-features-v2');
    const migrated = await demo.load();
    expect(migrated.features.find(f => f.id === saved[0].id)?.description).toBe('原版本留下的阅读说明');
    expect(migrated.features.some(f => f.id === 'chat-entry')).toBe(true);
    await demo.deleteFeature('chat-entry');
    expect((await demo.load()).features.some(f => f.id === 'chat-entry')).toBe(false);
  });
});
