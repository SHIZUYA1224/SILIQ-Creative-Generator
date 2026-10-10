#!/usr/bin/env python3
"""Development-only native CLI wall/RSS measurement; never claims UI timings.

Run after verified release build, with other builds/tests stopped. Every output
is new. A run directory records complete native reports and time -l evidence.
"""
from __future__ import annotations
import argparse
import datetime
import hashlib
import json
import math
from pathlib import Path
import platform
import re
import subprocess
import time

ROOT=Path(__file__).resolve().parents[1]
FIXTURE=ROOT/'fixtures/performance-safe'


def sha(path:Path)->str:return hashlib.sha256(path.read_bytes()).hexdigest()


def percentile(values:list[float],percent:float)->float:
 return sorted(values)[max(0,math.ceil(len(values)*percent)-1)]


def system_value(*args:str)->str:return subprocess.check_output(args,text=True).strip()


def invoke(args:list[str], log:Path)->tuple[dict,float,int]:
 started=time.perf_counter()
 result=subprocess.run(['/usr/bin/time','-l',str(ROOT/'target/release/siliq-render'),*args],cwd=ROOT,text=True,capture_output=True)
 elapsed=time.perf_counter()-started
 log.with_suffix('.stdout.json').write_text(result.stdout)
 log.with_suffix('.time.txt').write_text(result.stderr)
 if result.returncode:raise RuntimeError(f'native CLI failed; inspect {log.name}')
 match=re.search(r'\b(\d+)\s+maximum resident set size\b',result.stderr)
 if not match:raise RuntimeError('time -l did not report macOS peak RSS')
 return json.loads(result.stdout),elapsed,int(match.group(1))


