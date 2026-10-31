// Browser interaction test with an explicit Tauri transport fixture.
// Real HTTP, storage, membership and version checks are covered by Rust tests.
import {chromium,expect} from '@playwright/test';
const browser=await chromium.launch({headless:true,executablePath:process.env.PLAYWRIGHT_EXECUTABLE_PATH});
const page=await browser.newPage({viewport:{width:1440,height:1000}});
const errors=[];page.on('pageerror',e=>errors.push(e.message));
await page.addInitScript(()=>{
 globalThis.isTauri=true;
 let room=null;
 const copy=v=>JSON.parse(JSON.stringify(v));
 globalThis.__TAURI_INTERNALS__={invoke:async(cmd,args)=>{
  const demo=(await import('/src/demo.ts')).demo;
  if(cmd==='plugin:dialog|open')return '/fixture/shared-project';
  if(cmd==='open_project'||cmd==='refresh_project')return {...await demo.load(),root:'/fixture/shared-project',name:'共读交互测试（替身）'};
  if(cmd==='read_file')return demo.readFile(args.path);
  if(cmd==='get_changes')return demo.getChanges(args.base,args.head);
  if(cmd==='get_file_diff')return demo.getFileDiff(args.path,args.base,args.head);
  if(cmd==='journal_action')return {annotations:[],activities:[]};
  if(cmd==='collaboration_action'){
   const {action,payload}=args;
   if(action==='history')return [];
   if(action==='addresses')return ['192.168.1.2'];
   if(action==='leave'){room=null;return null;}
   if(action==='invitation')return 'http://192.168.1.2:45678/#session=fixture&invite=synthetic-fixture-only';
   if(action==='read')return copy(room);
   if(action==='create'){
    const project=await demo.load();room={id:'fixture',title:payload.title,revision:1,closed:false,me:'host',expiresAt:2000000000,inviteExpiresAt:2000000000,version:{commit:'a'.repeat(40),sourceDigest:'b'.repeat(64),associationDigest:'c'.repeat(64)},members:[{id:'host',name:payload.name,role:'host',status:'active'}],comments:[],tasks:[],hints:[],features:project.features.map(f=>({id:f.id,name:f.name,evidence:f.anchors.map(a=>({path:a.range.file,startLine:a.range.startLine,endLine:a.range.endLine})),modules:[]}))};return copy(room);
   }
   if(action==='comment'){room.comments.push({input:payload.comment,authorId:'host',createdAt:1793400000});room.revision++;return copy(room);}
   if(action==='task'){room.tasks.push({input:payload.task,status:'pending',revision:++room.revision,resultCommit:null,reviewNote:null,mergedInto:null});return copy(room);}
   if(action==='export')return '# Fixture task\n\n人工意见，按基准版本核对。';
   throw Error('Unhandled fixture collaboration action: '+action);
  }
  throw Error('Unhandled fixture IPC: '+cmd);
 }};
});
try {
 await page.goto(process.env.BASE_URL||'http://127.0.0.1:1420');
 await page.getByRole('button',{name:'共读',exact:true}).first().click();
 await expect(page.getByRole('dialog',{name:'局域网共读'})).toContainText('先打开已提交的真实');
 await page.getByRole('dialog',{name:'局域网共读'}).getByRole('button',{name:'关闭共读窗口'}).click();
 await page.getByRole('button',{name:'打开仓库',exact:true}).click();
 await expect(page.locator('.project-title h1')).toHaveText('共读交互测试（替身）');
 await page.getByRole('button',{name:'共读',exact:true}).first().click();
 const dialog=page.getByRole('dialog',{name:'局域网共读'});
 await dialog.getByLabel('你的称呼').fill('甲');await dialog.getByLabel('本轮名称').fill('登录流程共读');
 await dialog.getByRole('button',{name:'创建并生成邀请'}).click();
 await expect(dialog.getByLabel('共读邀请')).toHaveValue(/192\.168\.1\.2/);
 await dialog.getByRole('button',{name:'关闭共读窗口'}).click();
 await expect(page.getByLabel('共享批注内容')).toBeVisible();
 await page.getByLabel('共享批注内容').fill('<script>not executable</script> 请完善边界提示');
 await page.getByRole('button',{name:'提交共享批注',exact:true}).click();
 await expect(page.locator('.shared-comments')).toContainText('<script>not executable</script>');
 await expect(page.locator('.shared-comments script')).toHaveCount(0);
 await page.getByRole('button',{name:'整理批注',exact:true}).click();
 const organize=page.getByRole('dialog',{name:'整理批注与分工'});
 await organize.getByRole('button',{name:'整合为新任务'}).click();
 await organize.getByLabel('任务名称').fill('完善提示');await organize.getByLabel('目标',{exact:true}).fill('解释失败原因');
 await organize.getByLabel('怎样算完成').fill('失败分支有清楚提示');await organize.getByLabel('修改边界与保持不变的接口').fill('保持路由接口');
 await organize.getByRole('checkbox').first().check();await organize.getByRole('combobox',{name:'任务负责人',exact:true}).selectOption('host');
 await organize.getByRole('button',{name:'保存任务并检查耦合'}).click();await expect(organize.getByRole('heading',{name:'完善提示'})).toBeVisible();
 await organize.getByRole('button',{name:'导出 Markdown 任务包'}).click();await expect(organize.getByLabel('Markdown 任务包')).toHaveValue(/Fixture task/);
 await organize.getByRole('button',{name:'关闭共读窗口'}).click();
 await page.locator('.work-tabs').first().getByRole('button',{name:/我的批注/}).click();
 await expect(page.getByRole('button',{name:'＋ 记批注'})).toBeVisible();
 if(errors.length)throw Error(errors.join('\n'));
 console.log('PASS browser transport fixture: modal from hidden panel, explicit share, escaping, task consolidation/export and private notes remain separate.');
} finally {await browser.close();}
