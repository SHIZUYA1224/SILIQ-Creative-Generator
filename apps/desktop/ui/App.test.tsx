import { cleanup,fireEvent,render,screen,waitFor } from '@testing-library/react';
import { afterEach,beforeEach,describe,expect,it,vi } from 'vitest';
import fixture from '../../../fixtures/public-safe/documents/basic.json';
import type { DesignDocument,Node,SessionView } from './api';

const bridge=vi.hoisted(()=>({call:vi.fn()}));
vi.mock('./api',async()=>({...await vi.importActual<typeof import('./api')>('./api'),nativeAvailable:true,call:bridge.call}));
vi.mock('./Canvas',()=>({Canvas:({onSelect,onIssues}:{onSelect:(id:string,additive:boolean)=>void;onIssues:(issues:unknown[])=>void})=><><button onClick={()=>onSelect('headline',false)}>select headline</button><button onClick={()=>onIssues([])}>successful preview</button></>}));
vi.mock('./Inspector',()=>({Inspector:({edit,resolveFont,node}:{edit:(command:object)=>void;resolveFont:(id:string)=>void;node?:Node})=><><button onClick={()=>edit({op:'setTitle',title:'updated'})}>change title</button><button onClick={()=>resolveFont('font-main')}>resolve exact font</button>{node?.type==='text' && <><output>{`resolved text ${node.text.fontSize} / ${node.text.lineHeight}`}</output><button onClick={()=>edit({op:'setTextStyle',nodeId:node.id,style:{fontSize:70,allowedFontSizes:[70,64,60]}})}>change text size</button><button onClick={()=>edit({op:'setTextStyle',nodeId:node.id,style:{lineHeight:112}})}>change text line height</button></>}</>}));
vi.mock('./BatchPanel',()=>({BatchPanel:()=>null}));
vi.mock('./SettingsDialog',()=>({SettingsDialog:({close}:{close:()=>void})=><section role="dialog"><button onClick={close}>close settings</button></section>}));
import { App } from './App';

function view(revision=10):SessionView { return {sessionId:'session-test',document:{...structuredClone(fixture) as DesignDocument,revision},mode:'authoring',canUndo:true,canRedo:false,issues:[]}; }
beforeEach(()=>{bridge.call.mockReset();bridge.call.mockImplementation(async(command:string)=>{if(command==='recover_project'||command==='read_session'||command==='apply_edit'||command==='resolve_font')return view();if(command==='save_project')return {saved:true};return null;});});
afterEach(cleanup);

