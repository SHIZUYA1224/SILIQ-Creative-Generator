"""Generate a deterministic ICC v2 matrix/TRC description of sRGB.

No downloaded profile or third-party implementation is copied. Numeric facts:
https://registry.color.org/rgb-registry/files/sRGB.pdf (ICC sRGB guidance).
ICC header/tag layout: https://www.color.org/icc_specs2.html.
This generated mathematical profile contains no device or personal metadata.
"""
from pathlib import Path
import struct

def fixed(value): return struct.pack('>i', round(value * 65536))
def xyz(values): return b'XYZ '+bytes(4)+b''.join(fixed(v) for v in values)
def desc(text):
    a=text.encode('ascii')+b'\0'
    return b'desc'+bytes(4)+struct.pack('>I',len(a))+a+bytes(4+4+2+1+67)
# ICC v2 uses a sampled curve, avoiding v4's reference black scaling policy.
curve=[]
for i in range(4096):
    v=i/4095
    linear=v/12.92 if v<=0.04045 else ((v+0.055)/1.055)**2.4
    curve.append(round(linear*65535))
trc=b'curv'+bytes(4)+struct.pack('>I',len(curve))+struct.pack('>'+str(len(curve))+'H',*curve)
tags=[(b'desc',desc('SILIQ sRGB')),(b'cprt',b'text'+bytes(4)+b'Generated mathematical sRGB profile; public domain.\0'),(b'wtpt',xyz((0.9642,1.0,0.8249))),(b'rXYZ',xyz((0.4360747,0.2225045,0.0139322))),(b'gXYZ',xyz((0.3850649,0.7168786,0.0971045))),(b'bXYZ',xyz((0.1430804,0.0606169,0.7141733))),(b'rTRC',trc),(b'gTRC',trc),(b'bTRC',trc)]
header=bytearray(128)
header[4:8]=b'SILQ';header[8:12]=struct.pack('>I',0x02100000);header[12:16]=b'mntr';header[16:20]=b'RGB ';header[20:24]=b'XYZ '
header[24:36]=struct.pack('>6H',2000,1,1,0,0,0);header[36:40]=b'acsp';header[64:68]=struct.pack('>I',1);header[68:80]=b''.join(fixed(v) for v in (0.9642,1.0,0.8249));header[80:84]=b'SILQ'
base=128+4+12*len(tags);table=bytearray(struct.pack('>I',len(tags)));payload=bytearray();shared={}
for signature,data in tags:
    if data in shared: offset,length=shared[data]
    else:
        while len(payload)%4: payload.append(0)
        offset,length=base+len(payload),len(data);shared[data]=(offset,length);payload.extend(data)
    table.extend(signature+struct.pack('>II',offset,length))
profile=header+table+payload
profile[0:4]=struct.pack('>I',len(profile))
output=Path(__file__).resolve().parent.parent/'crates/design-render/resources/srgb.icc'
output.parent.mkdir(parents=True,exist_ok=True);output.write_bytes(profile)
