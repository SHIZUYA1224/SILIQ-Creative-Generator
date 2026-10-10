import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import { ArrowDownToLine, Check, ChevronDown, ChevronRight, ChevronUp, Copy, Eye, EyeOff, FilePlus2, FolderOpen, Grid2X2, ImagePlus, Layers, LockKeyhole, MousePointer2, PanelLeft, PenTool, Redo2, Save, Settings, ShieldCheck, Square, Trash2, Type, Undo2, UnlockKeyhole, ZoomIn, ZoomOut } from 'lucide-react';
import { Canvas } from './Canvas';
import { BatchPanel } from './BatchPanel';
import { SettingsDialog } from './SettingsDialog';
import { Inspector } from './Inspector';
import { call, issuesFrom, nativeAvailable, transactionId, type Bounds, type EditCommand, type Issue, type SessionView } from './api';
import { alignedNodes, ancestorIds, groupAffine, isEffectivelyLocked, layerTree, localDrag, resolvedNodes, resolvedStyles } from './geometry';
import { duplicateSelection } from './duplicate';
import { useDialogFocus } from './dialog-focus';
import { IssueDetails } from './IssueDetails';

function ToolButton({ label, children, active, disabled, onClick }: { label: string; children: ReactNode; active?: boolean; disabled?: boolean; onClick: () => void }) {
  return <button className={`tool-button ${active ? 'active' : ''}`} title={label} aria-label={label} disabled={disabled} onClick={onClick}>{children}</button>;
}

