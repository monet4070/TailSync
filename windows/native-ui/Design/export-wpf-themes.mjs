import {readFile,writeFile} from 'node:fs/promises';
import {fileURLToPath} from 'node:url';
import {dirname,resolve} from 'node:path';
const here=dirname(fileURLToPath(import.meta.url));
const source=await readFile(resolve(here,'../../../shared/tailsync-themes/src/builtins.rs'),'utf8');
const names=['Accent','AccentHover','AccentSoft','OnAccent','Canvas','Surface','Input','Hover','Active','Raised','Toast','Primary','Secondary','Tertiary','ToastText','Border','StrongBorder','Divider','Positive','PositiveSoft','Warning','WarningSoft','Info','InfoSoft'];
const result={};
for(const id of ['CANVAS','FLUX','LEDGER','AURA','MONO']){
 result[id.toLowerCase()]={};
 for(const dark of [false,true]){
  const block=source.split(`(${id}_ID, ${dark}) => (`)[1]?.split('),')[0];
  if(!block)throw Error(`Missing ${id}/${dark}`);
  const colors=[...block.matchAll(/"([^"]+)"/g)].map(m=>m[1]);
  if(colors.length!==names.length)throw Error(`Unexpected palette shape for ${id}`);
  result[id.toLowerCase()][dark?'dark':'light']=Object.fromEntries(names.map((name,i)=>[name,colors[i]]));
 }
}
const out=resolve(here,'wpf-palettes.generated.json'),text=JSON.stringify(result,null,2)+'\n';
if(process.argv.includes('--check')){if(await readFile(out,'utf8')!==text)throw Error('Regenerate WPF palettes from Rust builtins.');}else await writeFile(out,text);
console.log('Verified 5 source palettes × light/dark × 24 colors.');
