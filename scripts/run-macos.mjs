import {spawn} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {join} from 'node:path';
const root=fileURLToPath(new URL('../',import.meta.url));
const build=spawn('python3',[join(root,'scripts/build-macos.py'),'--debug'],{cwd:root,stdio:'inherit'});
build.on('exit',code=>{if(code!==0)process.exit(code??1);const app=spawn('open',[join(root,'target/debug/bundle/macos/SILIQ Creative Generator.app')],{stdio:'inherit'});app.on('exit',code=>process.exit(code??1));});
for(const signal of ['SIGINT','SIGTERM'])process.on(signal,()=>build.kill(signal));
