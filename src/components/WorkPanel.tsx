import { useEffect, useRef, useState } from 'react';
import CollaborationPanel from './CollaborationPanel';
import type {RuntimeLocation} from './RuntimePane';
import { invoke } from '@tauri-apps/api/core';

export interface Activity { projectRoot?:string|null; kind?:string|null; files?:string[]; id:string; task:string; stage:string; places:string[]; impact:string; expectedEffect:string; verification:string|null; recordedAt:string }
export interface Annotation { id:string; body:string; context:string; path:string|null; startLine:number|null; endLine:number|null; fileHash:string|null; featureId:string|null; featureName:string|null; quote:string; version:string }
export type NoteContext = Omit<Annotation,'id'|'body'|'version'>;
interface Journal { activities:Activity[]; annotations:Annotation[] }
const stages:Record<string,string> = {working:'修改中',checking:'验证中',delivered:'已交付',blocked:'待处理'};
export function projectActivities(root:string, events:Activity[]) {
  return events.filter(e=>e.kind==='projectChange' && e.projectRoot===root && !!e.files?.length && e.files.length<=30 && e.files.every(path=>{
    const parts=path.split('/');
    return !!path && !path.startsWith('/') && !path.includes('\\') && !/^[A-Za-z]:/.test(path)
      && !/[?*]/.test(path) && parts.every(part=>!!part && part!=='.' && part!=='..' && !['.reader','.git'].includes(part) && !part.includes(':') && !part.includes('\0'));
  }));
}
export function ActivityCards({events,root}:{events:Activity[];root:string}) {
  return <>{[...projectActivities(root,events)].reverse().map(e=><article className="work-card" key={e.id}>
    <small>{stages[e.stage] || e.stage} · {e.recordedAt ? new Date(Number(e.recordedAt)*1000).toLocaleString() : ''}</small><h3>{e.task}</h3>
    <b>改了哪里</b><p>{e.places.join('、')}</p><details><summary>涉及的项目文件</summary><p>{e.files?.join('\n')}</p></details><b>影响什么</b><p>{e.impact}</p><b>预期效果</b><p>{e.expectedEffect}</p>
    <b>Agent 报告的验证结果</b><p>{e.verification || '尚未报告验证结果'}</p>
  </article>)}</>;
}
const command = <T,>(root:string, action:string, payload:unknown = {}) => invoke<T>('journal_action',{root,action,payload});
const draftKey = (root:string,id:string) => `codetrail-note:${encodeURIComponent(root)}:${id}`;
function remember(root:string,note:Annotation) { localStorage.setItem(draftKey(root,note.id),JSON.stringify(note)); }
function NoteEditor({root,note,onSaved,onDelete,onPending}:{root:string;note:Annotation;onSaved:(n:Annotation)=>void;onDelete:()=>void;onPending:(id:string,pending:boolean)=>void}) {
  const [body,setBody] = useState(note.body);
  const [status,setStatus] = useState(note.version ? '已自动保存' : '请输入批注');
  const [retry,setRetry] = useState(0);
  const latest = useRef(note); const saving = useRef(false); const dirty = useRef(!!localStorage.getItem(draftKey(root,note.id)) || (!note.version && !!note.body)); const deleting = useRef(false);
  const mounted = useRef(true);
  const pending = useRef(onPending); pending.current=onPending;
  const saveRef = useRef<()=>Promise<void>>(async()=>{});
  saveRef.current = async () => {
    if (saving.current || deleting.current || !dirty.current) return;
    if (!latest.current.body.trim()) { setStatus('内容为空，尚未保存'); return; }
    saving.current=true; setStatus('保存中…');
    try {
      while(dirty.current) {
        const submitted={...latest.current}; dirty.current=false;
        const saved=await command<Annotation>(root,'save',{note:submitted,expectedVersion:submitted.version});
        latest.current={...latest.current,version:saved.version};
        if (dirty.current) remember(root,latest.current);
        else { localStorage.removeItem(draftKey(root,note.id)); pending.current(note.id,false); }
        if(mounted.current) { onSaved({...latest.current}); setStatus(dirty.current?'保存中…':'已自动保存'); }
      }
    } catch(e) { dirty.current=true; if(mounted.current) setStatus(`保存失败：${String(e)}。草稿保留在本机；如版本冲突，请重启应用并重新打开此项目恢复。`); }
    finally { saving.current=false; }
  };
  useEffect(()=>{ const t=setTimeout(()=>void saveRef.current(),600); return()=>clearTimeout(t); },[body,retry]);
  useEffect(()=>{mounted.current=true;return()=>{mounted.current=false;void saveRef.current();};},[]);
  return <article className="work-card"><small>{note.context}</small>{note.featureName && <p>{note.featureName}</p>}
    {note.path && <small>{note.path} · L{note.startLine}–{note.endLine}</small>}
    <textarea aria-label="批注内容" placeholder="哪里不明白？希望怎么修改？" value={body} maxLength={20000} onChange={e=>{
      const value=e.target.value; setBody(value); latest.current={...latest.current,body:value}; dirty.current=true; pending.current(note.id,true);
      try {remember(root,latest.current);setStatus('等待自动保存…');} catch {setStatus('本机草稿保存失败，请勿关闭页面');}
    }}/>
    <small role="status">{status}</small><div className="note-actions"><button onClick={()=>setRetry(v=>v+1)}>重试保存</button>
      <button disabled={saving.current} onClick={async()=>{deleting.current=true;try {
        if(latest.current.version) await command(root,'delete',{id:note.id,expectedVersion:latest.current.version});
        dirty.current=false;localStorage.removeItem(draftKey(root,note.id));pending.current(note.id,false);onDelete();
      }catch(e){deleting.current=false;setStatus(String(e));}}}>删除</button></div>
  </article>;
}
export default function WorkPanel({root,context,demo,collaborationOpen,onCloseCollaboration,onSharedCode}:{root:string;context:NoteContext;demo:boolean;collaborationOpen:boolean;onCloseCollaboration:()=>void;onSharedCode?:(location:RuntimeLocation)=>void}) {
  const [tab,setTab]=useState<'activity'|'notes'|'shared'>('activity');
  const [shared,setShared]=useState(false);
  const [events,setEvents]=useState<Activity[]>([]); const [notes,setNotes]=useState<Annotation[]>([]);
  const [error,setError]=useState(''); const [loaded,setLoaded]=useState(false); const [exported,setExported]=useState('');
  const [pending,setPending]=useState<Set<string>>(new Set());
  useEffect(()=>{
    if(demo) return;
    let alive=true; let polling=false; let initialized=false;
    async function read(){if(polling)return;polling=true;try{
      const j=await command<Journal>(root,'read'); if(!alive)return;
      setEvents(projectActivities(root,j.activities));setError('');
      if(!initialized){
        const map=new Map(j.annotations.map(n=>[n.id,n])); const recovered:string[]=[];
        const keys=Object.keys(localStorage).filter(key=>key.startsWith(`codetrail-note:${encodeURIComponent(root)}:`));
        for(const key of keys){
          const draft:Annotation=JSON.parse(localStorage.getItem(key)!);
          // A stale draft never silently overwrites a concurrent human edit.
          if(map.has(draft.id) && map.get(draft.id)!.version!==draft.version) {
            draft.id=crypto.randomUUID();draft.version='';draft.context+='（恢复草稿，请核对另一版本）';remember(root,draft);localStorage.removeItem(key);
          }
          map.set(draft.id,draft);recovered.push(draft.id);
        }
        setNotes([...map.values()]);setPending(new Set(recovered));setLoaded(true);initialized=true;
      }
    }catch(e){if(alive)setError(String(e));}finally{polling=false;}}
    void read(); const timer=setInterval(()=>void read(),2000);
    return()=>{alive=false;clearInterval(timer);};
  },[root,demo]);
  useEffect(()=>{const handler=(e:BeforeUnloadEvent)=>{if(pending.size){e.preventDefault();}};window.addEventListener('beforeunload',handler);return()=>window.removeEventListener('beforeunload',handler);},[pending]);
  function add(){const note:Annotation={...context,id:crypto.randomUUID(),body:'',version:''};setNotes(n=>[note,...n]);setTab('notes');}
  return <aside className="work-panel" aria-label="Agent 动态和我的批注">
    <div className="work-tabs"><button aria-pressed={tab==='activity'} onClick={()=>setTab('activity')}>Agent 动态</button><button aria-pressed={tab==='notes'} onClick={()=>setTab('notes')}>我的批注 {notes.length||''}</button>{shared&&<button aria-pressed={tab==='shared'} onClick={()=>setTab('shared')}>共读</button>}</div>
    <div hidden={tab!=='shared'} className="shared-panel-body"><CollaborationPanel root={root} demo={demo} context={context} open={collaborationOpen} onClose={onCloseCollaboration} onNavigate={onSharedCode} onConnected={active=>{setShared(active);if(active&&collaborationOpen)setTab('shared');if(!active&&tab==='shared')setTab('notes');}}/></div>
    <button hidden={tab==='shared'} className="add-note" disabled={demo||!loaded} onClick={add}>＋ 记批注</button>
    {demo?<p className="work-hint">打开本地项目后，可查看动态、记录批注。</p>:<>
      {error && <p role="alert" className="work-error">{error}</p>}
      <div className="work-scroll" hidden={tab==='shared'}>
        {tab==='activity'?<><p className="work-hint">仅显示当前项目的修改报告，每 2 秒更新；程序执行结果请看“运行记录”。历史状态不代表 Agent 仍在线。</p>{!events.length&&<p className="work-hint">暂无修改动态。Agent 修改本项目并上报后，会显示在这里。</p>}<ActivityCards events={events} root={root}/></>:<>
          <p className="work-hint">停笔后自动保存。批注属于你，不会自动发送给 Agent。</p>
          <button disabled={!loaded||!!pending.size||!notes.length} onClick={async()=>{try{setExported(await command<string>(root,'export'));}catch(e){setError(String(e));}}}>导出 Markdown</button>
          {pending.size>0&&<p className="work-hint">还有批注未保存，保存后可导出。</p>}
          {exported&&<p className="export-path">已导出：{exported}</p>}
          {!notes.length&&<p className="work-hint">在概览、功能或代码阅读时，点击“记批注”。</p>}
        </>}
        {/* Keep editors mounted while viewing activity, so autosaves finish. */}
        <div hidden={tab!=='notes'}>{notes.map(note=><NoteEditor key={note.id} root={root} note={note} onPending={(id,p)=>setPending(old=>{const next=new Set(old);p?next.add(id):next.delete(id);return next;})} onSaved={saved=>setNotes(old=>old.map(n=>n.id===saved.id?saved:n))} onDelete={()=>setNotes(old=>old.filter(n=>n.id!==note.id))}/>)}</div>
      </div>
    </>}
  </aside>;
}
