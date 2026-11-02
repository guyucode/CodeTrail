// Actual isolated Node execution + real CLI records; browser transport is a fixture.
// Start Vite first. No user project, human annotations or review state is touched.
import { chromium, expect } from '@playwright/test';
import { mkdtempSync, writeFileSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { resolve, join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
const root=mkdtempSync(join(tmpdir(),'codetrail-summary-test-'));
const cli=resolve('target/debug',process.platform==='win32'?'reader-cli.exe':'reader-cli');
let browser;
try {
 const source='export function finalPrice(price,limit) {\n const over=price>limit;\n return {result:over?limit:price,over,limit};\n}\n';
 writeFileSync(join(root,'logic.mjs'),source);
 const logic=await import(pathToFileURL(join(root,'logic.mjs')).href);
 const call=(action,payload)=>JSON.parse(execFileSync(cli,['trace',action,root],{input:payload?JSON.stringify(payload):undefined,encoding:'utf8'})).value;
 for(const [id,price,expected,legacy] of [['legacy',42,42,true],['current',150,120,false]]) {
  call('start',{id,label:legacy?'旧记录':'价格超过上限时返回上限',kind:'isolatedTest',producer:'actual Node fixture',summary:'真实隔离函数执行，浏览器传输为验收替身。'});
  const fileHash=createHash('sha256').update(readFileSync(join(root,'logic.mjs'))).digest('hex');
  const actual=logic.finalPrice(price,120);
  call('append',{runId:id,step:{id:'calculate',title:'计算最终价格',status:'ok',result:String(actual.result),reason:'实际函数返回',inputs:[{name:'价格',value:String(price)}],outputs:[{name:'结果',value:String(actual.result)}],elapsedMs:0,location:{path:'logic.mjs',startLine:1,endLine:4,fileHash}}});
  const finish={runId:id,status:'completed',summary:'真实隔离函数已返回'};
  if(!legacy) finish.verification={goal:'价格超过上限时返回上限',inputs:[{name:'价格',value:String(price)}],expected:String(expected),actual:String(actual.result),keyValues:[{name:'超过上限',value:actual.over?'是':'否'},{name:'本次上限',value:String(actual.limit)}],verdict:actual.result===expected?'matched':'different'};
  call('finish',finish);
 }
 const runs=call('list');const views=Object.fromEntries(runs.map(r=>[r.info.id,call('read',{runId:r.info.id})]));
 const before=readFileSync(join(root,'.reader/runs.json'));
 browser=await chromium.launch({executablePath:process.env.PLAYWRIGHT_EXECUTABLE_PATH,headless:true});
 const page=await browser.newPage();
 await page.addInitScript(({runs,views})=>{window.__TAURI_INTERNALS__={invoke:async(command,args)=>{
  if(command!=='journal_action')throw Error('Unexpected command');
  if(args.action==='traceList')return runs;
  if(args.action==='traceRead')return views[args.payload.runId];
  throw Error('Unexpected action');
 }};},{runs,views});
 await page.goto(process.env.BASE_URL||'http://127.0.0.1:1420');
 await page.evaluate(async()=>{
  const ReactModule=await import('/node_modules/.vite/deps/react.js');const React=ReactModule.default||ReactModule;
  const Client=await import('/node_modules/.vite/deps/react-dom_client.js');const createRoot=Client.createRoot||Client.default.createRoot;
  const {default:RuntimePane}=await import('/src/components/RuntimePane.tsx');
  const container=document.createElement('div');container.id='runtime-fixture';document.body.append(container);
  createRoot(container).render(React.createElement(RuntimePane,{root:'isolated-fixture',demo:false,onCode:()=>{},onContext:()=>{}}));
 });
 const fixture=page.locator('#runtime-fixture');
 const summary=fixture.getByRole('article',{name:'验证摘要'});
 await expect(summary.getByRole('heading',{name:'价格超过上限时返回上限'})).toBeVisible();
 for(const label of ['输入','预期','实际','关键中间值'])await expect(summary.getByText(label,{exact:true})).toBeVisible();
 const details=fixture.locator('details.runtime-details');expect(await details.evaluate(e=>e.open)).toBe(false);
 await expect(fixture.getByRole('button',{name:'查看对应代码',exact:true})).not.toBeVisible();
 await fixture.getByLabel('选择一次运行').selectOption('legacy');
 await expect(summary.getByText('未记录预期结果',{exact:true})).toBeVisible();
 await expect(summary.getByText('符合预期',{exact:true})).toHaveCount(0);
 await details.locator('summary').first().click();
 await expect(fixture.getByRole('heading',{name:'计算最终价格',exact:true})).toBeVisible();
 await fixture.getByLabel('选择一次运行').selectOption('current');
 await expect(summary.getByRole('heading',{name:'价格超过上限时返回上限'})).toBeVisible();
 expect(await details.evaluate(e=>e.open)).toBe(false);
 expect(readFileSync(join(root,'.reader/runs.json')).equals(before)).toBe(true);
 console.log('PASS actual CLI records, five-field overview, collapsed details, legacy fallback, run switching and readonly data (browser transport fixture)');
} finally {if(browser)await browser.close();rmSync(root,{recursive:true,force:true});}
