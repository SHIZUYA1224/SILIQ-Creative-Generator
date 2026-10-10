import { useEffect, useState } from 'react';
import { call, issuesFrom, type Issue } from './api';

export interface Candidate { rowId:string; variantId:string; file:string; format:string; issues:Issue[] }
function CandidateImage({planId,index,item}:{planId:string;index:number;item:Candidate}) {
  const [url,setUrl]=useState<string>();
  const [error,setError]=useState('');
  useEffect(()=>{
    let cancelled=false,localUrl:string|undefined;
    setUrl(undefined);setError('');
    if(item.issues.some(issue=>issue.severity==='error')) {setError('検査エラー');return;}
    void call<ArrayBuffer>('request_batch_preview',{planId,itemIndex:index}).then(bytes=>{
      if(cancelled)return;
      localUrl=URL.createObjectURL(new Blob([bytes],{type:'image/png'}));setUrl(localUrl);
    }).catch(e=>{if(!cancelled)setError(issuesFrom(e)[0]?.message ?? '表示できません');});
    return ()=>{cancelled=true;if(localUrl)URL.revokeObjectURL(localUrl);};
  },[planId,index,item]);
  return <figure className="candidate-card"><div>{url?<img src={url} alt={`${item.rowId} ${item.variantId} の検査用縮小画像`} />:<span>{error || '描画中…'}</span>}</div><figcaption><strong>{item.file}</strong><small>{item.rowId} · {item.variantId} · {item.format.toUpperCase()}</small>{item.issues.length>0 && <span>{item.issues.filter(i=>i.severity==='error').length} エラー / {item.issues.filter(i=>i.severity==='warning').length} 注意</span>}</figcaption></figure>;
}
export function BatchGallery({planId,items}:{planId:string;items:Candidate[]}) {
  const [page,setPage]=useState(0),pageSize=12,total=Math.max(1,Math.ceil(items.length/pageSize));
  useEffect(()=>setPage(0),[planId]);
  return <div className="candidate-gallery"><p className="helper">検査用縮小画像。各形式の元になる共通描画を表示します。JPEGの背景など、形式固有の変換は出力結果に適用します。</p><div className="candidate-pagination"><button disabled={page===0} onClick={()=>setPage(p=>p-1)}>前へ</button><span>{page+1} / {total} ページ · 全 {items.length} 件</span><button disabled={page+1>=total} onClick={()=>setPage(p=>p+1)}>次へ</button></div><div className="candidate-grid">{items.slice(page*pageSize,(page+1)*pageSize).map((item,i)=><CandidateImage key={`${planId}-${page*pageSize+i}`} planId={planId} index={page*pageSize+i} item={item} />)}</div></div>;
}
