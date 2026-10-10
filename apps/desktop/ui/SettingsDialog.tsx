import { useEffect, useState } from 'react';
import { HardDrive, RefreshCw, ShieldCheck } from 'lucide-react';
import { call, issuesFrom, type Issue } from './api';

interface StorageStatus { displayPath?: string; workspaceBytes: number; recoveryBytes: number; jobBytes: number; workspaceCount: number; savedJobCount: number; cleanableWorkspaceBytes: number; cleanableRecoveryBytes: number; cleanableJobBytes: number; protectedCount: number }
const size = (bytes: number) => bytes < 1024*1024 ? `${Math.round(bytes/1024)} KB` : `${(bytes/(1024*1024)).toFixed(1)} MB`;
export function SettingsDialog({ close, onIssues }: { close: () => void; onIssues: (issues: Issue[]) => void }) {
  const [status, setStatus] = useState<StorageStatus>();
  const [busy, setBusy] = useState(false);
  const [kind, setKind] = useState<'recovery'|'jobs'|'workspaces'>();
  const [notice, setNotice] = useState('');
  const refresh = async () => { try { setStatus(await call<StorageStatus>('storage_status')); } catch(e) { onIssues(issuesFrom(e)); } };
  useEffect(() => { void refresh(); }, []);
  const rows = status ? [
    {kind:'workspaces' as const,label:'ローカル作業領域',total:status.workspaceBytes,cleanable:status.cleanableWorkspaceBytes,description:'保存・復旧・ジョブから参照されていない、閉じた作品の一時データ。'},
    {kind:'recovery' as const,label:'編集の復旧データ',total:status.recoveryBytes,cleanable:status.cleanableRecoveryBytes,description:'閉じた作品の古い復旧データ。削除すると、その編集を復元できなくなります。'},
    {kind:'jobs' as const,label:'量産ジョブの保存データ',total:status.jobBytes,cleanable:status.cleanableJobBytes,description:'完了済みジョブの入力スナップショット。書き出した成果物には影響しません。'},
  ] : [];
  return <div className="modal-backdrop" onClick={close}><section className="modal settings-modal" role="dialog" aria-modal="true" aria-label="保存領域と復旧" onClick={e=>e.stopPropagation()}><div className="modal-heading"><div><span className="eyebrow">LOCAL STORAGE</span><h2>この Mac 内の保存領域</h2></div><button className="close-button" aria-label="閉じる" onClick={close}>×</button></div><p className="helper"><ShieldCheck size={12} />開いている作品、最新の復旧、実行中・中断中のジョブは保護します。</p>{status?.displayPath && <p className="storage-path">{status.displayPath}</p>}<div className="storage-rows">{rows.map(row=><div key={row.kind}><HardDrive size={18} /><div><strong>{row.label}</strong><span>{size(row.total)} · 削除可能 {size(row.cleanable)}</span><p>{row.description}</p></div><button className="button secondary" disabled={busy || row.cleanable===0} onClick={()=>setKind(row.kind)}>整理する</button></div>)}</div>{kind && <div className="approval-notice"><p>{rows.find(row=>row.kind===kind)?.description} この削除は元に戻せません。</p><div className="confirmation-buttons"><button className="button secondary" disabled={busy} onClick={()=>setKind(undefined)}>キャンセル</button><button className="button primary" disabled={busy} onClick={()=>void (async()=>{setBusy(true);try{const result=await call<{removedCount:number;removedBytes:number;status:StorageStatus}>('cleanup_storage',{kind});setStatus(result.status);setNotice(`${result.removedCount} 件・${size(result.removedBytes)}を整理しました`);setKind(undefined);}catch(e){onIssues(issuesFrom(e));}finally{setBusy(false);}})()}>確認して削除する</button></div></div>}<button className="button secondary" disabled={busy} onClick={()=>void refresh()}><RefreshCw size={12} />使用量を更新</button>{notice && <p className="helper" role="status">{notice}</p>}<p className="helper">保存済みプロジェクトと出力先のファイルを、この設定から削除することはありません。</p></section></div>;
}