export function App() {
  const [session, setSession] = useState<SessionView>();
  const [selected, setSelected] = useState<string[]>([]);
  const [variantId, setVariantId] = useState('portrait');
  const [issues, setIssues] = useState<Issue[]>([]);
  const [previewIssues,setPreviewIssues]=useState<Issue[]>([]);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState('');
  const [zoom, setZoom] = useState(1);
  const [grid, setGrid] = useState(false);
  const [safe, setSafe] = useState(false);
  const [snap, setSnap] = useState(true);
  const [showTemplates, setShowTemplates] = useState(false);
  const [showExport, setShowExport] = useState(false);
  const [showBatch, setShowBatch] = useState(false);
  const [showSettings, setShowSettings] = useState(false);
  const [savedRevision, setSavedRevision] = useState<number>();
  const latest = useRef(session); latest.current = session;
  const operationQueue = useRef(Promise.resolve());
  const pendingCount = useRef(0);
  const composing = useRef(false);
  useDialogFocus(showTemplates || showExport || showBatch || showSettings);

  const accept = useCallback((view: SessionView, reset = false) => {
    const previous = latest.current;
    if (previous && previous.sessionId!==view.sessionId) void call('close_session',{sessionId:previous.sessionId}).catch(()=>undefined);
    latest.current = view; setSession(view); setIssues(view.issues ?? []);
    if(previous?.sessionId!==view.sessionId || previous?.document.revision!==view.document.revision)setPreviewIssues([]);
    setVariantId(current => view.document.variants.some(v => v.id === current) ? current : view.document.variants[0].id);
    setSelected(current => reset ? [] : current.filter(id => view.document.nodes.some(n => n.id === id)));
  }, []);
  const perform = useCallback(async (operation: () => Promise<unknown>, success?: string) => {
    pendingCount.current++; setBusy(true);
    const next = operationQueue.current.then(async () => {
      try { const result=await operation(); if (success && result!==null) setMessage(success); }
      catch (e) {
        const failure = issuesFrom(e);
        const current = latest.current;
        if (current) {
          try { accept(await call<SessionView>('read_session', { sessionId: current.sessionId })); }
          catch (syncError) { failure.push(...issuesFrom(syncError)); }
        }
        setIssues(failure);
      }
      finally { pendingCount.current--; setBusy(pendingCount.current > 0); }
    });
    operationQueue.current = next;
    await next;
  }, [accept]);
  const edit = useCallback((command: EditCommand) => {
    void perform(async () => {
      const current = latest.current; if (!current) return;
      let activeCommand=command;
      if(command.op==='setTextStyle' && command.style && typeof command.style==='object') {
        const style=command.style as Record<string,unknown>;
        const variant=current.document.variants.find(v=>v.id===variantId);
        const existing=variant?.nodePatches.find(p=>p.nodeId===command.nodeId);
        if(variant && existing && (style.fontSize!==undefined || style.lineHeight!==undefined)) {
          activeCommand={op:'batch',commands:[command,{op:'setVariant',variant:{...variant,approvalStatus:'draft',nodePatches:variant.nodePatches.map(p=>p.nodeId===command.nodeId ? {...p,...(style.fontSize===undefined?{}:{fontSize:style.fontSize}),...(style.lineHeight===undefined?{}:{lineHeight:style.lineHeight})} : p)}}]};
        }
      }
      accept(await call<SessionView>('apply_edit', { sessionId: current.sessionId, expectedRevision: current.document.revision, transactionId: transactionId(), command:activeCommand }));
    });
  }, [accept, perform,variantId]);
  const openFixture = (fixture: string) => void perform(async () => { accept(await call<SessionView>('open_fixture', { fixture }), true); setShowTemplates(false); setSavedRevision(undefined); }, 'テンプレートを開きました');
  useEffect(() => { if (nativeAvailable) void perform(async () => {
    try { accept(await call<SessionView>('recover_project'), true); setMessage('前回の確定した編集を復旧しました'); }
    catch (error) {
      const failures = issuesFrom(error);
      if (failures.length === 1 && failures[0].message === 'no recoverable document has been committed') {
        accept(await call<SessionView>('open_fixture', { fixture: 'retail-promo-template' }), true); setShowTemplates(true);
      } else throw error;
    }
  }); }, [accept, perform]);
  useEffect(() => { if (message) { const timer = window.setTimeout(() => setMessage(''), 4500); return () => window.clearTimeout(timer); } }, [message]);

  const save = useCallback((saveAs = false) => void perform(async () => {
    const current = latest.current; if (!current) return;
    const result = await call<unknown>('save_project', { sessionId: current.sessionId, expectedRevision: current.document.revision, saveAs });
    if (result !== null) { setSavedRevision(current.document.revision); setMessage('プロジェクトを保存しました'); }
  }), [perform]);
  const history = useCallback((direction: 'undo' | 'redo') => void perform(async () => {
    const current = latest.current; if (!current) return;
    accept(await call<SessionView>(`${direction}_edit`, { sessionId: current.sessionId, expectedRevision: current.document.revision, transactionId: transactionId() }));
  }), [accept, perform]);
  const newDocument = useCallback(() => void perform(async () => { accept(await call<SessionView>('new_document'), true); setSavedRevision(undefined); }), [accept, perform]);
  const openProject = useCallback(() => void perform(async () => { const result = await call<SessionView | null>('open_project'); if (result) { accept(result, true); setSavedRevision(result.document.revision); } }), [accept, perform]);

  const allIssues=[...issues,...previewIssues].filter((issue,index,list)=>list.findIndex(other=>other.code===issue.code && other.message===issue.message && JSON.stringify(other.location)===JSON.stringify(issue.location))===index);
  const doc = session?.document;
  const nodes = doc ? resolvedNodes(doc, variantId) : [];
  const node = nodes.find(n => n.id === selected[0]);
  const authoring = session?.mode === 'authoring';
  const mutateBounds = (changes: { id: string; bounds: Bounds }[]) => {
    if (!doc) return;
    const variant = doc.variants.find(v => v.id === variantId)!;
    if (variant.nodePatches.length) {
      const patches = new Map(variant.nodePatches.map(p => [p.nodeId, p]));
      for (const change of changes) {
        const original=nodes.find(node=>node.id===change.id)!;
        const affine=original.type==='group' ? groupAffine(original.bounds,change.bounds,resolvedStyles(doc,variantId).find(style=>style.nodeId===change.id)?.affine) : undefined;
        patches.set(change.id, { ...patches.get(change.id), nodeId: change.id, bounds: change.bounds, ...(affine ? {affine} : {}) });
      }
      edit({ op: 'setVariant', variant: { ...variant, approvalStatus: 'draft', nodePatches: [...patches.values()] } });
    } else edit({ op: 'batch', commands: changes.map(c => ({ op: 'setBounds', nodeId: c.id, bounds: c.bounds })) });
  };
  const addNode = (type: 'text' | 'rect' | 'path') => {
    if (!doc) return;
    const id = `${type}-${crypto.randomUUID().slice(0, 8)}`;
    const root = doc.nodes.find(n => n.id === doc.baseArtboard.rootNodeId)!;
    const base = { id, name: type === 'text' ? 'テキスト' : type === 'path' ? 'ベジェパス' : '図形', type, bounds: { x: 80, y: 100, width: 500, height: type === 'text' ? 100 : 240 }, rotationDeg: 0, opacity: 1, visible: true, locked: false };
    const data = type === 'path' ? { ...base, bounds: { x: 80, y: 100, width: 500, height: 240 }, data: 'M 0 180 C 100 20 300 20 500 180 L 500 240 L 0 240 Z', viewBox: { x: 0, y: 0, width: 500, height: 240 }, fill: { kind: 'solid', color: '#39735B' }, fillRule: 'non-zero' } : type === 'rect' ? { ...base, fill: { kind: 'solid', color: '#39735B' }, cornerRadius: 16 } : {
      ...base, text: { value: 'ここにテキストを入力', fontId: doc.fonts[0]?.id, fontSize: 40, lineHeight: 52, letterSpacing: 0, maxLines: 2, overflowPolicy: 'error', allowedFontSizes: [], language: 'ja', align: 'left', kinsoku: 'strict-japanese' }, fill: { kind: 'solid', color: '#202723' },
    };
    const add = { op: 'addNode', parentId: root.id, index: root.type === 'group' ? root.childIds.length : 0, node: data };
    edit(type === 'text' ? { op:'batch',commands:[add,{op:'setTextStyle',nodeId:id,style:{kinsoku:'strict-japanese'}}] } : add);
    setSelected([id]);
  };
  const duplicate = useCallback(() => void perform(async () => {
    const current = latest.current; if (!current || current.mode !== 'authoring') return;
    const copies = duplicateSelection(current.document, selected); if (!copies) return;
    accept(await call<SessionView>('apply_edit', { sessionId:current.sessionId,expectedRevision:current.document.revision,transactionId:transactionId(),command:copies.command }));
    setSelected(copies.selected);
  }), [accept, perform, selected]);
  const groupSelection = () => { if (selected.length>=2) edit({op:'groupNodes',nodeIds:selected,groupId:`group-${crypto.randomUUID().slice(0,8)}`,name:'グループ'}); };
  const ungroupSelection = () => { if(node?.type==='group') edit({op:'ungroupNodes',groupId:node.id}); };
  const reorder = (offset:number) => { if(!doc || !node)return; const parent=doc.nodes.find(n=>n.type==='group'&&n.childIds.includes(node.id));if(parent?.type!=='group')return;edit({op:'reorder',nodeId:node.id,index:Math.max(0,Math.min(parent.childIds.length-1,parent.childIds.indexOf(node.id)+offset))}); };
  const deleteSelection = useCallback(() => { if (selected.length) edit({ op: 'batch', commands: selected.filter(id=>!ancestorIds(latest.current?.document.nodes ?? [],id).some(parent=>selected.includes(parent))).map(nodeId => ({ op: 'deleteNode', nodeId })) }); }, [edit, selected]);
  const importAsset = (kind: 'raster' | 'svg' | 'font', addToCanvas = false) => void perform(async () => {
    const current = latest.current; if (!current) return;
    const result = await call<SessionView | null>('import_asset', { sessionId: current.sessionId, expectedRevision: current.document.revision, transactionId: transactionId(), kind });
    if (result) {
      accept(result);
      if (addToCanvas && kind !== 'font') {
        const asset = result.document.assets.at(-1); if (!asset) return;
        const id = `media-${crypto.randomUUID().slice(0,8)}`;
        const root = result.document.nodes.find(n => n.id === result.document.baseArtboard.rootNodeId)!;
        const common = { id, name: kind === 'svg' ? 'ロゴ' : '画像', bounds: { x: 80, y: 100, width: 500, height: 340 }, rotationDeg: 0, opacity: 1, visible: true, locked: false, assetId: asset.id };
        const node = kind === 'svg' ? { ...common, type: 'logo', fit: 'contain', preserveAspectRatio: true, minWidth: 1, clearSpace: 0 } : { ...common, type: 'image', fit: 'contain', focalPoint: { x:.5,y:.5 }, clipToBounds: true };
        const view = await call<SessionView>('apply_edit', { sessionId: result.sessionId, expectedRevision: result.document.revision, transactionId: transactionId(), command: { op: 'addNode', parentId: root.id, index: root.type === 'group' ? root.childIds.length : 0, node } });
        accept(view); setSelected([id]);
      }
    }
  });
  const resolveFont = (fontId: string) => void perform(async () => {
    const current = latest.current; if (!current) return;
    const result = await call<SessionView | null>('resolve_font', { sessionId:current.sessionId,expectedRevision:current.document.revision,transactionId:transactionId(),fontId });
    if (result) accept(result);
  });
  useEffect(() => {
    const compositionStart = () => { composing.current = true; };
    const compositionEnd = () => { composing.current = false; };
    const keydown = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement;
      if (target.closest?.('input,textarea,select,[contenteditable="true"]') || target.isContentEditable || e.isComposing || composing.current || e.keyCode === 229) return;
      if (busy || pendingCount.current > 0) return;
      if (showTemplates || showExport || showBatch || showSettings) {
        if (e.key === 'Escape') { e.preventDefault(); setShowExport(false); setShowTemplates(false); setShowBatch(false); setShowSettings(false); }
        return;
      }
      if (e.key === '+' || e.key === '=' || e.key === '-') {
        e.preventDefault(); setZoom(value => Math.max(.25,Math.min(3,value+(e.key==='-' ? -.1 : .1)))); return;
      }
      if (e.metaKey || e.ctrlKey) {
        const key = e.key.toLowerCase();
        if (key === 's') { e.preventDefault(); save(e.shiftKey); }
        else if (key === 'z') { e.preventDefault(); if (e.shiftKey ? session?.canRedo : session?.canUndo) history(e.shiftKey ? 'redo' : 'undo'); }
        else if (key === 'n') { e.preventDefault(); newDocument(); }
        else if (key === 'o') { e.preventDefault(); openProject(); }
        else if (key === 'e' && session) { e.preventDefault(); setShowExport(true); }
        else if (key === 'd' && authoring) { e.preventDefault(); duplicate(); }
        else if (key === 'i' && authoring) { e.preventDefault(); importAsset('raster',true); }
      } else if (e.key === 'Delete' || e.key === 'Backspace') { e.preventDefault(); if (authoring) deleteSelection(); }
      else if (e.key.startsWith('Arrow') && authoring && doc) {
        const step=e.shiftKey ? 10 : 1;
        const dx=e.key==='ArrowLeft' ? -step : e.key==='ArrowRight' ? step : 0;
        const dy=e.key==='ArrowUp' ? -step : e.key==='ArrowDown' ? step : 0;
        const styles=resolvedStyles(doc,variantId);
        const chosen=nodes.filter(n=>selected.includes(n.id) && n.id!==doc.baseArtboard.rootNodeId && !isEffectivelyLocked(nodes,n) && !ancestorIds(nodes,n.id).some(parent=>selected.includes(parent)));
        if (chosen.length) { e.preventDefault(); mutateBounds(chosen.map(n=>{const delta=localDrag(nodes,n,dx,dy,styles);return {id:n.id,bounds:{...n.bounds,x:n.bounds.x+delta.dx,y:n.bounds.y+delta.dy}};})); }
      }
      else if (e.key === 'Escape') { setSelected([]); }
    };
    window.addEventListener('compositionstart',compositionStart);window.addEventListener('compositionend',compositionEnd);
    window.addEventListener('keydown', keydown); return () => { window.removeEventListener('keydown', keydown);window.removeEventListener('compositionstart',compositionStart);window.removeEventListener('compositionend',compositionEnd); };
  }, [authoring, busy, deleteSelection, doc, duplicate, history, importAsset, mutateBounds, newDocument, nodes, openProject, save, selected, session, showBatch, showExport, showSettings, showTemplates, variantId]);

  if (!nativeAvailable) return <div className="browser-message"><div className="brand-mark">S</div><h1>SILIQ Creative Generator</h1><p>広告、SNS、ブログの図解を、ひとつの編集アプリで。</p><div className="browser-notice">このアプリは完全オフラインのデスクトップアプリです。<br />Mac 用アプリを起動して制作を始めてください。</div></div>;

  return <div className="app-shell">
    <header className="app-header"><div className="brand"><span className="brand-mark">S</span><strong>SILIQ<span>CREATIVE GENERATOR</span></strong></div><div className="header-divider" /><div className="project-heading"><strong>{doc?.title ?? '起動しています'}</strong><span>{savedRevision === doc?.revision ? '保存済み' : '未保存の変更'} · ローカルプロジェクト</span></div><div className="header-actions"><ToolButton label="保存領域と復旧" onClick={()=>setShowSettings(true)}><Settings size={16} /></ToolButton><span className="offline-badge"><ShieldCheck size={14} />完全オフライン</span><button className="button secondary" disabled={!session || busy} onClick={() => save()}><Save size={15} />保存</button><button className="button secondary" disabled={!session || busy} onClick={() => save(true)} title="別名で保存 ⇧⌘S">別名で保存</button><button className="button primary" disabled={!session || busy} onClick={() => setShowExport(true)}><ArrowDownToLine size={15} />書き出す</button></div></header>
    <nav className="workspace-bar" aria-label="ワークスペース"><div className="workspace-tabs"><button className={!showBatch ? 'active' : ''} onClick={() => setShowBatch(false)}><Layers size={15} />デザイン制作</button><button className={showBatch ? 'active' : ''} disabled={!session} onClick={() => setShowBatch(true)}><Grid2X2 size={15} />データから量産</button></div><div className="workspace-tools"><button onClick={() => setShowTemplates(true)}><PanelLeft size={14} />テンプレート</button><button disabled={busy} onClick={newDocument}><FilePlus2 size={14} />新規</button><button disabled={busy} onClick={openProject}><FolderOpen size={14} />開く</button></div></nav>
    <div className="workbench">
      <aside className="left-panel"><div className="panel-heading"><h2>アートボード</h2><span>{doc?.variants.length ?? 0}</span></div><div className="artboard-list">{doc?.variants.map(v => <button key={v.id} className={variantId === v.id ? 'active' : ''} onClick={() => setVariantId(v.id)}><Square size={16} /><span>{v.label}<small>{v.width} × {v.height}</small></span><span className={`approval-dot ${v.approvalStatus}`} title={v.approvalStatus === 'approved' ? '承認済み' : '未承認'} /></button>)}</div><div className="panel-heading layer-heading"><h2>レイヤー</h2><Layers size={14} /></div><div className="layer-actions"><ToolButton label="グループ化" disabled={!authoring || busy || selected.length<2} onClick={groupSelection}><Layers size={15} /></ToolButton><ToolButton label="グループを解除" disabled={!authoring || busy || node?.type!=='group'} onClick={ungroupSelection}><Grid2X2 size={15} /></ToolButton><ToolButton label="前面へ" disabled={!authoring || busy || !node} onClick={()=>reorder(1)}><ChevronUp size={15} /></ToolButton><ToolButton label="背面へ" disabled={!authoring || busy || !node} onClick={()=>reorder(-1)}><ChevronDown size={15} /></ToolButton></div><div className="layer-list">{doc && layerTree(nodes,doc.baseArtboard.rootNodeId).map(({node:n,depth}) => <div className={`layer-row ${selected.includes(n.id) ? 'active' : ''}`} key={n.id}><button className="layer-select" style={{paddingLeft:8+depth*12}} onClick={e => setSelected(current => e.metaKey || e.shiftKey ? current.includes(n.id) ? current.filter(id => id !== n.id) : [...current, n.id] : [n.id])}>{n.type === 'text' ? <Type size={14} /> : n.type === 'group' ? <ChevronDown size={14} /> : n.type === 'image' || n.type === 'logo' ? <ImagePlus size={14} /> : <Square size={14} />}<span>{n.name}</span></button><button aria-label={`${n.name}の表示切替`} disabled={busy || !authoring} onClick={() => edit({ op: 'setVisible', nodeId: n.id, visible: !n.visible })}>{n.visible ? <Eye size={12} /> : <EyeOff size={12} />}</button><button aria-label={`${n.name}のロック切替`} disabled={busy || !authoring} onClick={() => edit({ op: 'setLocked', nodeId: n.id, locked: !n.locked })}>{n.locked ? <LockKeyhole size={12} /> : <UnlockKeyhole size={12} />}</button></div>)}</div><div className="left-panel-footer"><ShieldCheck size={14} /><p>素材・作品はこの Mac 内に保存。<br />外部への送信はありません。</p></div></aside>
      <main className="center-panel"><div className="canvas-toolbar"><div className="tool-group"><ToolButton label="選択ツール" active onClick={() => setSelected([])}><MousePointer2 size={17} /></ToolButton><ToolButton label="テキストを追加" disabled={!authoring || busy || !doc?.fonts.length} onClick={() => addNode('text')}><Type size={18} /></ToolButton><ToolButton label="図形を追加" disabled={!authoring || busy} onClick={() => addNode('rect')}><Square size={17} /></ToolButton><ToolButton label="ベジェパスを追加" disabled={!authoring || busy} onClick={() => addNode('path')}><PenTool size={17} /></ToolButton><ToolButton label="ロゴを取り込む" disabled={!authoring || busy} onClick={() => importAsset('svg', true)}><ImagePlus size={17} /></ToolButton><ToolButton label="画像を取り込む" disabled={!authoring || busy} onClick={() => importAsset('raster', true)}><ImagePlus size={18} /></ToolButton></div><div className="tool-group history-tools"><ToolButton label="元に戻す ⌘Z" disabled={busy || !session?.canUndo} onClick={() => history('undo')}><Undo2 size={16} /></ToolButton><ToolButton label="やり直す ⇧⌘Z" disabled={busy || !session?.canRedo} onClick={() => history('redo')}><Redo2 size={16} /></ToolButton><ToolButton label="複製 ⌘D" disabled={!authoring || busy || !selected.length} onClick={duplicate}><Copy size={15} /></ToolButton><ToolButton label="削除" disabled={!authoring || busy || !selected.length} onClick={deleteSelection}><Trash2 size={15} /></ToolButton></div><div className="canvas-display-controls"><button className={grid ? 'enabled' : ''} onClick={() => setGrid(!grid)}>グリッド</button><button className={safe ? 'enabled' : ''} onClick={() => setSafe(!safe)}>セーフエリア</button><button className={snap ? 'enabled' : ''} onClick={() => setSnap(!snap)}>スナップ</button></div></div>
        {session && doc ? <Canvas sessionId={session.sessionId} document={doc} variantId={variantId} selected={selected} zoom={zoom} grid={grid} safe={safe} snap={snap} disabled={!authoring || busy} onSelect={(id, additive) => setSelected(current => id === null ? [] : additive ? current.includes(id) ? current.filter(v => v !== id) : [...current, id] : [id])} onMove={mutateBounds} onIssues={setPreviewIssues} /> : <div className="loading-screen">{issues.some(issue => issue.severity === 'error') ? '前回の作品を復旧できませんでした。「新規」または「開く」から制作を始めてください。' : '制作エンジンを準備しています…'}</div>}
        <div className="canvas-bottom"><span>{selected.length ? `${selected.length} レイヤーを選択` : 'レイヤーを選択して編集'}</span><div><ToolButton label="縮小" onClick={() => setZoom(v => Math.max(.25, v - .1))}><ZoomOut size={14} /></ToolButton><button onClick={() => setZoom(1)}>{Math.round(zoom * 100)}%</button><ToolButton label="拡大" onClick={() => setZoom(v => Math.min(3, v + .1))}><ZoomIn size={14} /></ToolButton></div></div></main>
      <aside className="right-panel"><div className="panel-heading"><h2>{node ? node.name : 'プロジェクト設定'}</h2>{node && <span>{node.type.toUpperCase()}</span>}</div>{session && <Inspector key={`${node?.id ?? 'project'}-${doc?.revision}-${variantId}`} session={session} node={node} variantId={variantId} disabled={busy} edit={edit} changeBounds={bounds => node && mutateBounds([{ id: node.id, bounds }])} importAsset={importAsset} resolveFont={resolveFont} align={direction => doc && mutateBounds(alignedNodes(nodes,selected,direction,resolvedStyles(doc,variantId)))} selectionCount={selected.length} />}</aside>
    </div>
    <footer className="status-bar"><span className={allIssues.some(i => i.severity === 'error') ? 'status-error' : ''}>{allIssues.length ? `${allIssues.filter(i => i.severity === 'error').length} 件のエラー · ${allIssues.filter(i => i.severity === 'warning').length} 件の注意` : <><Check size={12} />検査エラーなし</>}</span><span>{doc ? `revision ${doc.revision} · sRGB` : '起動中'}</span><span>{busy ? '処理中…' : message || '素材を外部へ送信しません'}</span></footer>
    {allIssues.length > 0 && <div className="issues-panel"><strong>書き出し前に確認してください</strong><button aria-label="検査パネルを閉じる" onClick={() => {setIssues([]);setPreviewIssues([]);}}>×</button>{allIssues.map((issue, i) => <div key={`${issue.code}-${i}`}><code>{issue.code}</code><IssueDetails issue={issue} /></div>)}</div>}
    {showTemplates && <div className="modal-backdrop" onClick={() => setShowTemplates(false)}><section className="modal template-modal" role="dialog" aria-modal="true" aria-label="テンプレート" onClick={e => e.stopPropagation()}><div className="modal-heading"><div><span className="eyebrow">START WITH A TEMPLATE</span><h2>アイデアを、かたちに。</h2><p>広告、SNS、記事の図解。編集できるテンプレートから。</p></div><button className="close-button" aria-label="閉じる" onClick={() => setShowTemplates(false)}>×</button></div><div className="template-grid">{[{ id: 'retail-promo-template', title: '店舗・キャンペーン広告', category: '広告 / SNS', color: '#d64528' }, { id: 'showcase', title: 'ブランド・商品紹介', category: 'SNS / 商品', color: '#285655' }, { id: 'blog-summary', title: '記事の図解・3つのポイント', category: 'ブログ / 図解', color: '#446957' }, { id: 'blog-comparison', title: '比較表・説明資料', category: 'ブログ / PDF', color: '#758a63' }, { id: 'basic', title: '自由なレイアウト', category: '編集の基本', color: '#697d9e' }].map(t => <button className="template-card" key={t.id} onClick={() => openFixture(t.id)}><div className="template-cover" style={{ background: t.color }}><span>SILIQ</span><strong>{t.title}</strong><i /></div><small>{t.category}</small><strong>{t.title}</strong><span>テンプレートを開く <ChevronRight size={14} /></span></button>)}</div></section></div>}
    {showSettings && <SettingsDialog close={()=>setShowSettings(false)} onIssues={setIssues} />}
    {showExport && session && <ExportDialog session={session} variantId={variantId} busy={busy} perform={perform} close={() => setShowExport(false)} />}
    {session && <BatchPanel visible={showBatch} session={session} close={() => setShowBatch(false)} onIssues={setIssues} />}
    {message && <div className="toast" role="status"><Check size={15} />{message}</div>}
  </div>;
}

