// Development-only visual harness. Production builds do not include this entry.
import React from "react";
import { createRoot } from "react-dom/client";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { Connections } from "../src/pages/Connections";
import { Settings } from "../src/pages/Settings";
import { History } from "../src/pages/History";
import { readTour, saveTour } from "../src/onboarding/tourState";
import { demoHistoryPage } from "../src/onboarding/demo";
import guide from "../../shared/onboarding/guide.json";
import "../src/index.css";
import "../../shared/art-direction.css";

const settings = { auto_save_network_images:false, connection_mode:"auto", sync_enabled:true, history_limit:100, storage_quota_bytes:10737418240, language:"zh-CN", sync_shortcut:"Ctrl+Shift+S", history_shortcut:"Ctrl+Shift+H", notifications_enabled:true, progress_bar_enabled:true, storage_root:null, paired_peer_endpoints:{}, trusted_peer_addresses:{}, trusted_peer_keys:{}, enabled_peers:{} };
const pairing = {pairing_enabled:false,remaining_seconds:0,failed_attempts:0,max_failures:5,phase:"disabled",expires_at:null,peer:null,error:null,pending:[],pending_store_unavailable:false};
const peers = {self:{hostname:"DESKTOP · 示例设备",fingerprint:"预览数据 · 不连接实际设备",connection_mode:"auto",public_key:"demo",tailscale_ip:"",iroh_endpoint_id:null,routes:[{interface:"lan",address:"192.0.2.10:19890",status:"online",online:true,connected:false,latency_ms:null,pairing_endpoint:false,rtt_capable:true}]},peers:[],paired_peer_endpoints:{},discovery_error:null};
const root = createRoot(document.getElementById("root")!);
function render(name = "connections") { root.render(name === "settings" ? <Settings /> : name === "history" ? <History /> : <Connections />); }
mockWindows("connections");
mockIPC((cmd, args) => {
  if(cmd === "get_settings") return settings;
  if(cmd === "update_settings") {Object.assign(settings,JSON.parse(String(args?.settingsJson)));return settings;}
  if(cmd === "get_peers" || cmd === "refresh_peers") return peers;
  if(cmd === "get_pairing_status") return pairing;
  if(cmd === "enable_pairing") {pairing.pairing_enabled=true;pairing.phase="waiting";pairing.remaining_seconds=120;return pairing;}
  if(cmd === "cancel_pairing") {pairing.pairing_enabled=false;pairing.remaining_seconds=0;return pairing;}
  if(cmd === "take_pending_remote_pairing_link") return null;
  if(cmd === "get_remote_pairing_invite_status") return null;
  if(cmd === "get_local_theme_settings") return {activeThemeId:"builtin:canvas@1",appearance:"light",highContrast:false};
  if(cmd === "resolve_theme") return {tokens:{colors:{background:{canvas:"#f7f7f6",surface:"#fff"},text:{primary:"#252525",secondary:"#686868"},accent:{primary:"#7760c4"}}},assetSlots:{}};
  if(cmd === "list_themes_v2") return [];
  if(cmd === "get_storage_status") return {root:"示例存储目录",used_bytes:1048576,quota_bytes:10737418240,available:true,error:null};
  if(cmd === "get_update_status") return {current_version:"2.3.2",updates_enabled:false};
  if(cmd === "get_history_page") return demoHistoryPage({keyword:String(args?.keyword ?? "") || null,category:String(args?.category ?? "") || null,startTime:null,endTime:null,limit:Number(args?.limit ?? 30),offset:Number(args?.offset ?? 0),collection:args?.collection === "favorites" ? "favorites" : "all"});
  if(cmd === "get_history_capabilities") return {classifier_version:1,categories:["text","image","file"],multiple_labels:true,date_range_filter:true};
  if(cmd === "get_migration_diagnostics") return {unresolved_count:0};
  if(cmd === "wait_runtime_snapshot") return new Promise(() => {});
  if(cmd === "get_version") return {version:0};
  if(cmd === "get_file_progress") return {active:false,name:"",sent:0,total:0};
  if(cmd === "open_settings_window" || cmd === "open_connections_window") { render(cmd.includes("settings") ? "settings" : "connections"); return null; }
  if(cmd === "open_history_window") {render("history");return null;}
  if(cmd === "open_onboarding_window") {render("connections");return null;}
  if(["restore_entry", "open_preview_window", "open_favorites_window", "create_remote_pairing_invite", "start_remote_pairing"].includes(cmd)) throw new Error("浏览器仅预览界面，请在 TailSync 客户端执行此操作。");
  if(cmd.startsWith("plugin:")) return null;
  return null;
}, {shouldMockEvents:true});
localStorage.setItem("tailsync-lang","zh-CN");
if (!readTour().active) saveTour({active:true,index:0});
render(guide.steps[readTour().index].window);