def main(samples:int,output:Path)->None:
 if platform.system()!='Darwin':raise RuntimeError('this RSS measurement profile is macOS only')
 output.mkdir(parents=True,exist_ok=False)
 render_dir=output/'single';render_dir.mkdir()
 batch_dir=output/'batch';batch_dir.mkdir()
 document=FIXTURE/'documents/standard.json';dataset=FIXTURE/'data/100-rows.csv';resources=FIXTURE/'resources'
 spec=json.loads((FIXTURE/'fixture-manifest.json').read_text())
 initial_load=system_value('sysctl','-n','vm.loadavg')
 assert sha(document)==spec['documentSha256'] and sha(dataset)==spec['datasetSha256']
 for path,checksum in spec['resourceSha256'].items():assert sha(FIXTURE/path)==checksum
 timings=[];rss=[];hashes=[];profiles=[]
 for index in range(samples+1):
  name=f'standard-{index:03d}.png'
  report,elapsed,memory=invoke(['render','--doc',str(document),'--variant','v1','--resources',str(resources),
          '--out-dir',str(render_dir),'--name',name],output/f'single-{index:03d}')
  image=render_dir/name
  assert image.read_bytes()[:8]==b'\x89PNG\r\n\x1a\n'
  assert sha(image)==report['outputSha256']
  assert (report['width'],report['height'])==(1080,1350)
  assert len(report['fonts'])==3 and len(report['assets'])==8
  assert not report['warnings']
  timings.append(elapsed);rss.append(memory);hashes.append(report['outputSha256']);profiles.append(report['renderProfile'])
  print(f'Single {index+1}/{samples+1}: {elapsed:.3f}s, {memory/1024**2:.1f} MiB',flush=True)
 assert len(set(hashes))==1,'same native input produced different bytes'
 assert all(profile==profiles[0] for profile in profiles)
 batch,wall,batch_rss=invoke(['batch','--doc',str(document),'--data',str(dataset),'--variants','v1,v2,v3',
        '--resources',str(resources),'--out-dir',str(batch_dir),'--manifest','manifest.json'],output/'batch-run')
 manifest=json.loads((batch_dir/'manifest.json').read_text())
 assert manifest['status']=='completed' and manifest['errorPolicy']=='block-all' and not manifest['issues']
 assert manifest['rows']==100 and manifest['total']==300 and len(manifest['items'])==300
 assert manifest['templateSha256']==sha(document) and manifest['dataSha256']==sha(dataset)
 expected={f'row-{row:03d}_v{variant}.png' for row in range(1,101) for variant in range(1,4)}
 files={item['file'] for item in manifest['items']};assert files==expected
 assert {p.name for p in batch_dir.glob('*.png')}==expected
 identities=set();output_hashes=set()
 for item in manifest['items']:
  identity=(item['rowId'],item['variantId'],item['format']);assert identity not in identities;identities.add(identity)
  image=batch_dir/item['file'];assert image.read_bytes()[:8]==b'\x89PNG\r\n\x1a\n'
  assert sha(image)==item['sha256'] and (item['width'],item['height'])==(1080,1350)
  assert item['inputDigest'] and not item['warnings'];output_hashes.add(item['sha256'])
 assert len(output_hashes)==300
 summary=dict(measuredAtUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
   machine=dict(cpu=system_value('sysctl','-n','machdep.cpu.brand_string'),logicalCpus=int(system_value('sysctl','-n','hw.ncpu')),
       memoryBytes=int(system_value('sysctl','-n','hw.memsize')),os=system_value('sw_vers','-productVersion'),osBuild=system_value('sw_vers','-buildVersion'),
       architecture=platform.machine(),powerState=system_value('pmset','-g','batt'),screenScale='not measured; headless CLI',
       initialLoadAverage=initial_load,finalLoadAverage=system_value('sysctl','-n','vm.loadavg'),
       backgroundLoad='builds/tests stopped; other desktop/background applications were not stopped',
       storage='local workspace filesystem; SSD performance not independently profiled',build='Rust release optimized; checksum-verified native Skia',
       executableSha256=sha(ROOT/'target/release/siliq-render')),
   fixture=spec,single=dict(sampleCount=samples+1,firstProcessSeconds=timings[0],repeatedProcessSampleCount=samples,
       repeatedProcessP50Seconds=percentile(timings[1:],.50),repeatedProcessP95Seconds=percentile(timings[1:],.95),
       allProcessWallSeconds=timings,allProcessPeakRssBytes=rss,maxPeakRssBytes=max(rss),uniqueOutputHashCount=len(set(hashes)),outputSha256=hashes[0],
       cacheScope='fresh CLI/ResourceStore each sample; OS cache may be warm; assets decoded inside every native render; no predecoded native cache'),
   batch=dict(wallSeconds=wall,cliReportedSeconds=batch['seconds'],peakRssBytes=batch_rss,rows=100,variantCount=3,expectedOutputs=300,
       validatedOutputs=len(files),uniqueOutputHashCount=len(output_hashes),manifestSha256=sha(batch_dir/'manifest.json'),
       engineScope='full block-all native preflight, rerender of every item, durable journal, no-clobber publish and final manifest; sequential CLI job'),
   nativeRenderProfile=profiles[0],scope='external CLI wall clock and per-process peak RSS, not internal renderer-stage timings',
   unmeasured=['true filesystem-cold first launch','warm predecoded native render','renderer stage timings (RenderReport has none)',
       'UI startup/interaction/IME','native-to-WebView transfer and display','Host+WebView+worker combined memory','attributed network observation','M1 16GB baseline performance'])
 (output/'summary.json').write_text(json.dumps(summary,ensure_ascii=False,indent=2)+'\n')
 print(f'Batch: {wall:.3f}s, {batch_rss/1024**2:.1f} MiB; 300 filenames/header/hash/manifest identities verified.',flush=True)
 print('Summary:',output.relative_to(ROOT)/'summary.json',flush=True)


if __name__=='__main__':
 parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--samples',type=int,default=20)
 parser.add_argument('--output',type=Path,default=ROOT/'out/performance'/datetime.datetime.now(datetime.timezone.utc).strftime('run-%Y%m%dT%H%M%SZ'))
 args=parser.parse_args()
 if not 1<=args.samples<=100:parser.error('samples must be 1..100')
 resolved=args.output.resolve()
 if not resolved.is_relative_to((ROOT/'out/performance').resolve()):parser.error('output must be inside out/performance')
 main(args.samples,resolved)
