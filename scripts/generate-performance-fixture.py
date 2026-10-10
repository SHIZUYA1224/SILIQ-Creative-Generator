#!/usr/bin/env python3
"""Deterministic public-safe 120-layer / 8-image / 3-font performance input.

All pictures are synthetic. Existing unmodified OFL fonts are copied by their
verified SHA-256. Files are written under fixtures/performance-safe only; this
directory is intentionally separate from the packaged demo fixture directory.
"""
from __future__ import annotations
import csv
import hashlib
import json
from pathlib import Path
import shutil
import struct
import zlib

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'fixtures/performance-safe'
FONT_SOURCES = [
 ('regular', 'dff723ba59d57d136764a04b9b2d03205544f7cd785a711442d6d2d085ac5073'),
 ('bold', '1b0edfb500b73a4fa8a4fcaae1bbbd403994e08e73e3e0da37e70d3853f42c5f'),
 ('black', '3aa30b0956510f4205f759ab3079a5b658310ebcda2577f290466ea51c948819')]


def chunk(tag: bytes, data: bytes) -> bytes:
 return struct.pack('>I', len(data)) + tag + data + struct.pack('>I', zlib.crc32(tag + data) & 0xffffffff)


def picture(seed: int) -> bytes:
 width, height = 1600, 1000
 # Fixed row patterns, grid, and opaque subject; no time/random or real image.
 columns = [bytes(((37 + x // 9 + seed * 21) % 256, (90 + x // 13 + seed * 15) % 256,
                   (170 + x // 21 + seed * 17) % 256, 255)) for x in range(width)]
 row = b''.join(columns)
 raw = bytearray()
 for y in range(height):
  raw.append(0)
  scan = bytearray(row)
  if y % 100 < 2:
   scan = bytearray(bytes((40 + seed * 8, 50 + seed * 9, 60 + seed * 7, 255)) * width)
  if 240 <= y <= 760:
   left = 300 + (y - 240) // 3 + seed * 20
   scan[left * 4:(left + 200) * 4] = bytes((245, 200 - seed * 8, 60 + seed * 10, 255)) * 200
  raw.extend(scan)
 return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 6, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(raw, 9)) + chunk(b'IEND', b'')


def node(node_id: str, kind: str, bounds: tuple[int, int, int, int], **fields) -> dict:
 x, y, width, height = bounds
 return dict(id=node_id, name=node_id, type=kind, bounds=dict(x=x, y=y, width=width, height=height),
             rotationDeg=0, opacity=1, visible=True, locked=False, **fields)


def text(node_id: str, value: str, font_id: str, bounds: tuple[int, int, int, int], size=15) -> dict:
 return node(node_id, 'text', bounds, text=dict(value=value, fontId=font_id, fontSize=size, lineHeight=size * 1.6,
             letterSpacing=0, maxLines=1, overflowPolicy='error', allowedFontSizes=[], language='ja', align='left', kinsoku='strict-japanese'),
             fill=dict(kind='solid', color='#203A35'))


def main() -> None:
 (OUT / 'resources/assets').mkdir(parents=True, exist_ok=True)
 (OUT / 'resources/fonts').mkdir(parents=True, exist_ok=True)
 (OUT / 'documents').mkdir(exist_ok=True)
 (OUT / 'data').mkdir(exist_ok=True)
 (OUT / 'licenses').mkdir(exist_ok=True)
 fonts=[]; assets=[]; hashes={}
 for name, checksum in FONT_SOURCES:
  source=ROOT / f'fixtures/public-safe/resources/fonts/{checksum}.otf'
  assert hashlib.sha256(source.read_bytes()).hexdigest()==checksum
  destination=OUT / f'resources/fonts/{checksum}.otf';shutil.copyfile(source,destination)
  hashes[destination.relative_to(OUT).as_posix()]=checksum
  fonts.append(dict(id=name, nameHint='Noto Sans JP '+name+' (unmodified OFL fixture)', state='resolved', sha256=checksum, faceIndex=0, packagePolicy='embed-confirmed'))
 shutil.copyfile(ROOT / 'fixtures/public-safe/licenses/NotoSansJP-OFL-1.1.txt', OUT / 'licenses/NotoSansJP-OFL-1.1.txt')
 for index in range(8):
  data=picture(index);checksum=hashlib.sha256(data).hexdigest()
  destination=OUT / f'resources/assets/{checksum}.png';destination.write_bytes(data)
  hashes[destination.relative_to(OUT).as_posix()]=checksum
  assets.append(dict(id=f'image-{index}', kind='raster', state='resolved', sha256=checksum, mimeHint='image/png'))
 stops=[dict(offset=0,color='#EFF9F4',opacity=1),dict(offset=1,color='#A9D8C9',opacity=1)]
 layers=[node('background','rect',(0,0,1080,1350),fill=dict(kind='linear-gradient',angleDeg=90,stops=stops),cornerRadius=0),
         node('radial','rect',(640,30,360,130),fill=dict(kind='radial-gradient',center=dict(x=.5,y=.5),radius=.8,
                 stops=[dict(offset=0,color='#FFFFFF',opacity=.8),dict(offset=1,color='#FFFFFF',opacity=0)]),cornerRadius=65),
         text('headline','公開用の資料 BENCH 001','black',(40,35,900,62),32),
         text('subheadline','日本語と English / 固定された検証素材','regular',(40,115,950,40),20)]
 styles=[]
 for index in range(8):
  x=40+(index%4)*255;y=195+(index//4)*290
  layers.append(node(f'picture-{index}','image',(x,y,225,220),assetId=f'image-{index}',fit='cover',focalPoint=dict(x=.5,y=.5),clipToBounds=True))
  layers.append(text(f'image-label-{index}',f'資料 {index+1} / Image','bold',(x,y+228,225,35),17))
  styles.append(dict(nodeId=f'picture-{index}',effects=[dict(kind='shadow',color='#102A25',opacity=.28,offsetX=2,offsetY=4,blur=5)]))
 for index in range(40):
  x=40+(index%4)*255;y=800+(index//4)*43
  layers.append(text(f'line-{index}',f'日本語 AB {index:02d}',FONT_SOURCES[index%3][0],(x,y,225,32)))
 while len(layers)<120:
  index=len(layers);x=40+(index%12)*83;y=1250+((index//12)%2)*25
  layers.append(node(f'detail-{index}','rect',(x,y,58,10),fill=dict(kind='solid',color=['#216B57','#85BDAA','#DBEEE5'][index%3]),cornerRadius=3))
 root=node('root','group',(0,0,1080,1350),childIds=[n['id'] for n in layers])
 doc=dict(format='siliq-creative-generator-core',schemaVersion='0.4.0-core',documentId='performance-standard',revision=1,
          title='Public-safe standard performance fixture (synthetic; approval for QA only)',colorSpace='sRGB',
          baseArtboard=dict(width=1080,height=1350,rootNodeId='root'),nodes=[root,*layers],fonts=fonts,assets=assets,
          slots=[dict(id='headline',type='text',required=True,nodeId='headline',target='text.value',maxLength=40)],
          nodeStyles=styles,variants=[dict(id=f'v{i}',label=f'Approved QA variant {i}',width=1080,height=1350,
          approvalStatus='approved',nodePatches=[]) for i in range(1,4)])
 # text slots change every row; variants have distinct explicit placement.
 for i,v in enumerate(doc['variants']):
  v['nodePatches']=[] if i==0 else [dict(nodeId='headline',bounds=dict(x=40+i*4,y=35,width=900,height=62))]
 path=OUT / 'documents/standard.json';path.write_text(json.dumps(doc,ensure_ascii=False,indent=2)+'\n')
 with (OUT / 'data/100-rows.csv').open('w',newline='') as file:
  writer=csv.writer(file,lineterminator='\n');writer.writerow(['rowId','headline'])
  for index in range(1,101):writer.writerow([f'row-{index:03d}',f'公開用の資料 BENCH {index:03d}'])
 spec=dict(formatVersion='1.0.0',purpose='public-safe native performance measurement',width=1080,height=1350,
           drawableLayers=120,totalNodes=121,imageCount=8,imageSizes=[[1600,1000]]*8,fontFaces=3,gradientKinds=['linear-gradient','radial-gradient'],imageClips=8,shadowCount=8,
           datasetRows=100,approvedVariants=['v1','v2','v3'],pngCodec='native Skia PNG; profile reported by renderer',
           imageGenerator='scripts/generate-performance-fixture.py; fixed pixel patterns; PNG zlib '+zlib.ZLIB_VERSION,
           resourceSha256=hashes,documentSha256=hashlib.sha256(path.read_bytes()).hexdigest(),datasetSha256=hashlib.sha256((OUT/'data/100-rows.csv').read_bytes()).hexdigest())
 (OUT/'fixture-manifest.json').write_text(json.dumps(spec,ensure_ascii=False,indent=2)+'\n')
 print('Generated 120 visible drawable layers + root, 8 x 1.6MP images, 3 font faces, 100 rows x 3 approved variants.')

if __name__=='__main__':main()
