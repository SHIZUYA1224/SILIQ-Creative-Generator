import { useEffect, useState } from 'react';
import { ArrowLeft, Check, FileSpreadsheet, LoaderCircle, Play, RefreshCw, Square } from 'lucide-react';
import { IssueDetails } from './IssueDetails';
import { BatchGallery, type Candidate } from './BatchGallery';
import { call, issuesFrom, type Issue, type SessionView } from './api';

interface DatasetView { datasetId: string; columns: string[]; rowCount: number; sampleRows: Record<string, string>[] }
interface PlanView { previewPlanId?:string; items:Candidate[]; planId: string | null; rows: number; total: number; variantIds: string[]; previewNames: string[]; issues: Issue[] }
interface JobView { jobId: string; status: string; total: number; completed: number; currentRowId?: string; currentVariantId?: string; issues: Issue[] }
interface SavedBatch { snapshotId: string; documentTitle: string; revision: number; rows: number; total: number; variantIds: string[]; errorPolicy: string }
const statuses: Record<string, string> = { queued: '待機中', running: '書き出し中', completed: '完了', cancelled: '停止しました', interrupted: '中断しました', failed: '失敗', completed_with_errors: '問題のある項目を除いて完了' };

export function BatchPanel({ visible, session, close, onIssues }: { visible: boolean; session: SessionView; close: () => void; onIssues: (issues: Issue[]) => void }) {
  const [dataset, setDataset] = useState<DatasetView>();
  const [mapping, setMapping] = useState<Record<string, string>>({});
  const [variants, setVariants] = useState(session.document.variants.filter(v => v.approvalStatus === 'approved').map(v => v.id));
  const [pattern, setPattern] = useState('{rowId}_{variantId}.png');
  const [plan, setPlan] = useState<PlanView>();
  const [job, setJob] = useState<JobView>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<Issue[]>([]);
  const [validOnly, setValidOnly] = useState(false);
  const [formats, setFormats] = useState(['png']);
  const [allowRasterization, setAllowRasterization] = useState(false);
  const [jpegBackground, setJpegBackground] = useState('#ffffff');
  const [savedBatches, setSavedBatches] = useState<SavedBatch[]>([]);
  const [savedId, setSavedId] = useState('');
  const running = job?.status === 'running' || job?.status === 'queued';
  useEffect(()=>{setDataset(undefined);setMapping({});setVariants(session.document.variants.filter(v=>v.approvalStatus==='approved').map(v=>v.id));setPlan(undefined);setSavedId('');},[session.sessionId]);
  useEffect(() => { setPlan(undefined); }, [session.document.revision, session.sessionId, mapping, variants, pattern, validOnly, formats, allowRasterization, jpegBackground]);
  useEffect(() => {
    if (!visible) return;
    let cancelled = false;
    void call<SavedBatch[]>('list_saved_batches').then(items => { if (!cancelled) setSavedBatches(items); }).catch(e => { if (!cancelled) setError(issuesFrom(e)); });
    return () => { cancelled = true; };
  }, [visible, job?.status]);
  useEffect(() => {
    if (!running || !job) return;
    let cancelled = false;
    const poll = async () => {
      try { const next = await call<JobView>('batch_status', { jobId: job.jobId }); if (!cancelled) setJob(next); }
      catch (e) { if (!cancelled) { const issues = issuesFrom(e); setError(issues); onIssues(issues); } }
    };
    const timer = window.setInterval(() => void poll(), 600);
    void poll();
    return () => { cancelled = true; window.clearInterval(timer); };
  }, [running, job?.jobId, onIssues]);
  const operation = async (action: () => Promise<void>) => {
    if (busy) return;
    setBusy(true); setError([]);
    try { await action(); } catch (e) { const issues = issuesFrom(e); setError(issues); onIssues(issues); } finally { setBusy(false); }
  };
  if (!visible) return null;
  return <div className="modal-backdrop"><section className="modal batch-modal" role="dialog" aria-modal="true" aria-label="データから量産"><div className="modal-heading"><div><span className="eyebrow">BATCH PRODUCTION</span><h2>承認したデザインを、まとめて制作。</h2><p>入力データと作品の版を固定して、安全に書き出します。</p></div><button className="close-button" aria-label="閉じる" onClick={close}>×</button></div><div className="batch-layout"><div>
    <h3 className="batch-step">01 データを選ぶ</h3><button className="button secondary full-width" disabled={busy || running} onClick={() => void operation(async () => {
      const next = await call<DatasetView | null>('select_dataset', { sessionId: session.sessionId });
      if (!next) return;
      setDataset(next); setPlan(undefined);
      setMapping(Object.fromEntries(next.columns.map(column => [column, column === 'rowId' || session.document.slots.some(slot => slot.id === column) ? column : ''])));
    })}><FileSpreadsheet size={15} />CSV / JSON を取り込む</button><p className="helper">UTF-8・先頭ゼロを保持。値を数式として実行しません。</p>
    {dataset && <><div className="dataset-summary"><Check size={13} />{dataset.rowCount} 行 · {dataset.columns.length} 列を取り込み済み</div><h3 className="batch-step">02 列をスロットに対応付ける</h3>{dataset.columns.map(column => <label className="mapping-row" key={column}><span>{column}</span><select aria-label={`${column}の対応先`} disabled={running || busy} value={mapping[column] ?? ''} onChange={e => setMapping(previous => ({ ...previous, [column]: e.target.value }))}><option value="">使用しない</option><option value="rowId">行 ID（ファイル名）</option>{session.document.slots.map(slot => <option key={slot.id} value={slot.id}>{slot.id}{slot.required ? '（必須）' : ''}</option>)}</select></label>)}</>}
    <h3 className="batch-step">03 承認済みサイズを選ぶ</h3>{session.document.variants.map(variant => <label className="check-field" key={variant.id}><input type="checkbox" checked={variants.includes(variant.id)} disabled={busy || running || variant.approvalStatus !== 'approved'} onChange={e => setVariants(previous => e.target.checked ? [...previous, variant.id] : previous.filter(id => id !== variant.id))} />{variant.label}{variant.approvalStatus !== 'approved' ? '（未承認）' : ''}</label>)}<label className="field"><span>出力名</span><input aria-label="量産の出力名" value={pattern} disabled={busy || running} onChange={e => setPattern(e.target.value)} /></label><p className="helper">変数: {'{rowId}'} / {'{variantId}'} / {'{format}'}。拡張子は選択形式に合わせます。WebPはロスレスです。</p>
    <h3 className="batch-step">04 出力形式</h3><div className="format-options">{['png','jpeg','webp','pdf','svg'].map(format => <label className="check-field" key={format}><input type="checkbox" checked={formats.includes(format)} disabled={busy || running} onChange={e => setFormats(previous => e.target.checked ? [...previous,format] : previous.filter(f=>f!==format))} />{format.toUpperCase()}</label>)}</div>{formats.includes('jpeg') && <label className="field"><span>JPEGの背景色（品質90）</span><input aria-label="量産JPEG背景色" type="color" value={jpegBackground} disabled={busy || running} onChange={e=>setJpegBackground(e.target.value)} /></label>}{(formats.includes('pdf') || formats.includes('svg')) && <div className="approval-notice"><label className="check-field"><input type="checkbox" checked={allowRasterization} disabled={busy || running} onChange={e=>setAllowRasterization(e.target.checked)} />PDF / SVG 全体の画像化を許可する</label><p>文字・図形は編集できません。PDFはRGB校正用・144 DPIです。</p></div>}
    <label className="check-field"><input type="checkbox" checked={validOnly} disabled={busy || running} onChange={e => setValidOnly(e.target.checked)} />エラーのある候補を除いて出力する</label><p className="helper">通常はエラーが1件でもあれば停止します。この選択では除外した項目も結果に記録します。</p>
    <button className="button primary full-width" disabled={busy || running || !dataset || variants.length === 0 || formats.length === 0 || ((formats.includes('pdf') || formats.includes('svg')) && !allowRasterization)} onClick={() => void operation(async () => {
      const current = await call<SessionView>('read_session', { sessionId: session.sessionId });
      const result = await call<PlanView | null>('preflight_batch', { sessionId: current.sessionId, revision: current.document.revision, datasetId: dataset!.datasetId, mapping: Object.fromEntries(Object.entries(mapping).filter(([,value]) => value)), variantIds: variants, namePattern: pattern, errorPolicy: validOnly ? 'valid-only' : 'block-all', recipes: formats.map(format=>({format,quality:format==='jpeg'?90:null,jpegBackground:format==='jpeg'?jpegBackground:null,webpLossless:true,allowRasterization,pdfDpi:144})) });
      if (result) { setPlan(result); setError(result.issues); }
    })}>{busy ? <LoaderCircle className="spin" size={14} /> : <Check size={14} />}全件検査・出力先を選択</button></div><div className="batch-results"><h3 className="batch-step">プレビューと検査結果</h3>{dataset ? <div className="data-preview"><table><thead><tr>{dataset.columns.map(column => <th key={column}>{column}</th>)}</tr></thead><tbody>{dataset.sampleRows.map((row, i) => <tr key={i}>{dataset.columns.map(column => <td key={column}>{row[column]}</td>)}</tr>)}</tbody></table><p className="helper">先頭 {dataset.sampleRows.length} 行の入力内容</p></div> : <div className="batch-empty"><FileSpreadsheet size={36} /><p>データを取り込むと、入力内容と検査結果を確認できます。</p></div>}
    {plan && <div className="batch-plan"><div className="batch-stat"><strong>{plan.total}</strong><span>件の候補 · {plan.rows} 行 × {plan.variantIds.length} サイズ × {formats.length} 形式</span></div><p className="helper">元のデザインや素材を編集しても、このジョブの入力は変わりません。</p>{(plan.previewPlanId ?? plan.planId) && plan.items.length>0 && <BatchGallery planId={(plan.previewPlanId ?? plan.planId)!} items={plan.items} />}<button className="button primary full-width" disabled={busy || running || !plan.planId || (plan.issues.some(issue => issue.severity === 'error') && !validOnly)} onClick={() => void operation(async () => setJob(await call<JobView>('start_batch', { planId: plan.planId })))}><Play size={14} />{validOnly ? '正常な候補を書き出す' : `${plan.total} 件を書き出す`}</button></div>}
    {job && <div className="job-progress"><strong>{statuses[job.status] ?? job.status}</strong><progress value={job.completed} max={Math.max(1, job.total)} /><span>{job.completed} / {job.total} 件{job.currentRowId ? ` · ${job.currentRowId}` : ''}</span>{running ? <button className="button secondary" disabled={busy} onClick={() => void operation(async () => setJob(await call<JobView>('cancel_batch', { jobId: job.jobId })))}><Square size={12} />安全に停止する</button> : ['cancelled','interrupted','failed'].includes(job.status) ? <button className="button secondary" disabled={busy} onClick={() => void operation(async () => setJob(await call<JobView>('resume_batch', { jobId: job.jobId })))}><RefreshCw size={13} />確認して再開する</button> : null}{job.issues.map((issue, i) => <p className="batch-error" key={i}>{issue.code}: {issue.message}</p>)}</div>}
    {error.length > 0 && <div className="batch-errors">{error.map((issue,i) => <div key={i}><code>{issue.code}</code><IssueDetails issue={issue} /></div>)}</div>}
    </div></div>{savedBatches.length > 0 && <div className="saved-jobs"><h3 className="batch-step">保存済みジョブから再開</h3><div className="saved-job-row"><select aria-label="保存済みジョブ" value={savedId} disabled={busy || running} onChange={e => setSavedId(e.target.value)}><option value="">ジョブを選択</option>{savedBatches.map(item => <option key={item.snapshotId} value={item.snapshotId}>{item.documentTitle} · revision {item.revision} · {item.total} 件</option>)}</select><button className="button secondary" disabled={!savedId || busy || running} onClick={() => void operation(async () => { const result = await call<JobView | null>('resume_saved_batch', { snapshotId: savedId }); if (result) setJob(result); })}><RefreshCw size={13} />出力先を再選択して再開</button></div><p className="helper">同じ出力先を選んでください。既存出力の内容を照合し、別の入力や改変されたファイルには上書きしません。</p></div>}<button className="button secondary" onClick={close}><ArrowLeft size={13} />{running ? '量産を続けて制作に戻る' : '制作に戻る'}</button></section></div>;
}
