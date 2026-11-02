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

import {RuntimeReading,VerificationOverview,type RunView} from '../src/components/RuntimePane';
function runView(legacy=false):RunView {
 const step=fixture();
 return {run:{info:{id:'actual-one',label:'去掉两侧空格',kind:'isolatedTest',producer:'real test',summary:'实际执行隔离函数'},status:'completed',startedAt:'1',finishedAt:'2',revision:1,steps:[step.record],
  verification:legacy?undefined:{goal:'去掉两侧空格',inputs:[{name:'文本',value:'  hello  '}],expected:'hello',actual:'hello',keyValues:[{name:'输出长度',value:'5'}],verdict:'matched'}},steps:[step]};
}
describe('minimal verification summary',()=>{
 it('defaults to five summary fields and collapses process/source',()=>{
  const html=renderToStaticMarkup(createElement(RuntimeReading,{record:runView(),onCode:()=>{}}));
  for(const text of ['验证目标','输入','预期','实际','关键中间值','hello','输出长度','符合预期','隔离验证'])expect(html).toContain(text);
  expect(html).toContain('<details class="runtime-details">');expect(html).not.toContain('<details open');
 });
 it('keeps legacy records without inventing expected values or a pass',()=>{
  const record=runView(true);record.steps[0]=fixture(true);
  const html=renderToStaticMarkup(createElement(VerificationOverview,{record}));
  expect(html).toContain('未记录预期结果');expect(html).not.toContain('符合预期');
  expect(html).toContain('旧记录');expect(html).toContain('源码已有变化');expect(html).toContain('你好');
 });
 it('does not treat completed execution as a successful comparison',()=>{
  const record=runView();record.run.verification!.verdict='different';record.run.verification!.actual='wrong';
  const html=renderToStaticMarkup(createElement(VerificationOverview,{record}));expect(html).toContain('不符合预期');expect(html).toContain('wrong');
 });
});
