import {describe,it,expect} from 'vitest';
import {createElement} from 'react';
import {renderToStaticMarkup} from 'react-dom/server';
import {RuntimeSteps,type StepView} from '../src/components/RuntimePane';
function fixture(stale=false):StepView {return {sourceMatches:!stale,sourceProblem:stale?'source changed':null,record:{sequence:1,recordedAt:'1',sourceExcerpt:'return "<script>"',observation:{id:'one',title:'实际输入',status:'ok',result:'返回 <script>alert(1)</script>',reason:'条件成立',elapsedMs:7,inputs:[{name:'text',value:'你好'}],outputs:[],location:{path:'app.py',startLine:1,endLine:2,fileHash:'hash'}}}};}
describe('runtime observations',()=>{
 it('shows actual results and keeps variables and code under disclosure',()=>{
  const html=renderToStaticMarkup(createElement(RuntimeSteps,{steps:[fixture()],onCode:()=>{throw Error('must not execute during render');}}));
  for(const text of ['实际输入','条件成立','你好','展开关键变量与代码位置','查看对应代码','未采集变量'])expect(html).toContain(text);
  expect(html).toContain('<details>');expect(html).not.toContain('<script>');expect(html).toContain('&lt;script&gt;');
 });
 it('disables navigation for old source versions but retains old values and code',()=>{
  const html=renderToStaticMarkup(createElement(RuntimeSteps,{steps:[fixture(true)],onCode:()=>{throw Error('stale must not navigate');}}));
  expect(html).toContain('disabled');expect(html).toContain('当前源码与运行时版本不一致');expect(html).toContain('你好');expect(html).toContain('return');
 });
});
