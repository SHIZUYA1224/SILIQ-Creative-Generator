import { describe,expect,it } from 'vitest';
import template from '../../../fixtures/public-safe/documents/basic.json';
import type { DesignDocument,Node } from './api';
import { duplicateSelection } from './duplicate';

function doc() { return structuredClone(template) as DesignDocument; }
function duplicate(document:DesignDocument,selection:string[]) { let count=0;return duplicateSelection(document,selection,()=>`copy-${++count}`)!; }
function commands(result:ReturnType<typeof duplicate>) { return result.command.commands as {op:string;node?:Node;nodeId?:string;parentId?:string;style?:NonNullable<DesignDocument['nodeStyles']>[number];variant?:DesignDocument['variants'][number];locked?:boolean}[]; }

describe('appearance-preserving duplication',()=>{
  it('copies nested groups under the same parent without duplicating selected descendants',()=>{
    const document=doc(), before=JSON.stringify(document), source=document.nodes.find(n=>n.id==='accent-group')!;
    const result=duplicate(document,['accent-group','accent']);
    const adds=commands(result).filter(c=>c.op==='addNode');
    expect(result.selected).toEqual(['copy-1']);expect(adds).toHaveLength(2);
    expect(adds[0].parentId).toBe('root');expect(adds[0].node?.bounds.x).toBe(source.bounds.x+20);
    expect(adds[1].parentId).toBe('copy-1');expect(adds[1].node?.bounds).toEqual(document.nodes.find(n=>n.id==='accent')!.bounds);
    expect(JSON.stringify(document)).toBe(before);
  });
  it('keeps locked descendants and remaps style references only within the cloned subtree',()=>{
    const document=doc();document.nodes.find(n=>n.id==='accent')!.locked=true;
    document.nodeStyles=[{nodeId:'accent',blendMode:'multiply',stroke:{color:'#abcdef',width:2},protectedFrom:['accent-group','photo']}];
    const list=commands(duplicate(document,['accent-group']));
    const style=list.find(c=>c.op==='setNodeStyle'&&c.nodeId==='copy-2')!.style!;
    expect(style.blendMode).toBe('multiply');expect(style.protectedFrom).toEqual(['copy-1','photo']);
    expect(list.at(-1)).toEqual({op:'setLocked',nodeId:'copy-2',locked:true});
    expect(list.find(c=>c.op==='addNode'&&c.node?.id==='copy-2')?.node?.locked).toBe(false);
  });
  it('preserves all explicit size patches and fit choices with only a top-level offset',()=>{
    const document=doc(), result=duplicate(document,['headline']);
    const variants=commands(result).filter(c=>c.op==='setVariant');
    const original=document.variants.find(v=>v.nodePatches.some(p=>p.nodeId==='headline'))!;
    const next=variants.find(c=>c.variant?.id===original.id)!.variant!;
    const old=original.nodePatches.find(p=>p.nodeId==='headline')!,copy=next.nodePatches.find(p=>p.nodeId==='copy-1')!;
    expect(copy.fontSize).toBe(old.fontSize);expect(copy.bounds).toEqual({...old.bounds,x:old.bounds.x+20,y:old.bounds.y+20});
    expect(next.nodePatches).toHaveLength(original.nodePatches.length+1);
  });
  it('translates group content and conjugates an existing leaf affine',()=>{
    const document=doc();document.nodeStyles=[{nodeId:'accent-group',affine:[2,0,0,2,5,6]},{nodeId:'photo',affine:[2,0,0,3,5,6]}];
    expect(commands(duplicate(document,['accent-group'])).find(c=>c.nodeId==='copy-1')!.style!.affine).toEqual([2,0,0,2,25,26]);
    expect(commands(duplicate(document,['photo'])).find(c=>c.nodeId==='copy-1')!.style!.affine).toEqual([2,0,0,3,-15,-34]);
  });
  it('refuses root-only copies and ID collisions',()=>{
    expect(duplicateSelection(doc(),['root'])).toBeNull();
    expect(()=>duplicateSelection(doc(),['headline'],()=> 'root')).toThrow();
  });
  it('moves a copied leaf clip and mask along with its bounds',()=>{
    const document=doc();document.nodeStyles=[{nodeId:'photo',clip:{data:'M0 0H40V40Z',fillRule:'non-zero'},mask:{data:'M0 0L40 0L0 40Z',fillRule:'even-odd',opacity:.5,invert:true}}];
    const style=commands(duplicate(document,['photo'])).find(c=>c.nodeId==='copy-1')!.style!;
    expect(style.clip?.data).toBe('M20 20 L60 20 L60 60 Z');
    expect(style.mask?.data).toBe('M20 20 L60 20 L20 60 Z');
    expect(style.mask?.opacity).toBe(.5);expect(style.mask?.invert).toBe(true);
    expect(document.nodeStyles[0].clip?.data).toBe('M0 0H40V40Z');
  });
});
