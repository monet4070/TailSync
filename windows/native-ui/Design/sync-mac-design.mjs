import { readFile, writeFile } from 'node:fs/promises';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
const base=resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const source=async p=>readFile(resolve(base,p),'utf8');
const files=['TailSyncApp.swift','Models/Theme.swift','Services/Loc.swift','Views/HistoryFilterBar.swift','Views/HistoryRow.swift','Views/SettingsView.swift','Views/SettingsLayout.swift','Views/SettingsGeneralSection.swift','Views/SettingsUpdatesSection.swift','Views/SettingsStorageSection.swift','Views/SettingsNetworkSection.swift','Views/SettingsThemeSection.swift','Views/ConnectionsView.swift','Views/SettingsRemotePairingSection.swift'];
const sources={};let loc;
for(const file of files){const path=`macos/swift-ui/Sources/TailSync/${file}`,text=await source(path);sources[path]=createHash('sha256').update(text).digest('hex');if(file==='Services/Loc.swift')loc=text;}
const chinese=loc.slice(loc.indexOf('"zh-CN": ['));
const strings=Object.fromEntries([...chinese.matchAll(/^\s*"([^"\n]+)":\s*("(?:\\.|[^"\\])*")/gm)].map(m=>[m[1],JSON.parse(m[2])]));
const guide=JSON.parse(await source('shared/onboarding/guide.json'));
strings['onboarding.title']=guide.ui.title['zh-CN'];strings['onboarding.reopen']=guide.ui.reopen['zh-CN'];
const row=(id,type='switch',extra={})=>({id,type,title:`settings.${id}`,description:`settings.${id}Description`,...extra});
const sections=[
 {id:'general',rows:[row('syncEnabled','switch',{value:true}),row('launchAtLogin','switch',{value:true}),row('syncShortcut','shortcut',{keys:['⌥',"'"]}),row('historyShortcut','shortcut',{keys:['⌥','/']}),row('notifications','switch',{value:true}),row('progressBar','switch',{value:true})]},
 {id:'updates',rows:[row('currentVersion','update',{description:'settings.updateReady'})]},
 {id:'history',rows:[row('limit','slider',{value:100,min:10,max:500,step:10}),row('autoSaveNetworkImages','switch',{value:false}),row('thirdPartyImages','disclosure',{rows:[row('qqImageAccess','switch',{value:false}),row('wechatImageAccess','automatic')]})]},
 {id:'storage',rows:[row('storageLocation','storage',{title:'settings.storage',description:'settings.storageChange'}),row('storageQuota','stepper',{value:10,min:1,max:16384})]},
 {id:'manageConnections',rows:[row('manageConnections','connections')]},
 {id:'appearance',rows:[row('theme','appearance'),row('colorTheme','themes'),row('language','language')]}
];
// Key names are validated against the actual Mac catalogue instead of silently
// introducing near-identical Windows copy.
for(const section of sections)for(const r of section.rows){if(r.id==='historyShortcut'){r.title='settings.historyShortcut';r.description='settings.historyShortcutDescription';}}
const missing=[];for(const s of sections){if(!strings[`settings.${s.id}`])missing.push(`settings.${s.id}`);for(const r of s.rows){if(!strings[r.title]&&r.type!=='storage')missing.push(r.title);}}
if(missing.length)throw Error(`Missing Mac labels: ${missing.join(', ')}`);
const builtinSource=await source('shared/tailsync-themes/src/builtins.rs');
sources['shared/tailsync-themes/src/builtins.rs']=createHash('sha256').update(builtinSource).digest('hex');
const themeIds=['CANVAS','FLUX','LEDGER','AURA','MONO'];
const palettes=themeIds.map(id=>{const block=builtinSource.split(`(${id}_ID, false) => (`)[1]?.split('),')[0];if(!block)throw Error(`Missing ${id} palette`);const colors=[...block.matchAll(/"([^"]+)"/g)].map(m=>m[1]);return {accent:colors[0],canvas:colors[4],surface:colors[5]};});
const data={reference:'macOS Canvas / supplied 2x screenshots',sources,windows:{history:{width:400,height:600,minWidth:300,minHeight:360,title:'History'},favorites:{width:400,height:600,minWidth:300,minHeight:360,title:'Favorites'},settings:{width:540,height:500,minWidth:380,minHeight:400,title:'Settings'},connections:{width:520,height:640,minWidth:440,minHeight:500,title:strings['connections.title']}},metrics:{caption:32,filterHeight:88,searchHeight:36,filterControlHeight:28,filterGap:8,filterPadding:8,settingsRowPadding:13,cardRadius:10,controlRadius:9,sectionTitle:23,searchFont:18,historyFont:13},strings,sections,themes:['canvas','flux','ledger','aura','mono'].map((id,i)=>({id,name:strings['settings.colorTheme.'+['tailsync','ocean','forest','rose','high-contrast'][i]],...palettes[i]}))};
const output=resolve(dirname(fileURLToPath(import.meta.url)),'mac-ui-manifest.generated.json');
const next=JSON.stringify(data,null,2)+'\n';
if(process.argv.includes('--check')){if(await readFile(output,'utf8')!==next)throw Error('Mac UI sources changed. Run sync-mac-design.mjs and review the presentation mapping.');}else await writeFile(output,next);
console.log(`${sections.length} sections, ${Object.keys(strings).length} Mac labels; design ${process.argv.includes('--check')?'verified':'generated'}.`);