function ExportDialog({ session, variantId, busy, perform, close }: { session: SessionView; variantId: string; busy: boolean; perform: (op: () => Promise<unknown>, success?: string) => Promise<void>; close: () => void }) {
  const variant = session.document.variants.find(v => v.id === variantId)!;
  const [format, setFormat] = useState('png');
  const [quality, setQuality] = useState(90);
  const [background, setBackground] = useState('#ffffff');
  const [allowRasterization, setAllowRasterization] = useState(false);
  const [pdfDpi, setPdfDpi] = useState(144);
  const needsRasterization = format === 'svg' || format === 'pdf';
  return <div className="modal-backdrop" onClick={close}><section className="modal export-modal" role="dialog" aria-modal="true" aria-label="書き出し" onClick={e => e.stopPropagation()}><div className="modal-heading"><div><span className="eyebrow">EXPORT DESIGN</span><h2>作品を書き出す</h2></div><button className="close-button" aria-label="閉じる" onClick={close}>×</button></div><div className="export-preview"><Square size={30} /><div><strong>{variant.label}</strong><span>{variant.width} × {variant.height} px · sRGB</span></div></div><label className="field"><span>出力形式</span><select aria-label="出力形式" value={format} onChange={e => { setFormat(e.target.value); setAllowRasterization(false); }}><option value="png">PNG（透過対応）</option><option value="jpeg">JPEG（背景色を指定）</option><option value="webp">WebP（ロスレス・透過対応）</option><option value="pdf">PDF（RGB校正・説明資料）</option><option value="svg">SVG（画像を内包する互換形式）</option></select></label>
  {format === 'jpeg' && <div className="field-grid"><label className="field"><span>品質 1–100</span><input aria-label="JPEG品質" type="number" min={1} max={100} value={quality} onChange={e => setQuality(Number(e.target.value))} /></label><label className="field"><span>透過部分の背景色</span><input aria-label="JPEG背景色" type="color" value={background} onChange={e => setBackground(e.target.value)} /></label></div>}
  {format === 'pdf' && <label className="field"><span>PDF解像度 DPI（72–1200）</span><input aria-label="PDF解像度" type="number" value={pdfDpi} min={72} max={1200} onChange={e => setPdfDpi(Number(e.target.value))} /></label>}
  {needsRasterization && <div className="approval-notice"><p>この形式ではアートボード全体を画像として内包します。文字・図形は編集できず、PDFはRGB校正用です。</p><label className="check-field"><input type="checkbox" checked={allowRasterization} onChange={e => setAllowRasterization(e.target.checked)} />この変換を許可する</label></div>}
  <p className="helper">現在の正確プレビューと同じ描画エンジンから出力します。既存ファイルへの上書きは拒否します。</p>{variant.approvalStatus !== 'approved' && <p className="approval-notice">このアートボードは未承認です。制作画面の「承認する」でレイアウトを確定してください。</p>}<button className="button primary full-width" disabled={busy || variant.approvalStatus !== 'approved' || (needsRasterization && !allowRasterization) || (format === 'jpeg' && (!Number.isFinite(quality) || !Number.isInteger(quality) || quality < 1 || quality > 100)) || (format === 'pdf' && (!Number.isFinite(pdfDpi) || !Number.isInteger(pdfDpi) || pdfDpi < 72 || pdfDpi > 1200))} onClick={() => void perform(async () => { const result = await call<unknown>('export_design', { sessionId: session.sessionId, revision: session.document.revision, variantId, recipe: { format, quality: format === 'jpeg' ? quality : null, jpegBackground: format === 'jpeg' ? background : null, webpLossless: true, allowRasterization, pdfDpi } }); if (result) close(); return result; }, '作品を書き出しました')}><ArrowDownToLine size={16} />保存先を選んで書き出す</button></section></div>;
}
