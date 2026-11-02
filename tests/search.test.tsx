import { describe, it, expect } from 'vitest';
import { renderToStaticMarkup } from 'react-dom/server';
import CodeSearch from '../src/components/CodeSearch';
import { demo } from '../src/demo';
describe('code search', () => {
  it('exposes indexed definitions with source locations without requiring a file tab', async () => {
    const project = await demo.load();
    const html = renderToStaticMarkup(<CodeSearch project={project} query="choose_route" revision={1} onNavigate={() => {}} />);
    expect(html).toContain('choose_route');
    expect(html).toContain('agent/model_router.py:');
    expect(html).toContain('文字匹配');
  });
});
