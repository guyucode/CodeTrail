import {invoke} from '@tauri-apps/api/core';
export interface SharedScope {path:string;startLine:number;endLine:number}
export interface SharedMember {id:string;name:string;role:string;status:string}
export interface SharedCommentInput {id:string;body:string;featureId:string|null;scope:SharedScope|null;parentId:string|null}
export interface SharedComment {input:SharedCommentInput;authorId:string;createdAt:number}
export interface TaskInput {id:string;title:string;goal:string;acceptance:string;boundary:string;commentIds:string[];featureIds:string[];scopes:SharedScope[];contracts:string[];ownerId:string|null;dependsOn:string[];couplingNote:string}
export interface SharedTask {input:TaskInput;status:string;revision:number;resultCommit:string|null;reviewNote:string|null;mergedInto:string|null}
export interface SharedSession {
 id:string;title:string;revision:number;closed:boolean;me:string;expiresAt:number;inviteExpiresAt:number;
 version:{commit:string;sourceDigest:string;associationDigest:string};
 members:SharedMember[];comments:SharedComment[];tasks:SharedTask[];
 features:{id:string;name:string;evidence:SharedScope[];modules:string[]}[];
 hints:{left:string;right:string;kind:string;reason:string}[];
 offline?:boolean;connectionError?:string;
}
export const collaboration=<T=SharedSession|null>(root:string,action:string,payload:unknown={})=>invoke<T>('collaboration_action',{root,action,payload});
export const taskLabels:Record<string,string>={pending:'待处理',accepted:'已接手',needsReview:'待人工复核',reviewed:'已人工复核',merged:'已合并'};
export function emptyTask():TaskInput{return {id:crypto.randomUUID(),title:'',goal:'',acceptance:'',boundary:'',commentIds:[],featureIds:[],scopes:[],contracts:[],ownerId:null,dependsOn:[],couplingNote:''};}
export function commentThreads(comments:SharedComment[]) {
 const ids=new Set(comments.map(c=>c.input.id));
 return comments.filter(c=>!c.input.parentId||!ids.has(c.input.parentId)).map(root=>({root,replies:comments.filter(c=>c.input.parentId===root.input.id)}));
}
