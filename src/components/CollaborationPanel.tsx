import {useEffect,useRef,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import {createPortal} from 'react-dom';
import {collaboration,emptyTask,taskLabels} from '../collaboration';
import type {SharedComment,SharedCommentInput,SharedSession,TaskInput} from '../collaboration';
import type {Annotation,NoteContext} from './WorkPanel';
import type {RuntimeLocation} from './RuntimePane';

function Overlay({title,onClose,children}:{title:string;onClose:()=>void;children:React.ReactNode}) {
 const ref=useRef<HTMLDivElement>(null);
 useEffect(()=>{const old=document.activeElement as HTMLElement|null;ref.current?.focus();return()=>old?.focus();},[]);
 return createPortal(<div className="modal-backdrop" onKeyDown={e=>{if(e.key==='Escape'){e.stopPropagation();onClose();}if(e.key==='Tab'){const nodes=[...ref.current!.querySelectorAll<HTMLElement>('button:not(:disabled),input:not(:disabled),textarea:not(:disabled),select:not(:disabled),[tabindex="0"]')].filter(n=>n.getClientRects().length);const first=nodes[0],last=nodes[nodes.length-1];if(e.shiftKey&&(document.activeElement===first||document.activeElement===ref.current)){e.preventDefault();last?.focus();}else if(!e.shiftKey&&document.activeElement===last){e.preventDefault();first?.focus();}}}}><div ref={ref} tabIndex={-1} role="dialog" aria-modal="true" aria-label={title} className="modal modal-wide collab-dialog"><div className="modal-heading"><h2>{title}</h2><button onClick={onClose} aria-label="关闭共读窗口">关闭</button></div>{children}</div></div>,document.body);
}
const draftKey=(root:string,id:string)=>`codetrail-shared-draft:${encodeURIComponent(root)}:${id}`;
interface Draft {comment:SharedCommentInput;localFileHash:string|null}
export default function CollaborationPanel({root,demo,context,open,onClose,onConnected,onNavigate}:{root:string;demo:boolean;context:NoteContext;open:boolean;onClose:()=>void;onConnected:(active:boolean)=>void;onNavigate?:(location:RuntimeLocation)=>void}) {
 const [session,setSession]=useState<SharedSession|null>(null);const current=useRef(session);current.current=session;
 const [error,setError]=useState('');const [syncError,setSyncError]=useState('');const [busy,setBusy]=useState(false);const [mode,setMode]=useState<'create'|'join'>('create');
 const [name,setName]=useState('');const [title,setTitle]=useState('');const [invitation,setInvitation]=useState('');const [shareInvite,setShareInvite]=useState('');
 const [draft,setDraft]=useState<Draft|null>(null);const [organize,setOrganize]=useState(false);const [task,setTask]=useState<TaskInput|null>(null);
 const [addresses,setAddresses]=useState<string[]>([]);const [address,setAddress]=useState('');
 const [markdown,setMarkdown]=useState('');const [owner,setOwner]=useState('');const [personal,setPersonal]=useState<Annotation[]>([]);
 const [history,setHistory]=useState<{id:string;title:string;version:{commit:string}}[]>([]);
 const [mergeTarget,setMergeTarget]=useState('');const [mergeSources,setMergeSources]=useState<string[]>([]);
 const [resultCommit,setResultCommit]=useState('');const [reviewNote,setReviewNote]=useState('');
 const mounted=useRef(true);const mutation=useRef(false);const connected=useRef(onConnected);connected.current=onConnected;
 const requestGeneration=useRef(0);
 function adopt(s:SharedSession|null){if(!mounted.current)return;current.current=s;setSession(s);connected.current(!!s);}
 useEffect(()=>{
  mounted.current=true;if(demo)return;
  let polling=false;
  async function read(){if(polling||mutation.current)return;polling=true;const generation=requestGeneration.current;try{const s=await collaboration(root,'read');if(mounted.current&&generation===requestGeneration.current){adopt(s);setSyncError('');}}catch(e){if(mounted.current)setSyncError(String(e));}finally{polling=false;}}
  void read();const timer=setInterval(()=>void read(),2500);
  return()=>{mounted.current=false;clearInterval(timer);};
 },[root,demo]);
 useEffect(()=>{
  if(!session)return;
  try{const saved=localStorage.getItem(draftKey(root,session.id));setDraft(saved?JSON.parse(saved):null);}catch{setError('共享草稿恢复失败，请保留当前输入。');}
 },[root,session?.id]);
 useEffect(()=>{if(open&&!demo)void collaboration<typeof history>(root,'history').then(setHistory).catch(e=>setError(String(e)));},[open,root,demo]);
 function remember(value:Draft|null){setDraft(value);if(!session)return;try{value?localStorage.setItem(draftKey(root,session.id),JSON.stringify(value)):localStorage.removeItem(draftKey(root,session.id));}catch{setError('本机草稿保存失败，请勿关闭窗口。');}}
 async function run(action:string,payload:Record<string,unknown>={}) {
  if(mutation.current)return;mutation.current=true;requestGeneration.current++;setBusy(true);setError('');
  try {const value=await collaboration(root,action,{expectedRevision:current.current?.revision,...payload});adopt(value);return true;}catch(e){if(mounted.current)setError(String(e));return false;}finally{mutation.current=false;if(mounted.current)setBusy(false);}
 }
 const me=session?.members.find(m=>m.id===session.me);const host=me?.role==='host';const active=me?.status==='active';
 const writable=!!session&&active&&!session.closed&&!session.offline&&!busy;
 const author=(id:string)=>session?.members.find(m=>m.id===id)?.name||'已离开的成员';
 function beginReply(c:SharedComment){if(draft?.comment.body.trim()&&!window.confirm('当前共享草稿尚未提交，替换为回复？'))return;remember({comment:{id:crypto.randomUUID(),body:'',featureId:c.input.featureId,scope:c.input.scope,parentId:c.input.id},localFileHash:null});}
 async function publish(){if(!draft?.comment.body.trim())return;const sent=draft;if(await run('comment',sent as unknown as Record<string,unknown>))remember(null);}
 async function copy(text:string){try{await navigator.clipboard.writeText(text);}catch{setError('复制失败，请在文本框中手动选择复制。');}}
 async function exportTasks(){try{adopt(await collaboration(root,'read'));setMarkdown(await collaboration<string>(root,'export',{ownerId:owner||session?.me}));}catch(e){setError(String(e));}}
 function composer(){return <div className="shared-composer"><label>共享批注{draft?.comment.parentId?' · 回复':''}<textarea aria-label="共享批注内容" maxLength={2000} placeholder="写下要与成员讨论的问题，提交后才会共享。" value={draft?.comment.body||''} disabled={!writable} onChange={e=>{
  const value=draft||{comment:{id:crypto.randomUUID(),body:'',featureId:context.featureId,scope:context.path&&context.startLine&&context.endLine?{path:context.path,startLine:context.startLine,endLine:context.endLine}:null,parentId:null},localFileHash:context.fileHash};remember({...value,comment:{...value.comment,body:e.target.value}});
 }}/></label>{draft?.comment.scope&&<small>{draft.comment.scope.path} · L{draft.comment.scope.startLine}–{draft.comment.scope.endLine}</small>}<small>草稿仅保存在本机；明确提交后同步给本轮成员。</small><div className="note-actions"><button disabled={!writable||!draft?.comment.body.trim()} onClick={()=>void publish()}>提交共享批注</button>{draft&&<button onClick={()=>{if(!draft.comment.body.trim()||window.confirm('丢弃这份未提交的共享草稿？'))remember(null);}}>清除草稿</button>}<button disabled={!writable} onClick={async()=>{try{const j=await invoke<{annotations:Annotation[]}>('journal_action',{root,action:'read',payload:{}});setPersonal(j.annotations);}catch(e){setError(String(e));}}}>引用个人批注</button></div>
 {personal.length>0&&<label>只复制你选中的一条<select aria-label="选择个人批注" defaultValue="" onChange={e=>{const n=personal.find(n=>n.id===e.target.value);if(!n)return;if(draft?.comment.body.trim()&&!window.confirm('替换当前未提交草稿？'))return;remember({comment:{id:crypto.randomUUID(),body:n.body,featureId:n.featureId,scope:n.path&&n.startLine&&n.endLine?{path:n.path,startLine:n.startLine,endLine:n.endLine}:null,parentId:null},localFileHash:n.fileHash});setPersonal([]);}}><option value="">请选择要共享的批注</option>{personal.map(n=><option key={n.id} value={n.id}>{n.body.slice(0,70)}</option>)}</select></label>}</div>;}
 return <>
  {session?<div className="shared-reading"><div className="shared-heading"><strong>{session.title}</strong><button disabled={!active} onClick={()=>setOrganize(true)}>整理批注</button></div><p className="work-hint">本轮版本固定；提交或跳转前核对当前代码。外部改动不会自动迁移旧批注。</p><small>本轮 {session.version.commit.slice(0,8)} · {session.members.filter(m=>m.status==='active').length} 人{session.closed?' · 已结束':''}</small>
  {session.offline&&<p role="status" className="work-error">当前显示本机已同步副本，暂不能提交。{session.connectionError}。主持人须保持应用打开。</p>}
  {!active&&<p>等待发起人批准加入；批准前不显示共享讨论。</p>}
  {host&&session.members.filter(m=>m.status==='pending').map(m=><article key={m.id} className="work-card"><p>{m.name} 申请加入</p><button disabled={busy} onClick={()=>void run('member',{memberId:m.id,status:'active'})}>批准</button><button disabled={busy} onClick={()=>void run('member',{memberId:m.id,status:'removed'})}>拒绝</button></article>)}
  {active&&<>{composer()}<div className="shared-comments">{!session.comments.length&&<p className="work-hint">暂无共享批注。个人批注不会自动出现在这里。</p>}{session.comments.map(c=><article className="work-card" key={c.input.id}><small>{author(c.authorId)} · {new Date(c.createdAt*1000).toLocaleString()}{c.input.parentId?' · 回复':''}</small>{c.input.featureId&&<b>{session.features.find(f=>f.id===c.input.featureId)?.name}</b>}{c.input.scope&&<small>{c.input.scope.path} · L{c.input.scope.startLine}–{c.input.scope.endLine}</small>}<p>{c.input.body}</p>{(c.input.scope||c.input.featureId)&&<button disabled={busy} onClick={async()=>{try{const location=await collaboration<RuntimeLocation>(root,'location',{scope:c.input.scope,featureId:c.input.featureId});onNavigate?.(location);}catch(e){setError(String(e));}}}>查看对应代码</button>}<button disabled={!writable} onClick={()=>beginReply(c)}>回复</button></article>)}</div></>}
  </div>:<p className="work-hint">点顶部“共读”创建局域网会话，或粘贴别人发来的邀请。</p>}
  {error&&<p role="alert" className="work-error">{error}</p>}{syncError&&<p role="status" className="work-error">{syncError}</p>}
  {open&&<Overlay title="局域网共读" onClose={onClose}>
   {error&&<p role="alert" className="work-error">{error}</p>}{syncError&&<p role="status" className="work-error">{syncError}</p>}
   {demo?<p>先打开已提交的真实 Git 项目，再发起或加入共读。</p>:session?<>
    <p>{session.title} · 基准 {session.version.commit.slice(0,12)}</p><p>代码留在各自电脑。只同步明确共享的批注、功能范围和任务；当前采用局域网明文连接，仅用于可信网络，不开放公网。</p>
    <p>本轮固定代码版本和功能关联。成员可独立阅读，提交前重新核对本地版本；源码变化请另建一轮。</p>
    {host&&!session.closed&&<><button disabled={busy||session.offline} onClick={async()=>{try{setShareInvite(await collaboration<string>(root,'invitation'));setAddresses(await collaboration<string[]>(root,'addresses'));}catch(e){setError(String(e));}}}>显示邀请</button><button disabled={busy||session.offline} onClick={async()=>{if(await run('rotateInvite'))setShareInvite(await collaboration<string>(root,'invitation'));setAddresses(await collaboration<string[]>(root,'addresses'));}}>撤销旧邀请并生成新邀请</button>
    {shareInvite&&<><textarea readOnly aria-label="共读邀请" value={shareInvite}/><button onClick={()=>void copy(shareInvite)}>复制邀请</button>{shareInvite.includes('127.0.0.1')&&<p>未检测到局域网地址，此邀请仅可用于本机测试。</p>}{addresses.length>1&&<details><summary>邀请连接不上？选择本机网卡</summary><label>局域网地址<select aria-label="局域网地址" value={address||new URL(shareInvite).hostname} onChange={e=>setAddress(e.target.value)}>{addresses.map(ip=><option key={ip} value={ip}>{ip}</option>)}</select></label><button disabled={busy} onClick={async()=>{if(await run('advertise',{address:address||new URL(shareInvite).hostname}))setShareInvite(await collaboration<string>(root,'invitation'));}}>更新邀请地址</button></details>}</>}</>}
    <div className="shared-members">{session.members.map(m=><p key={m.id}>{m.name} · {m.role==='host'?'发起人':m.status==='active'?'已加入':m.status==='pending'?'待批准':'已移出'}{host&&m.role!=='host'&&m.status==='active'&&<button disabled={busy||session.closed||session.offline} onClick={()=>{if(window.confirm(`移除 ${m.name}？已同步到对方电脑的内容无法远程删除。`))void run('member',{memberId:m.id,status:'removed'});}}>移出</button>}</p>)}</div>
    {host&&!session.closed&&<button disabled={busy||session.offline} onClick={()=>{if(window.confirm('结束本轮后只能阅读和导出；新版本可另建共读。'))void run('close');}}>结束本轮</button>}
    <button disabled={busy} onClick={async()=>{if(await run('leave')){setShareInvite('');onClose();}}}>离开本轮（保留本机历史）</button>
   </>:<><div className="work-tabs"><button aria-pressed={mode==='create'} onClick={()=>setMode('create')}>发起共读</button><button aria-pressed={mode==='join'} onClick={()=>setMode('join')}>加入共读</button></div><label>你的称呼<input value={name} maxLength={40} onChange={e=>setName(e.target.value)}/></label>
    {mode==='create'?<label>本轮名称<input value={title} maxLength={60} onChange={e=>setTitle(e.target.value)} placeholder="例如：登录流程评审"/></label>:<label>粘贴邀请<textarea value={invitation} onChange={e=>setInvitation(e.target.value)} maxLength={2000}/></label>}
    <p>参与者各自打开相同版本的 Git 项目。阅读范围内的代码须先提交，功能关联和概览也须一致。不会自动上传源码或个人批注。</p><p>发起人需要保持 CodeTrail 打开；局域网明文连接只用于可信网络。无需另装服务器。</p>
    <button disabled={busy||!name.trim()||(mode==='create'?!title.trim():!invitation.trim())} onClick={async()=>{if(await run(mode,{name:name.trim(),title:title.trim(),invitation:invitation.trim()})){setName('');setTitle('');setInvitation('');if(mode==='create'){setShareInvite(await collaboration<string>(root,'invitation'));setAddresses(await collaboration<string[]>(root,'addresses'));}else onClose();}}}>{busy?'正在连接…':mode==='create'?'创建并生成邀请':'申请加入'}</button>
    <button disabled={busy} onClick={()=>void run('leave')}>清除失效连接（保留历史）</button>
    {history.length>0&&<details><summary>本机历史共读</summary>{history.map(h=><p key={h.id}>{h.title} · {h.version.commit.slice(0,8)} <button disabled={busy} onClick={async()=>{if(await run('resume',{id:h.id}))onClose();}}>打开历史</button></p>)}</details>}
   </>}
  </Overlay>}
  {organize&&session&&<Overlay title="整理批注与分工" onClose={()=>setOrganize(false)}>
   {error&&<p role="alert" className="work-error">{error}</p>}{syncError&&<p role="status" className="work-error">{syncError}</p>}<p>把相关意见合成一个有边界的任务；原始批注保留。发起人负责分工，各成员可接手自己的任务并导出。</p>
   {host&&<button disabled={!writable} onClick={()=>setTask(emptyTask())}>整合为新任务</button>}
   {task&&host&&<form className="shared-task-form" onSubmit={async e=>{e.preventDefault();if(await run('task',{task:{...task,contracts:[...new Set(task.contracts.map(v=>v.trim()).filter(Boolean))]}}))setTask(null);}}>
    <label>任务名称<input required value={task.title} maxLength={120} onChange={e=>setTask({...task,title:e.target.value})}/></label>
    <label>目标<textarea required value={task.goal} maxLength={3000} onChange={e=>setTask({...task,goal:e.target.value})}/></label>
    <label>怎样算完成<textarea required value={task.acceptance} maxLength={1500} onChange={e=>setTask({...task,acceptance:e.target.value})}/></label>
    <label>修改边界与保持不变的接口<textarea required value={task.boundary} maxLength={1500} onChange={e=>setTask({...task,boundary:e.target.value})}/></label>
    <fieldset><legend>来源批注（保留原话）</legend>{session.comments.map(c=><label className="shared-check" key={c.input.id}><input type="checkbox" checked={task.commentIds.includes(c.input.id)} onChange={e=>setTask({...task,commentIds:e.target.checked?[...task.commentIds,c.input.id]:task.commentIds.filter(id=>id!==c.input.id)})}/>{author(c.authorId)}：{c.input.body.slice(0,160)}</label>)}</fieldset>
    <details><summary>补充功能与共享约定</summary>{session.features.map(f=><label className="shared-check" key={f.id}><input type="checkbox" checked={task.featureIds.includes(f.id)} onChange={e=>setTask({...task,featureIds:e.target.checked?[...task.featureIds,f.id]:task.featureIds.filter(id=>id!==f.id)})}/>{f.name}</label>)}<label>共同涉及的接口、配置或数据约定（每行一项）<textarea value={task.contracts.join('\n')} onChange={e=>setTask({...task,contracts:e.target.value.split('\n')})}/></label></details>
    <label>负责人<select aria-label="任务负责人" value={task.ownerId||''} onChange={e=>setTask({...task,ownerId:e.target.value||null})}><option value="">暂不分配</option>{session.members.filter(m=>m.status==='active').map(m=><option key={m.id} value={m.id}>{m.name}</option>)}</select></label>
    <fieldset><legend>须先完成并复核的任务</legend>{session.tasks.filter(t=>t.status!=='merged'&&t.input.id!==task.id).map(t=><label className="shared-check" key={t.input.id}><input type="checkbox" checked={task.dependsOn.includes(t.input.id)} onChange={e=>setTask({...task,dependsOn:e.target.checked?[...task.dependsOn,t.input.id]:task.dependsOn.filter(id=>id!==t.input.id)})}/>{t.input.title}</label>)}</fieldset>
    <label>耦合判断与分工理由<textarea value={task.couplingNote} maxLength={1500} onChange={e=>setTask({...task,couplingNote:e.target.value})} placeholder="共享逻辑由谁负责？哪些接口保持不变？何时可以并行？"/></label>
    <button disabled={!writable||!task.commentIds.length} type="submit">保存任务并检查耦合</button><button type="button" onClick={()=>setTask(null)}>取消编辑</button>
   </form>}
   {mergeTarget&&host&&<section className="shared-task-form"><b>并入「{session.tasks.find(t=>t.input.id===mergeTarget)?.input.title}」</b><p>合并后沿用目标任务的负责人；来源任务保留为历史，下游依赖同步调整。</p>{session.tasks.filter(t=>t.status==='pending'&&t.input.id!==mergeTarget).map(t=><label className="shared-check" key={t.input.id}><input type="checkbox" checked={mergeSources.includes(t.input.id)} onChange={e=>setMergeSources(e.target.checked?[...mergeSources,t.input.id]:mergeSources.filter(id=>id!==t.input.id))}/>{t.input.title}</label>)}<button disabled={!writable||!mergeSources.length} onClick={async()=>{if(await run('mergeTasks',{targetId:mergeTarget,sourceIds:mergeSources})){setMergeTarget('');setMergeSources([]);}}}>确认合并</button><button onClick={()=>setMergeTarget('')}>取消合并</button></section>}
   {session.tasks.filter(t=>t.status!=='merged').map(t=><article className="work-card" key={t.input.id}><h3>{t.input.title}</h3><small>{taskLabels[t.status]} · {t.input.ownerId?author(t.input.ownerId):'待分工'}</small><p>{t.input.goal}</p><details><summary>范围、验收与原始意见</summary><p>验收：{t.input.acceptance}</p><p>边界：{t.input.boundary}</p><p>耦合：{t.input.couplingNote||'待确认'}</p><p>前置：{t.input.dependsOn.map(id=>session.tasks.find(t=>t.input.id===id)?.input.title||id).join('、')||'无'}</p>{t.input.commentIds.map(id=><blockquote key={id}>{session.comments.find(c=>c.input.id===id)?.input.body}</blockquote>)}{t.resultCommit&&<p>交付提交：{t.resultCommit}</p>}{t.reviewNote&&<p>人工复核：{t.reviewNote}</p>}</details>
    {host&&<><button disabled={!writable} onClick={()=>setTask({...t.input})}>调整任务</button>{t.status==='pending'&&<button disabled={!writable} onClick={()=>{setMergeTarget(t.input.id);setMergeSources([]);}}>合并待处理任务</button>}</>}
    {t.input.ownerId===session.me&&t.status==='pending'&&t.input.ownerId&&<button disabled={!writable} onClick={()=>void run('taskStatus',{taskId:t.input.id,status:'accepted'})}>确认接手</button>}
    {t.input.ownerId===session.me&&t.status==='accepted'&&<button disabled={!writable||!resultCommit.trim()} onClick={()=>void run('taskStatus',{taskId:t.input.id,status:'needsReview',resultCommit:resultCommit.trim()})}>提交待复核</button>}
    {host&&t.status==='needsReview'&&<button disabled={!writable||!reviewNote.trim()} onClick={()=>void run('taskStatus',{taskId:t.input.id,status:'reviewed',resultCommit:t.resultCommit,reviewNote:reviewNote.trim()})}>人工复核完成</button>}
    {(host||t.input.ownerId===session.me)&&t.status!=='pending'&&<button disabled={!writable} onClick={()=>void run('taskStatus',{taskId:t.input.id,status:'pending'})}>退回待处理</button>}
   </article>)}
   {session.tasks.some(t=>t.status==='merged')&&<details><summary>已合并任务历史</summary>{session.tasks.filter(t=>t.status==='merged').map(t=><p key={t.input.id}>{t.input.title} → {session.tasks.find(target=>target.input.id===t.mergedInto)?.input.title}<br/>{t.input.goal}</p>)}</details>}
   {session.tasks.some(t=>t.status!=='merged')&&<><label>实际交付的 Git 提交 ID<input value={resultCommit} onChange={e=>setResultCommit(e.target.value)} placeholder="用于提交待复核，不自动验证业务结果" maxLength={64}/></label>{host&&<label>人工复核说明<textarea value={reviewNote} onChange={e=>setReviewNote(e.target.value)} maxLength={1500}/></label>}
   <details><summary>查看任务间的耦合提示（不是完整依赖证明）</summary>{session.hints.map(h=><p key={h.left+h.right}><b>{session.tasks.find(t=>t.input.id===h.left)?.input.title} ↔ {session.tasks.find(t=>t.input.id===h.right)?.input.title}</b><br/>{h.reason}</p>)}</details>
   <label>导出给谁<select aria-label="导出负责人" value={owner||session.me} onChange={e=>setOwner(e.target.value)}>{session.members.map(m=><option key={m.id} value={m.id}>{m.name}</option>)}</select></label><button disabled={busy} onClick={()=>void exportTasks()}>导出 Markdown 任务包</button></>}
   {markdown&&<><textarea readOnly aria-label="Markdown 任务包" className="shared-markdown" value={markdown}/><button onClick={()=>void copy(markdown)}>复制给 Agent</button><button onClick={async()=>{try{await invoke('save_collaboration_export',{root,ownerId:owner||session.me});}catch(e){setError(String(e));}}}>另存为 Markdown</button></>}
  </Overlay>}
 </>;
}
