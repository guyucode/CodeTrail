import {describe,it,expect} from 'vitest';
import {createElement} from 'react';
import {renderToStaticMarkup} from 'react-dom/server';
import CollaborationPanel from '../src/components/CollaborationPanel';
import WorkPanel from '../src/components/WorkPanel';
const context={context:'项目概览',path:null,startLine:null,endLine:null,fileHash:null,featureId:null,featureName:null,quote:''};
describe('shared reading stays separate from private feedback',()=>{
 it('does not invite or upload while rendering a demo, and keeps the existing personal workflow',()=>{
  const html=renderToStaticMarkup(createElement(WorkPanel,{root:'/demo',demo:true,context,collaborationOpen:false,onCloseCollaboration:()=>{}}));
  expect(html).toContain('我的批注');expect(html).toContain('Agent 动态');expect(html).not.toContain('提交共享批注');expect(html).not.toContain('密码');
 });
 it('explains explicit joining instead of implying collaboration is already connected',()=>{
  const html=renderToStaticMarkup(createElement(CollaborationPanel,{root:'/fixture',demo:true,context,open:false,onClose:()=>{},onConnected:()=>{}}));
  expect(html).toContain('邀请');expect(html).not.toContain('提交共享批注');
 });
});
