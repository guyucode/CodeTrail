import { describe, expect, it } from 'vitest';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import ProjectOverview, { overviewTarget } from '../src/components/ProjectOverview';
import type { ProjectSnapshot, ReadingStep } from '../src/types';

const step: ReadingStep = { id:'start', title:'Read entry', description:'Follow the real implementation', moduleId:'ui', featureId:'feature', evidenceId:'entry' };
function fixture(): ProjectSnapshot {
  return { root:'/fixture', name:'Example', git:{available:false,branch:null,head:null}, files:[],symbols:[],warnings:[],
    overview:{ summary:'A project <script>description</script>', modules:[{id:'ui',name:'Interface',responsibility:'Start reading'}], paths:[{id:'read',name:'Read project',description:'A guided path',steps:[step]}]},
    features:[{id:'feature',name:'Entry',description:'Read it',color:'#67c9f3',parentId:null,reviewState:'unread',reviewedAt:null,reviewedDigest:null,currentDigest:'current',anchors:[{id:'entry',kind:'range',symbolId:null,symbolName:null,range:{file:'entry.ts',startLine:1,endLine:3},contentHash:'hash',status:'exact',note:''}]}] };
}
describe('project overview navigation', () => {
  it('resolves the exact named feature and evidence without fallback', () => {
    const p=fixture();
    expect(overviewTarget(p,step)?.anchor.range.file).toBe('entry.ts');
    expect(overviewTarget(p,{...step,evidenceId:'other'})).toBeNull();
    expect(overviewTarget(p,{...step,featureId:'other'})).toBeNull();
    for (const status of ['missing','ambiguous'] as const) {
      p.features[0].anchors[0].status=status;
      expect(overviewTarget(p,step)).toBeNull();
    }
  });
  it('renders explanatory paths and safely escapes content without changing review data', () => {
    const p=fixture();const before=JSON.stringify(p);
    const html=renderToStaticMarkup(createElement(ProjectOverview,{project:p,onRead:()=>{throw Error('render must not navigate');}}));
    expect(html).toContain('Read entry');expect(html).toContain('Interface');
    expect(html).toContain('&lt;script&gt;');expect(html).not.toContain('<script>');
    expect(JSON.stringify(p)).toBe(before);
  });
  it('makes broken links explicit and labels changed evidence', () => {
    const p=fixture();p.features[0].anchors[0].status='missing';
    let html=renderToStaticMarkup(createElement(ProjectOverview,{project:p,onRead:()=>{}}));
    expect(html).toContain('disabled');expect(html).toContain('关联需要更新');
    p.features[0].anchors[0].status='changed';
    html=renderToStaticMarkup(createElement(ProjectOverview,{project:p,onRead:()=>{}}));
    expect(html).toContain('证据已变化');
  });
});
