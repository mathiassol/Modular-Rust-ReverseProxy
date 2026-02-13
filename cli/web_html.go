// Embedded web dashboard HTML
package main

var webIndexHTML = `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Proxycache Dashboard</title>
<style>
*{margin:0;padding:0;box-sizing:border-box}
:root{
  --bg:#ffffff;--bg2:#f6f8fa;--bg3:#eef1f5;--border:#d1d9e0;
  --fg:#1f2328;--fg2:#656d76;--accent:#2563eb;--accent-light:#dbeafe;
  --green:#1a7f37;--green-bg:#dafbe1;--red:#cf222e;--red-bg:#ffebe9;
  --yellow:#9a6700;--yellow-bg:#fff8c5;
  --shadow:0 1px 3px rgba(0,0,0,.08);
}
body{font-family:'Segoe UI',system-ui,-apple-system,sans-serif;background:var(--bg);color:var(--fg);overflow:hidden;height:100vh}
.layout{display:flex;height:100vh}
.sidebar{width:210px;background:var(--bg2);border-right:1px solid var(--border);display:flex;flex-direction:column;flex-shrink:0}
.sidebar .logo{padding:20px 18px 16px;border-bottom:1px solid var(--border)}
.sidebar .logo h1{font-size:16px;font-weight:700;letter-spacing:-.3px}
.sidebar .logo span{color:var(--fg2);font-weight:400;font-size:11px;display:block;margin-top:2px}
.sidebar nav{flex:1;padding:6px 0}
.sidebar nav button{
  display:flex;align-items:center;gap:9px;width:100%;padding:8px 18px;
  background:none;border:none;color:var(--fg2);cursor:pointer;font-size:13px;text-align:left;
  transition:all .12s;border-left:3px solid transparent;
}
.sidebar nav button:hover{background:var(--bg3);color:var(--fg)}
.sidebar nav button.active{color:var(--accent);background:var(--accent-light);border-left-color:var(--accent);font-weight:600}
.sidebar nav button svg{width:16px;height:16px;flex-shrink:0;opacity:.7}
.sidebar nav button.active svg{opacity:1}
.sidebar .status-bar{padding:12px 18px;border-top:1px solid var(--border);font-size:11px;color:var(--fg2)}
.sidebar .status-bar .dot{display:inline-block;width:8px;height:8px;border-radius:50%;margin-right:5px}
.sidebar .status-bar .dot.on{background:var(--green)}
.sidebar .status-bar .dot.off{background:var(--red)}
.main{flex:1;overflow:hidden;position:relative}
.tab{display:none;height:100%;overflow-y:auto;padding:28px 32px}
.tab.active{display:block}
h2{font-size:17px;margin-bottom:16px;font-weight:600}
h3{font-size:14px;margin:20px 0 10px;font-weight:600;color:var(--fg2)}
.grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(160px,1fr));gap:12px;margin-bottom:24px}
.card{background:var(--bg2);border:1px solid var(--border);border-radius:8px;padding:14px;box-shadow:var(--shadow)}
.card .label{font-size:10px;color:var(--fg2);text-transform:uppercase;letter-spacing:.5px;margin-bottom:4px;font-weight:500}
.card .val{font-size:18px;font-weight:700}
.card .val.g{color:var(--green)}.card .val.r{color:var(--red)}.card .val.b{color:var(--accent)}.card .val.y{color:var(--yellow)}
.actions{display:flex;gap:8px;flex-wrap:wrap;margin-bottom:24px}
.btn{padding:6px 16px;border-radius:7px;border:1px solid var(--border);background:var(--bg);color:var(--fg);cursor:pointer;font-size:12.5px;font-weight:500;box-shadow:var(--shadow);transition:all .12s}
.btn:hover{border-color:var(--accent);color:var(--accent)}
.btn.primary{background:var(--accent);color:#fff;border-color:var(--accent)}.btn.primary:hover{opacity:.88}
.btn.danger{border-color:var(--red);color:var(--red)}.btn.danger:hover{background:var(--red-bg)}
.btn.warn{border-color:var(--yellow);color:var(--yellow)}.btn.warn:hover{background:var(--yellow-bg)}
.btn:disabled{opacity:.35;pointer-events:none}
.log-box{background:var(--bg2);border:1px solid var(--border);border-radius:8px;padding:14px;font-family:'Cascadia Code','Fira Code',monospace;font-size:11.5px;line-height:1.7;max-height:260px;overflow-y:auto;white-space:pre-wrap;color:var(--fg2)}
.proto-row{display:flex;align-items:center;gap:10px;padding:10px 14px;background:var(--bg2);border:1px solid var(--border);border-radius:8px;margin-bottom:8px}
.proto-dot{width:10px;height:10px;border-radius:50%;flex-shrink:0}
.proto-dot.on{background:var(--green)}.proto-dot.off{background:var(--red)}.proto-dot.warn{background:var(--yellow)}
.proto-name{font-weight:600;font-size:13px;min-width:80px}
.proto-detail{color:var(--fg2);font-size:12px}
.tbl{width:100%;border-collapse:collapse;font-size:12.5px;margin-bottom:20px}
.tbl th{text-align:left;padding:6px 10px;background:var(--bg3);border:1px solid var(--border);font-weight:600;font-size:11px;text-transform:uppercase;letter-spacing:.4px;color:var(--fg2)}
.tbl td{padding:6px 10px;border:1px solid var(--border)}
.tbl td.k{font-weight:500;color:var(--accent);width:180px}
.tbl tr:hover{background:var(--bg2)}
.badge{display:inline-block;padding:2px 8px;border-radius:4px;font-size:10.5px;font-weight:600}
.badge.on{background:var(--green-bg);color:var(--green)}.badge.off{background:var(--red-bg);color:var(--red)}
.mod-grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(220px,1fr));gap:10px;margin-bottom:20px}
.mod-card{background:var(--bg2);border:1px solid var(--border);border-radius:8px;padding:14px;cursor:pointer;transition:all .12s}
.mod-card:hover{border-color:var(--accent);box-shadow:0 2px 8px rgba(37,99,235,.08)}
.mod-card .mod-head{display:flex;justify-content:space-between;align-items:center;margin-bottom:8px}
.mod-card .mod-name{font-weight:600;font-size:13px}
.mod-card .mod-settings{font-size:11px;color:var(--fg2);line-height:1.5}
.toggle-sw{width:36px;height:20px;border-radius:10px;background:var(--bg3);border:1px solid var(--border);cursor:pointer;position:relative;transition:all .2s;flex-shrink:0}
.toggle-sw.on{background:var(--green);border-color:var(--green)}
.toggle-sw .knob{width:14px;height:14px;border-radius:50%;background:#fff;position:absolute;top:2px;left:2px;transition:all .2s;box-shadow:0 1px 2px rgba(0,0,0,.15)}
.toggle-sw.on .knob{left:18px}
.edit-overlay{display:none;position:fixed;top:0;left:0;right:0;bottom:0;background:rgba(0,0,0,.3);z-index:100;align-items:center;justify-content:center}
.edit-overlay.show{display:flex}
.edit-panel{background:var(--bg);border-radius:12px;padding:24px;min-width:340px;max-width:480px;box-shadow:0 12px 40px rgba(0,0,0,.15)}
.edit-panel h3{margin-top:0;margin-bottom:16px;font-size:15px;color:var(--fg)}
.edit-panel .field{margin-bottom:10px}
.edit-panel .field label{font-size:11px;color:var(--fg2);text-transform:uppercase;letter-spacing:.4px;display:block;margin-bottom:3px;font-weight:500}
.edit-panel .field input{width:100%;padding:7px 10px;background:var(--bg2);border:1px solid var(--border);border-radius:6px;color:var(--fg);font-size:13px;font-family:inherit}
.edit-panel .field input:focus{outline:none;border-color:var(--accent)}
.edit-panel .edit-actions{display:flex;gap:8px;margin-top:16px;justify-content:flex-end}
.dev-output{background:var(--bg2);border:1px solid var(--border);border-radius:8px;padding:14px;font-family:'Cascadia Code','Fira Code',monospace;font-size:11.5px;line-height:1.7;min-height:180px;white-space:pre-wrap;color:var(--fg2);overflow-y:auto}
::-webkit-scrollbar{width:5px}::-webkit-scrollbar-track{background:transparent}::-webkit-scrollbar-thumb{background:var(--border);border-radius:3px}
</style>
</head>
<body>
<div class="layout">
  <div class="sidebar">
    <div class="logo"><h1>Proxycache</h1><span>Dashboard</span></div>
    <nav>
      <button class="active" data-tab="overview" onclick="switchTab('overview')">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/></svg>
        Overview
      </button>
      <button data-tab="metrics" onclick="switchTab('metrics')">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="22 12 18 12 15 21 9 3 6 12 2 12"/></svg>
        Metrics
      </button>
      <button data-tab="config" onclick="switchTab('config')">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polygon points="12 2 2 7 12 12 22 7 12 2"/><polyline points="2 17 12 22 22 17"/><polyline points="2 12 12 17 22 12"/></svg>
        Config
      </button>
      <button data-tab="protocols" onclick="switchTab('protocols')">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/></svg>
        Protocols
      </button>
      <button data-tab="dev" onclick="switchTab('dev')">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="16 18 22 12 16 6"/><polyline points="8 6 2 12 8 18"/></svg>
        Development
      </button>
    </nav>
    <div class="status-bar"><div id="sidebar-status"><span class="dot off"></span>Checking...</div></div>
  </div>
  <div class="main">

    <!-- OVERVIEW TAB -->
    <div class="tab active" id="tab-overview">
      <h2>Proxy Control</h2>
      <div class="grid" id="overview-cards"></div>
      <div class="actions">
        <button class="btn primary" onclick="proxyAction('start')">&#9654; Start</button>
        <button class="btn danger" onclick="proxyAction('stop')">&#9632; Stop</button>
        <button class="btn warn" onclick="proxyAction('reload')">&#8635; Reload</button>
        <button class="btn" onclick="proxyAction('ping')">&#9881; Ping</button>
      </div>
      <h3>Quick Metrics</h3>
      <div class="grid" id="overview-metrics"></div>
      <h3>Protocols</h3>
      <div id="overview-protocols"></div>
      <h3 style="margin-top:24px">Logs</h3>
      <div class="log-box" id="log-box">Loading...</div>
    </div>

    <!-- METRICS TAB -->
    <div class="tab" id="tab-metrics">
      <h2>Metrics</h2>
      <h3>Requests</h3>
      <div class="grid" id="m-requests"></div>
      <h3>Bandwidth</h3>
      <div class="grid" id="m-bandwidth"></div>
      <h3>Latency</h3>
      <div class="grid" id="m-latency"></div>
      <h3>Connections</h3>
      <div class="grid" id="m-connections"></div>
      <h3>Connection Pool</h3>
      <div class="grid" id="m-pool"></div>
      <h3>Circuit Breaker</h3>
      <div class="grid" id="m-cb"></div>
      <h3>System</h3>
      <div class="grid" id="m-system"></div>
    </div>

    <!-- CONFIG TAB -->
    <div class="tab" id="tab-config">
      <h2>Server Configuration</h2>
      <div class="actions">
        <button class="btn" onclick="doVerifyWeb()">&#10003; Verify</button>
        <button class="btn warn" onclick="doRepairWeb()">&#9881; Repair</button>
      </div>
      <div id="verify-result"></div>
      <table class="tbl" id="server-table"><tbody></tbody></table>
      <h2>Modules</h2>
      <div class="mod-grid" id="mod-grid"></div>
    </div>

    <!-- PROTOCOLS TAB -->
    <div class="tab" id="tab-protocols">
      <h2>Protocol Support</h2>
      <div id="proto-detail"></div>
      <h3 style="margin-top:24px">TLS Configuration</h3>
      <div id="tls-detail"></div>
    </div>

    <!-- DEV TAB -->
    <div class="tab" id="tab-dev">
      <h2>Development</h2>
      <div class="actions">
        <button class="btn primary" onclick="devCompile()">&#9889; Compile Rust</button>
        <button class="btn warn" onclick="devReload()">&#8635; Full Reload</button>
      </div>
      <div class="dev-output" id="dev-output">Ready.</div>
    </div>
  </div>
</div>

<!-- Edit Overlay -->
<div class="edit-overlay" id="edit-overlay" onclick="if(event.target===this)closeEdit()">
  <div class="edit-panel" id="edit-panel"></div>
</div>

<script>
var modules=[], proxyStatus={}, metricsData={}, protocolsData={}, tlsData={}, serverData={};
var api=function(p,o){return fetch(p,o).then(function(r){return r.json()}).catch(function(){return {}})};

function switchTab(n){
  document.querySelectorAll('.tab').forEach(function(t){t.classList.remove('active')});
  document.querySelectorAll('.sidebar nav button').forEach(function(b){b.classList.remove('active')});
  document.getElementById('tab-'+n).classList.add('active');
  document.querySelector('[data-tab="'+n+'"]').classList.add('active');
}

function card(l,v,c){return '<div class="card"><div class="label">'+l+'</div><div class="val '+(c||'')+'">'+v+'</div></div>'}
function fmtB(b){if(!b||b===0)return '0 B';b=Number(b);if(b<1024)return b+' B';if(b<1048576)return (b/1024).toFixed(1)+' KB';if(b<1073741824)return (b/1048576).toFixed(1)+' MB';return (b/1073741824).toFixed(2)+' GB'}
function val(d,k){var v=d[k];return v!==undefined&&v!==null?v:'—'}

// ── Overview ──
function refreshOverview(){
  return api('/api/proxy/status').then(function(d){
    proxyStatus=d;
    var up=d.process_running||d.api_responding;
    document.getElementById('overview-cards').innerHTML=
      card('Status',up?'Running':'Stopped',up?'g':'r')+
      card('API',d.api_responding?'Online':'Offline',d.api_responding?'g':'r')+
      card('Uptime',d.uptime||'—','b')+
      card('PID',d.pid||'—','b')+
      card('Listen',d.listen||'—','')+
      card('Backend',d.backend||'—','');
    document.getElementById('sidebar-status').innerHTML=up
      ?'<span class="dot on"></span>Running (pid '+val(d,'pid')+')'
      :'<span class="dot off"></span>Stopped';
    document.getElementById('overview-metrics').innerHTML=
      card('Requests',val(d,'requests_total'),'b')+
      card('OK',val(d,'requests_ok'),'g')+
      card('Errors',val(d,'requests_err'),'r')+
      card('Bytes In',fmtB(d.bytes_in),'')+
      card('Bytes Out',fmtB(d.bytes_out),'')+
      card('Avg Latency',val(d,'avg_latency_ms')+'ms','y');
  });
}
function refreshProtoOverview(){
  return api('/api/proxy/protocols').then(function(d){
    protocolsData=d;
    var html='';
    html+=protoRow('HTTP/1.1',true,'Always enabled','TCP');
    var h2=d.http2||{};var h3=d.http3||{};
    html+=protoRow('HTTP/2',h2.enabled,'ALPN negotiation','TLS');
    html+=protoRow('HTTP/3',h3.enabled,'QUIC transport, port '+(h3.port||'—'),'UDP');
    document.getElementById('overview-protocols').innerHTML=html;
  });
}
function protoRow(name,en,detail,transport){
  var dot=en?'on':'off';
  return '<div class="proto-row"><div class="proto-dot '+dot+'"></div><div class="proto-name">'+name+'</div><div class="proto-detail">'+detail+'</div><div style="margin-left:auto;font-size:11px;color:var(--fg2)">'+transport+'</div></div>';
}
function proxyAction(a){
  api('/api/proxy/'+a,{method:'POST'}).then(function(r){
    if(a==='ping'&&r.alive!==undefined)alert(r.alive?'Pong! '+r.latency_ms+'ms':'Not responding');
    setTimeout(refreshAll,800);
  });
}
function refreshLogs(){
  return api('/api/proxy/logs').then(function(r){
    var b=document.getElementById('log-box');
    b.textContent=(r.logs||'').replace(/\x1b\[[0-9;]*m/g,'')||'No logs yet';
    b.scrollTop=b.scrollHeight;
  });
}

// ── Metrics ──
function refreshMetrics(){
  return api('/api/proxy/metrics').then(function(d){
    metricsData=d;
    document.getElementById('m-requests').innerHTML=
      card('Total',val(d,'requests_total'),'b')+card('OK',val(d,'requests_ok'),'g')+card('Errors',val(d,'requests_err'),'r');
    document.getElementById('m-bandwidth').innerHTML=
      card('Bytes In',fmtB(d.bytes_in),'b')+card('Bytes Out',fmtB(d.bytes_out),'b');
    document.getElementById('m-latency').innerHTML=
      card('Avg (ms)',d.requests_total>0?Math.round(d.latency_sum_ms/d.requests_total):'—','y')+
      card('Max (ms)',val(d,'latency_max_ms'),'r')+
      card('Sum (ms)',val(d,'latency_sum_ms'),'');
    document.getElementById('m-connections').innerHTML=
      card('Active',val(d,'active_connections'),'b')+card('Total Served',val(d,'connections_total'),'');
    document.getElementById('m-pool').innerHTML=
      card('Pool Hits',val(d,'pool_hits'),'g')+card('Pool Misses',val(d,'pool_misses'),'r')+
      card('Hit Rate',d.pool_hits+d.pool_misses>0?Math.round(d.pool_hits/(d.pool_hits+d.pool_misses)*100)+'%':'—','b');
    document.getElementById('m-cb').innerHTML=
      card('Trips',val(d,'cb_trips'),'y')+card('Rejects',val(d,'cb_rejects'),'r');
    document.getElementById('m-system').innerHTML=
      card('Uptime',val(d,'uptime_secs')+'s','');
  });
}

// ── Config ──
function refreshConfig(){
  return api('/api/proxy/server').then(function(d){
    serverData=d;
    var tb=document.querySelector('#server-table tbody');
    var html='<tr><th>Setting</th><th>Value</th></tr>';
    var keys=Object.keys(d).filter(function(k){return k!=='offline'}).sort();
    for(var i=0;i<keys.length;i++){
      var k=keys[i],v=d[k];
      var vc=typeof v==='boolean'?(v?'<span class="badge on">true</span>':'<span class="badge off">false</span>'):String(v);
      html+='<tr><td class="k">'+k+'</td><td>'+vc+'</td></tr>';
    }
    tb.innerHTML=html;
  });
}
function refreshModules(){
  return api('/api/config').then(function(data){
    modules=Array.isArray(data)?data:[];
    var grid=document.getElementById('mod-grid');
    var html='';
    for(var i=0;i<modules.length;i++){
      var m=modules[i];
      var settings=m.settings||{};
      var keys=Object.keys(settings).sort();
      var shtml='';
      for(var j=0;j<keys.length;j++){
        shtml+=keys[j]+' = '+settings[keys[j]]+'<br>';
      }
      if(!shtml)shtml='<em style="color:var(--fg2)">no settings</em>';
      html+='<div class="mod-card" onclick="openEdit(\''+m.name+'\','+m.is_server+')">';
      html+='<div class="mod-head"><span class="mod-name">'+m.name+'</span>';
      if(!m.is_server){
        html+='<div class="toggle-sw '+(m.enabled?'on':'')+'" onclick="event.stopPropagation();toggleMod(\''+m.name+'\')"><div class="knob"></div></div>';
      } else {
        html+='<span class="badge on">core</span>';
      }
      html+='</div><div class="mod-settings">'+shtml+'</div></div>';
    }
    grid.innerHTML=html;
  });
}
function toggleMod(name){
  api('/api/toggle/'+name,{method:'POST'}).then(function(){refreshModules()});
}
function openEdit(name,isServer){
  var mod=modules.find(function(m){return m.name===name});
  if(!mod)return;
  var panel=document.getElementById('edit-panel');
  var settings=isServer?Object.assign({},mod.settings):Object.assign({},mod.settings);
  var keys=Object.keys(settings).sort();
  var html='<h3>'+name+'</h3>';
  for(var i=0;i<keys.length;i++){
    var k=keys[i],v=String(settings[k]).replace(/"/g,'&quot;');
    html+='<div class="field"><label>'+k+'</label><input data-key="'+k+'" value="'+v+'"></div>';
  }
  html+='<div class="edit-actions"><button class="btn" onclick="closeEdit()">Cancel</button>';
  html+='<button class="btn primary" onclick="saveEdit(\''+name+'\')">Save</button></div>';
  panel.innerHTML=html;
  document.getElementById('edit-overlay').classList.add('show');
}
function closeEdit(){document.getElementById('edit-overlay').classList.remove('show')}
function saveEdit(name){
  var inputs=document.querySelectorAll('#edit-panel input');
  var u={};
  inputs.forEach(function(inp){
    var v=inp.value;
    if(v==='true')v=true;else if(v==='false')v=false;
    else if(/^\d+$/.test(v))v=parseInt(v);
    else if(/^\d+\.\d+$/.test(v))v=parseFloat(v);
    u[inp.dataset.key]=v;
  });
  api('/api/update/'+name,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(u)})
    .then(function(){closeEdit();refreshConfig();refreshModules()});
}
function doVerifyWeb(){
  var el=document.getElementById('verify-result');
  el.innerHTML='<div style="padding:8px;color:var(--fg2)">Verifying...</div>';
  api('/api/proxy/verify').then(function(d){
    if(d.ok){
      el.innerHTML='<div style="padding:8px;color:var(--green)">&#10003; Config is valid</div>';
    } else {
      var issues=(d.issues||[]).map(function(i){return '<div style="color:var(--yellow);padding:2px 0">&#8226; '+i+'</div>'}).join('');
      el.innerHTML='<div style="padding:8px"><div style="color:var(--red);margin-bottom:4px">&#10007; Issues found:</div>'+issues+'</div>';
    }
    setTimeout(function(){el.innerHTML=''},8000);
  });
}
function doRepairWeb(){
  api('/api/proxy/repair',{method:'POST'}).then(function(d){
    var el=document.getElementById('verify-result');
    if(d.ok){
      var fixes=(d.fixes||[]);
      if(fixes.length>0){
        el.innerHTML='<div style="padding:8px;color:var(--green)">&#10003; Repaired: '+fixes.join(', ')+'</div>';
      } else {
        el.innerHTML='<div style="padding:8px;color:var(--green)">&#10003; No repairs needed</div>';
      }
    } else {
      el.innerHTML='<div style="padding:8px;color:var(--red)">&#10007; '+(d.error||'Repair failed')+'</div>';
    }
    setTimeout(function(){el.innerHTML='';refreshConfig();refreshModules()},5000);
  });
}

// ── Protocols ──
function refreshProtocols(){
  api('/api/proxy/protocols').then(function(d){
    protocolsData=d;
    var tls=d.tls_enabled;
    var h1=d.http1||{};var h2=d.http2||{};var h3=d.http3||{};
    var html='';
    html+='<div class="proto-row"><div class="proto-dot on"></div><div class="proto-name">HTTP/1.1</div><div class="proto-detail">Always enabled — TCP transport — Port: '+(h1.port||'—')+'</div></div>';
    if(h2.enabled){
      html+='<div class="proto-row"><div class="proto-dot on"></div><div class="proto-name">HTTP/2</div><div class="proto-detail">ALPN: "'+val(h2,'alpn')+'" — Requires TLS — Multiplexed streams</div></div>';
    } else {
      html+='<div class="proto-row"><div class="proto-dot off"></div><div class="proto-name">HTTP/2</div><div class="proto-detail">'+(tls?'Disabled in config':'Requires TLS (not configured)')+'</div></div>';
    }
    if(h3.enabled){
      html+='<div class="proto-row"><div class="proto-dot on"></div><div class="proto-name">HTTP/3</div><div class="proto-detail">QUIC/UDP — Port: '+val(h3,'port')+' — 0-RTT capable</div></div>';
    } else {
      html+='<div class="proto-row"><div class="proto-dot off"></div><div class="proto-name">HTTP/3</div><div class="proto-detail">'+(tls?'Disabled in config':'Requires TLS (not configured)')+'</div></div>';
    }
    html+='<div class="proto-row"><div class="proto-dot '+(tls?'on':'off')+'"></div><div class="proto-name">TLS</div><div class="proto-detail">'+(tls?'Enabled — ALPN auto-negotiation':'Not configured')+'</div></div>';
    document.getElementById('proto-detail').innerHTML=html;
  });
  api('/api/proxy/tls').then(function(d){
    tlsData=d;
    var html='';
    if(d.enabled){
      html+='<table class="tbl"><tr><th>Setting</th><th>Value</th></tr>';
      html+='<tr><td class="k">Status</td><td><span class="badge on">Enabled</span></td></tr>';
      html+='<tr><td class="k">cert_path</td><td>'+val(d,'cert_path')+'</td></tr>';
      html+='<tr><td class="k">key_path</td><td>'+val(d,'key_path')+'</td></tr>';
      html+='<tr><td class="k">cert_exists</td><td>'+(d.cert_exists?'<span class="badge on">yes</span>':'<span class="badge off">no</span>')+'</td></tr>';
      html+='<tr><td class="k">key_exists</td><td>'+(d.key_exists?'<span class="badge on">yes</span>':'<span class="badge off">no</span>')+'</td></tr>';
      html+='<tr><td class="k">alpn_protocols</td><td>'+val(d,'alpn_protocols')+'</td></tr>';
      html+='<tr><td class="k">session_cache_size</td><td>'+val(d,'session_cache_size')+'</td></tr>';
      html+='</table>';
    } else {
      html+='<div class="proto-row"><div class="proto-dot off"></div><div class="proto-name">TLS</div><div class="proto-detail">Not configured — set tls_cert and tls_key in server config</div></div>';
    }
    document.getElementById('tls-detail').innerHTML=html;
  });
}

// ── Dev ──
function devCompile(){
  var o=document.getElementById('dev-output');o.textContent='Compiling Rust...\n';
  api('/api/proxy/compile',{method:'POST'}).then(function(r){
    o.textContent+=r.status==='success'?'\u2713 Build successful\n':'\u2717 Build failed: '+(r.error||'unknown')+'\n';
  });
}
function devReload(){
  var o=document.getElementById('dev-output');o.textContent='Reloading (stop \u2192 compile \u2192 start)...\n';
  api('/api/proxy/reload',{method:'POST'}).then(function(){
    o.textContent+='Reload started...\n';
    setTimeout(function(){refreshAll().then(function(){o.textContent+='Done.\n'})},5000);
  });
}

// ── Init ──
function refreshAll(){
  return Promise.all([refreshOverview(),refreshProtoOverview(),refreshLogs(),refreshMetrics(),refreshConfig(),refreshModules(),refreshProtocols()]);
}
refreshAll();
setInterval(function(){refreshOverview();refreshMetrics()},5000);
setInterval(refreshLogs,10000);
setInterval(function(){refreshProtoOverview();refreshProtocols()},15000);
</script>
</body>
</html>` + ""
