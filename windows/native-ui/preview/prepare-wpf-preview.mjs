import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {dirname,resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
const here=dirname(fileURLToPath(import.meta.url));
const template=await readFile(resolve(here,'template.html'),'utf8');
const design=JSON.parse(await readFile(resolve(here,'../Design/mac-ui-manifest.generated.json'),'utf8'));
const palettes=JSON.parse(await readFile(resolve(here,'../Design/wpf-palettes.generated.json'),'utf8'));
const records=JSON.parse(await readFile(resolve(here,'../DemoData/history.json'),'utf8'));
const landscape=await readFile(resolve(here,'../TailSync.Native/Assets/Landscape.svg'),'utf8');
let fragment=template.replace('__MAC_DESIGN__',JSON.stringify(design).replaceAll('<','\\u003c')).replace('__HISTORY_DATA__',JSON.stringify(records).replaceAll('<','\\u003c')).replace('__LANDSCAPE_DATA_URI__',`data:image/svg+xml;base64,${Buffer.from(landscape).toString('base64')}`);
fragment=fragment.replaceAll('tailsync-native-preview','tailsync-wpf-preview').replaceAll('mac-parity','wpf-reference').replace("design:'macOS source parity'","design:'WPF source-based design reference'").replace('aria-label="TailSync macOS 界面对齐预览"','aria-label="TailSync WPF 备选 · 目标外观预览，非 Windows 实机截图"');
// The design reference uses the same full light/dark colors as the WPF resources.
const paletteCode=`const wpfPalettes=${JSON.stringify(palettes)};function applyWpfPalette(){const p=wpfPalettes[state.colorTheme||'canvas'];for(const [css,key] of Object.entries({canvas:'Canvas',surface:'Surface',input:'Input',text:'Primary',secondary:'Secondary',tertiary:'Tertiary',accent:'Accent','accent-soft':'AccentSoft',border:'Border',divider:'Divider',positive:'Positive','on-accent':'OnAccent'}))root.style.setProperty('--tsm-'+css,'light-dark('+p.light[key]+','+p.dark[key]+')');const fonts={canvas:'"STZhongsong","Songti SC",SimSun,Georgia,serif',flux:'Bahnschrift,DengXian,"Segoe UI",sans-serif',ledger:'"STZhongsong","Songti SC",SimSun,Georgia,serif',aura:'Candara,YouYuan,"Microsoft YaHei UI",sans-serif',mono:'"Cascadia Mono","Cascadia Code",Consolas,monospace'};root.style.setProperty('--tsm-display',fonts[state.colorTheme||'canvas']);}`;
fragment=fragment.replace(' const prior=',paletteCode+'\n const prior=');
fragment=fragment.replace('function applyTheme(theme,save=true){','function applyTheme(theme,save=true){applyWpfPalette();');
fragment=fragment.replace("root.style.setProperty('--tsm-accent',\`light-dark(\${theme.accent},#ec8668)\`);return;","root.style.setProperty('--tsm-accent',`light-dark(${theme.accent},#ec8668)`);applyWpfPalette();return;");
fragment=fragment.replace('root.dataset.theme=state.theme;render();','root.dataset.theme=state.theme;applyWpfPalette();render();').replaceAll('"Songti SC",SimSun,serif','var(--tsm-display)');
const marker='<div class="nav nav-pills"';
fragment=fragment.replace(marker,'<div class="tsm-review-label">WPF 备选 · 目标外观预览 · 非 Windows 实机截图</div>\n  '+marker);
fragment=fragment.replace('<style>','<style>\n#tailsync-wpf-preview .tsm-review-label{color:var(--tsm-secondary);font-size:12px;margin:0 0 12px;}');
if(/__(MAC_DESIGN|HISTORY_DATA|LANDSCAPE_DATA_URI)__/.test(fragment))throw Error('Unresolved template data');
await writeFile(resolve(here,'tailsync-wpf.html'),fragment);
const index=process.argv.indexOf('--inline-output');if(index>=0){const output=resolve(process.argv[index+1]);await mkdir(dirname(output),{recursive:true});await writeFile(output,fragment);}
console.log('Prepared WPF design reference from the shared Mac layout and demo fixture.');
