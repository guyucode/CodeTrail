import { describe,it,expect } from 'vitest';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import {ActivityCards,projectActivities,type Activity} from '../src/components/WorkPanel';
const root='/fixture/project';
const event=(patch:Partial<Activity>={}):Activity=>({projectRoot:root,kind:'projectChange',files:['src/chat.py'],id:'one',task:'<script>bad</script>',stage:'working',places:['聊天入口'],impact:'影响提交',expectedEffect:'减少等待',verification:null,recordedAt:'1',...patch});
describe('current-project Agent changes',()=>{
  it('keeps expected outcomes distinct from reported verification and escapes content',()=>{
    const html=renderToStaticMarkup(createElement(ActivityCards,{root,events:[event()]}));
    for(const text of ['改了哪里','影响什么','预期效果','尚未报告验证结果','修改中','聊天入口','减少等待','src/chat.py'])expect(html).toContain(text);
    expect(html).not.toContain('<script>');expect(html).not.toContain('已验证通过');
  });
  it('rejects copied foreign, legacy, runtime and metadata-only events',()=>{
    const own=event({task:'当前项目修改'});
    const mixed=[own,event({projectRoot:root+'-other',task:'其他项目修改'}),event({projectRoot:null,kind:null,files:[],task:'旧版开发动态'}),event({kind:'runtimeExecution',task:'程序运行结果'}),event({files:['.reader/agent.json'],task:'阅读器关联更新'})];
    expect(projectActivities(root,mixed)).toEqual([own]);
    const html=renderToStaticMarkup(createElement(ActivityCards,{root,events:mixed}));
    expect(html).toContain('当前项目修改');for(const text of ['其他项目修改','旧版开发动态','程序运行结果','阅读器关联更新'])expect(html).not.toContain(text);
    expect(renderToStaticMarkup(createElement(ActivityCards,{root:root+'-other',events:[own]}))).toBe('');
  });
  it('does not treat outside paths or globs as current project files',()=>{
    for(const path of ['/outside.py','../outside.py','src/../outside.py','src\\outside.py','.git/config','src/.reader/agent.json','src/*.py'])expect(projectActivities(root,[event({files:[path]})])).toEqual([]);
  });
});
