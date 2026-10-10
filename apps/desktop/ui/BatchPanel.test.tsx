import { cleanup,fireEvent,render,screen,waitFor } from '@testing-library/react';
import { afterEach,beforeEach,describe,expect,it,vi } from 'vitest';
import fixture from '../../../fixtures/public-safe/documents/basic.json';
import type { DesignDocument,SessionView } from './api';

const bridge=vi.hoisted(()=>({call:vi.fn()}));
vi.mock('./api',async()=>({...await vi.importActual<typeof import('./api')>('./api'),call:bridge.call}));
import { BatchPanel } from './BatchPanel';

function session(id:string,variantIds:string[],slotId:string):SessionView {
  const document=structuredClone(fixture) as DesignDocument;
  document.variants=variantIds.map(variantId=>({...document.variants[0],id:variantId,label:variantId,approvalStatus:'approved',nodePatches:[]}));
  document.slots=[{...document.slots[0],id:slotId}];
  return {sessionId:id,document,mode:'authoring',canUndo:false,canRedo:false,issues:[]};
}
const oldSession=session('session-old',['old-square','old-portrait'],'old-slot');
const newSession=session('session-new',['new-square'],'new-slot');
const close=vi.fn(),onIssues=vi.fn();
function plan(variants:string[]){return {planId:'plan-old',items:[],rows:1,total:variants.length,variantIds:variants,previewNames:[],issues:[]};}
const runningJob={jobId:'job-old',status:'running',total:2,completed:0,issues:[]};

beforeEach(()=>{
  bridge.call.mockReset();onIssues.mockReset();close.mockReset();
  bridge.call.mockImplementation(async(command:string,args?:Record<string,unknown>)=>{
    if(command==='list_saved_batches')return [];
    if(command==='read_session')return args?.sessionId==='session-old'?oldSession:newSession;
    if(command==='select_dataset'){
      const old=args?.sessionId==='session-old',slot=old?'old-slot':'new-slot';
      return {datasetId:old?'dataset-old':'dataset-new',columns:['rowId',slot],rowCount:1,sampleRows:[{rowId:'001',[slot]:'text'}]};
    }
    if(command==='preflight_batch')return plan(args!.variantIds as string[]);
    if(command==='start_batch'||command==='batch_status')return runningJob;
    if(command==='cancel_batch')return {...runningJob,status:'cancelled'};
    return null;
  });
});
afterEach(cleanup);
async function importAndInspect(){fireEvent.click(screen.getByRole('button',{name:'CSV / JSON を取り込む'}));await screen.findByLabelText('old-slotの対応先');fireEvent.click(screen.getByRole('button',{name:'全件検査・出力先を選択'}));await screen.findByRole('button',{name:'2 件を書き出す'});}

describe('batch session boundaries',()=>{
  it('resets data, mapping, candidate plan and hidden old variant IDs on a project switch',async()=>{
    const {rerender}=render(<BatchPanel visible session={oldSession} close={close} onIssues={onIssues}/>);
    await importAndInspect();
    rerender(<BatchPanel visible session={newSession} close={close} onIssues={onIssues}/>);
    await waitFor(()=>expect(screen.queryByLabelText('old-slotの対応先')).toBeNull());
    expect(screen.queryByRole('button',{name:'2 件を書き出す'})).toBeNull();
    expect((screen.getByRole('checkbox',{name:'new-square'}) as HTMLInputElement).checked).toBe(true);
    expect((screen.getByRole('button',{name:'全件検査・出力先を選択'}) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(screen.getByRole('button',{name:'CSV / JSON を取り込む'}));await screen.findByLabelText('new-slotの対応先');
    fireEvent.click(screen.getByRole('button',{name:'全件検査・出力先を選択'}));
    await waitFor(()=>expect(bridge.call.mock.calls.filter(([command])=>command==='preflight_batch')).toHaveLength(2));
    const args=bridge.call.mock.calls.filter(([command])=>command==='preflight_batch').at(-1)![1];
    expect(args.sessionId).toBe('session-new');expect(args.datasetId).toBe('dataset-new');expect(args.variantIds).toEqual(['new-square']);
    expect(args.mapping).toEqual({rowId:'rowId','new-slot':'new-slot'});
  });
  it('keeps the running frozen job controllable while resetting the editable project inputs',async()=>{
    const {rerender}=render(<BatchPanel visible session={oldSession} close={close} onIssues={onIssues}/>);
    await importAndInspect();fireEvent.click(screen.getByRole('button',{name:'2 件を書き出す'}));await screen.findByText('書き出し中');
    rerender(<BatchPanel visible session={newSession} close={close} onIssues={onIssues}/>);
    await waitFor(()=>expect(screen.queryByLabelText('old-slotの対応先')).toBeNull());
    expect(screen.getByText('書き出し中')).toBeDefined();
    fireEvent.click(screen.getByRole('button',{name:'安全に停止する'}));await screen.findByText('停止しました');
    expect(bridge.call.mock.calls.find(([command])=>command==='cancel_batch')![1]).toEqual({jobId:'job-old'});
    expect(onIssues).not.toHaveBeenCalled();
  });
});
