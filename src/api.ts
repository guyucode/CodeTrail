import { invoke, isTauri } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { demo } from './demo';
import type { BindRequest, ChangeSummary, FeatureDraft, FileContent, FileDiff, ProjectSnapshot, ReviewState, SymbolRelations } from './types';

let demoMode = true;
const desktop = isTauri();
export const api = {
  isDesktop: desktop,
  async loadDemo() { demoMode = true; return demo.load(); },
  async chooseProject(): Promise<ProjectSnapshot | null> {
    if (!desktop) throw new Error('当前是浏览器演示。请运行 npm run desktop，在桌面应用中打开本地仓库。');
    const selected = await open({ directory: true, multiple: false, title: '选择要阅读的代码仓库' });
    if (!selected || typeof selected !== 'string') return null;
    return api.openProject(selected);
  },
  async openProject(path: string): Promise<ProjectSnapshot> {
    if (!desktop) throw new Error('真实仓库读取需要 CodeTrail 桌面应用');
    const result = await invoke<ProjectSnapshot>('open_project', { path });
    demoMode = false;
    return result;
  },
  refresh: () => demoMode ? demo.refresh() : invoke<ProjectSnapshot>('refresh_project'),
  readFile: (path: string) => demoMode ? demo.readFile(path) : invoke<FileContent>('read_file', { path }),
  saveFeature: (draft: FeatureDraft) => demoMode ? demo.saveFeature(draft) : invoke<ProjectSnapshot>('save_feature', { draft }),
  deleteFeature: (featureId: string) => demoMode ? demo.deleteFeature(featureId) : invoke<ProjectSnapshot>('delete_feature', { featureId }),
  bindAnchor: (request: BindRequest) => demoMode ? demo.bindAnchor(request) : invoke<ProjectSnapshot>('bind_anchor', { request }),
  removeAnchor: (featureId: string, anchorId: string) => demoMode ? demo.removeAnchor(featureId, anchorId) : invoke<ProjectSnapshot>('remove_anchor', { featureId, anchorId }),
  setReview: (featureId: string, state: ReviewState, expectedDigest?: string) => demoMode ? demo.setReview(featureId, state, expectedDigest) : invoke<ProjectSnapshot>('set_review', { featureId, state, expectedDigest: expectedDigest ?? null }),
  getChanges: (base: string, head: string) => demoMode ? demo.getChanges(base, head) : invoke<ChangeSummary>('get_changes', { base, head }),
  getFileDiff: (path: string, base: string, head: string) => demoMode ? demo.getFileDiff(path, base, head) : invoke<FileDiff>('get_file_diff', { path, base, head }),
  getRelations: (symbolId: string, base: string, head: string, semantic = false) => demoMode ? demo.getRelations(symbolId, base, head, semantic) : invoke<SymbolRelations>('get_relations', { symbolId, base, head, semantic }),
  exportReview: () => demoMode ? demo.exportReview() : invoke<string>('export_review'),
};