describe('desktop operation boundaries',()=>{
  it('shows damaged recovery and leaves it intact until a deliberate open or new action',async()=>{
    bridge.call.mockImplementation(async(command:string)=>{if(command==='recover_project')throw [{severity:'error',code:'E_SAVE_FAILED',message:'invalid recovery workspace reference',location:{},remedy:'Open a saved project.'}];return view();});
    render(<App />);
    await screen.findByText('前回の作品を復旧できませんでした。「新規」または「開く」から制作を始めてください。');
    expect(bridge.call.mock.calls.some(([command])=>command==='open_fixture')).toBe(false);
    expect(screen.getByRole('button',{name:'新規'})).toBeDefined();expect(screen.getByRole('button',{name:'開く'})).toBeDefined();
  });
  it('resynchronizes a failed operation before sending the next expected revision',async()=>{
    let applyCount=0;
    bridge.call.mockImplementation(async(command:string)=>{if(command==='recover_project')return view(10);if(command==='read_session')return view(11);if(command==='apply_edit'){if(++applyCount===1)throw [{severity:'error',code:'E_SAVE_FAILED',message:'recovery persistence failed',location:{},remedy:'Save as.'}];return view(12);}return null;});
    render(<App />);await screen.findByText('change title');
    fireEvent.click(screen.getByText('change title'));await waitFor(()=>expect(bridge.call.mock.calls.some(([command])=>command==='read_session')).toBe(true));
    await waitFor(()=>expect(screen.getByText('revision 11 · sRGB')).toBeDefined());
    fireEvent.click(screen.getByText('successful preview'));expect(screen.getByText('E_SAVE_FAILED')).toBeDefined();
    fireEvent.click(screen.getByText('change title'));await waitFor(()=>expect(applyCount).toBe(2));
    const applyCalls=bridge.call.mock.calls.filter(([command])=>command==='apply_edit');expect(applyCalls[1][1].expectedRevision).toBe(11);
  });
  it('distinguishes Save As and wires the exact-font picker',async()=>{
    render(<App />);await screen.findByText('resolve exact font');
    fireEvent.click(screen.getByText('resolve exact font'));await waitFor(()=>expect(bridge.call.mock.calls.some(([command,args])=>command==='resolve_font'&&args.fontId==='font-main')).toBe(true));
    await waitFor(()=>expect((screen.getByRole('button',{name:'別名で保存'}) as HTMLButtonElement).disabled).toBe(false));
    fireEvent.keyDown(window,{key:'S',metaKey:true,shiftKey:true});await waitFor(()=>expect(bridge.call.mock.calls.some(([command,args])=>command==='save_project'&&args.saveAs===true)).toBe(true));
  });
  it('guards shortcuts during IME, field editing, dialogs and pending operations',async()=>{
    render(<App />);await screen.findByText('change title');
    fireEvent.compositionStart(window);fireEvent.keyDown(window,{key:'s',metaKey:true});fireEvent.compositionEnd(window);
    const field=document.createElement('input');document.body.appendChild(field);fireEvent.keyDown(field,{key:'s',metaKey:true});field.remove();
    fireEvent.click(screen.getByRole('button',{name:'保存領域と復旧'}));fireEvent.keyDown(window,{key:'s',metaKey:true});
    expect(bridge.call.mock.calls.some(([command])=>command==='save_project')).toBe(false);
    fireEvent.keyDown(window,{key:'Escape'});expect(screen.queryByRole('dialog')).toBeNull();
    let finish:(result:unknown)=>void=()=>{};
    bridge.call.mockImplementation((command:string)=>command==='save_project'?new Promise(resolve=>{finish=resolve;}):Promise.resolve(view()));
    fireEvent.keyDown(window,{key:'s',metaKey:true});await waitFor(()=>expect(bridge.call.mock.calls.filter(([command])=>command==='save_project')).toHaveLength(1));
    fireEvent.keyDown(window,{key:'s',metaKey:true});expect(bridge.call.mock.calls.filter(([command])=>command==='save_project')).toHaveLength(1);finish(null);
  });
  it('duplicates the selection and nudges in the active layout',async()=>{
    render(<App />);await screen.findByText('select headline');fireEvent.click(screen.getByText('select headline'));
    fireEvent.keyDown(window,{key:'d',metaKey:true});await waitFor(()=>expect(bridge.call.mock.calls.some(([command,args])=>command==='apply_edit'&&args.command.op==='batch'&&args.command.commands.some((c:{op:string})=>c.op==='addNode'))).toBe(true));
    await waitFor(()=>expect(screen.queryByText('処理中…')).toBeNull());
    fireEvent.click(screen.getByText('select headline'));fireEvent.keyDown(window,{key:'ArrowRight',shiftKey:true});
    await waitFor(()=>expect(bridge.call.mock.calls.filter(([command])=>command==='apply_edit')).toHaveLength(2));
  });
  it('updates active size overrides instead of leaving edited text behind its old patch',async()=>{
    let current=view(10);
    const original=current.document.variants.find(v=>v.id==='portrait')!.nodePatches.find(p=>p.nodeId==='headline')!;
    bridge.call.mockImplementation(async(command:string,args?:{command?:{op:string;commands?:{op:string;nodeId?:string;style?:object;variant?:DesignDocument['variants'][number]}[]}})=>{
      if(command==='recover_project')return current;
      if(command==='apply_edit'){
        const next=structuredClone(current);
        next.document.variants=next.document.variants.map(v=>({...v,approvalStatus:'draft'}));
        for(const edit of args!.command!.commands ?? []){
          if(edit.op==='setTextStyle'){const node=next.document.nodes.find(n=>n.id===edit.nodeId)!;if(node.type==='text')Object.assign(node.text,edit.style);}
          if(edit.op==='setVariant')next.document.variants=next.document.variants.map(v=>v.id===edit.variant!.id?edit.variant!:v);
        }
        next.document.revision++;current=next;return next;
      }
      return null;
    });
    render(<App />);await screen.findByText('select headline');fireEvent.click(screen.getByText('select headline'));
    expect(screen.getByText('resolved text 60 / 80')).toBeDefined();
    fireEvent.click(screen.getByText('change text size'));await screen.findByText('resolved text 70 / 80');
    let sent=bridge.call.mock.calls.filter(([command])=>command==='apply_edit').at(-1)![1].command;
    expect(sent.op).toBe('batch');
    expect(sent.commands[0].style.allowedFontSizes).toContain(70);
    expect(sent.commands[1].variant.id).toBe('portrait');
    expect(sent.commands[1].variant.nodePatches.find((p:{nodeId:string})=>p.nodeId==='headline')).toEqual({...original,fontSize:70});
    fireEvent.click(screen.getByText('change text line height'));await screen.findByText('resolved text 70 / 112');
    sent=bridge.call.mock.calls.filter(([command])=>command==='apply_edit').at(-1)![1].command;
    expect(sent.commands[1].variant.nodePatches.find((p:{nodeId:string})=>p.nodeId==='headline')).toEqual({...original,fontSize:70,lineHeight:112});
    expect(current.document.variants.find(v=>v.id==='square')!.nodePatches).toEqual(fixture.variants.find(v=>v.id==='square')!.nodePatches);
  });
});
