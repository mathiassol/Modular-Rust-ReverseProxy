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
.sidebar{width:230px;background:var(--bg2);border-right:1px solid var(--border);display:flex;flex-direction:column;flex-shrink:0;z-index:10}
.sidebar .logo{padding:22px 20px 18px;border-bottom:1px solid var(--border)}
.sidebar .logo h1{font-size:17px;font-weight:700;color:var(--fg);letter-spacing:-.3px}
.sidebar .logo span{color:var(--fg2);font-weight:400;font-size:12px;display:block;margin-top:2px}
.sidebar nav{flex:1;padding:8px 0}
.sidebar nav button{
  display:flex;align-items:center;gap:10px;width:100%;padding:9px 20px;
  background:none;border:none;color:var(--fg2);cursor:pointer;font-size:13.5px;text-align:left;
  transition:all .12s;border-left:3px solid transparent;
}
.sidebar nav button:hover{background:var(--bg3);color:var(--fg)}
.sidebar nav button.active{color:var(--accent);background:var(--accent-light);border-left-color:var(--accent);font-weight:600}
.sidebar nav button svg{width:17px;height:17px;flex-shrink:0;opacity:.7}
.sidebar nav button.active svg{opacity:1}
.sidebar .status-bar{padding:14px 20px;border-top:1px solid var(--border);font-size:12px;color:var(--fg2)}
.sidebar .status-bar .dot{display:inline-block;width:8px;height:8px;border-radius:50%;margin-right:6px}
.sidebar .status-bar .dot.on{background:var(--green)}
.sidebar .status-bar .dot.off{background:var(--red)}
.main{flex:1;overflow:hidden;position:relative;background:var(--bg)}

.tab{display:none;height:100%;width:100%}
.tab.active{display:flex;flex-direction:column}

/* Proxy Control */
.proxy-tab{padding:32px 36px;overflow-y:auto}
.proxy-tab h2{font-size:18px;margin-bottom:20px;font-weight:600;color:var(--fg)}
.proxy-grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(180px,1fr));gap:14px;margin-bottom:28px}
.proxy-card{background:var(--bg2);border:1px solid var(--border);border-radius:10px;padding:18px;transition:all .12s;box-shadow:var(--shadow)}
.proxy-card:hover{border-color:var(--accent);box-shadow:0 2px 8px rgba(37,99,235,.1)}
.proxy-card .label{font-size:11px;color:var(--fg2);text-transform:uppercase;letter-spacing:.6px;margin-bottom:6px;font-weight:500}
.proxy-card .value{font-size:20px;font-weight:700}
.proxy-card .value.green{color:var(--green)}
.proxy-card .value.red{color:var(--red)}
.proxy-card .value.blue{color:var(--accent)}
.proxy-actions{display:flex;gap:10px;flex-wrap:wrap;margin-bottom:28px}
.btn{
  padding:7px 18px;border-radius:8px;border:1px solid var(--border);
  background:var(--bg);color:var(--fg);cursor:pointer;font-size:13px;
  transition:all .12s;font-weight:500;box-shadow:var(--shadow);
}
.btn:hover{border-color:var(--accent);color:var(--accent)}
.btn.primary{background:var(--accent);color:#fff;border-color:var(--accent)}
.btn.primary:hover{opacity:.88}
.btn.danger{border-color:var(--red);color:var(--red)}
.btn.danger:hover{background:var(--red-bg)}
.btn.warn{border-color:var(--yellow);color:var(--yellow)}
.btn.warn:hover{background:var(--yellow-bg)}
.btn:disabled{opacity:.35;pointer-events:none}
.log-box{
  background:var(--bg2);border:1px solid var(--border);border-radius:10px;padding:16px;
  font-family:'Cascadia Code','Fira Code',monospace;font-size:12px;line-height:1.7;
  max-height:300px;overflow-y:auto;white-space:pre-wrap;color:var(--fg2);
}

/* Config hex grid */
.config-tab{position:relative;overflow:hidden;flex:1;width:100%;height:100%}
.hex-wrap{position:absolute;top:0;left:0;width:100%;height:100%}
.hex-svg{position:absolute;top:0;left:0;width:100%;height:100%;pointer-events:none;z-index:0}
.hex-nodes{position:absolute;top:0;left:0;width:100%;height:100%;z-index:1}
.hex-node{position:absolute;cursor:pointer;transition:transform .15s;user-select:none;-webkit-user-select:none}
.hex-node:hover{transform:scale(1.06)}
.hex-node svg polygon{transition:all .15s;filter:drop-shadow(0 2px 6px rgba(0,0,0,.08))}
.hex-node .hex-label{
  position:absolute;top:50%;left:50%;transform:translate(-50%,-55%);
  font-size:10.5px;font-weight:600;pointer-events:none;text-align:center;
  width:80px;line-height:1.25;
}
.hex-node .hex-status{
  position:absolute;bottom:18%;left:50%;transform:translateX(-50%);
  font-size:8px;pointer-events:none;text-transform:uppercase;letter-spacing:.8px;font-weight:600;
}
.hex-popup{
  position:absolute;background:var(--bg);border:1px solid var(--border);border-radius:12px;
  padding:20px;min-width:260px;z-index:100;box-shadow:0 8px 30px rgba(0,0,0,.12);
  display:none;
}
.hex-popup.show{display:block}
.hex-popup h3{font-size:14px;margin-bottom:14px;color:var(--fg);font-weight:600}
.hex-popup .toggle-row{display:flex;align-items:center;justify-content:space-between;margin-bottom:14px;padding-bottom:14px;border-bottom:1px solid var(--border)}
.hex-popup .toggle-label{font-size:13px;color:var(--fg2)}
.toggle-switch{
  width:40px;height:22px;border-radius:11px;background:var(--bg3);border:1px solid var(--border);
  cursor:pointer;position:relative;transition:all .2s;
}
.toggle-switch.on{background:var(--green);border-color:var(--green)}
.toggle-switch .knob{
  width:16px;height:16px;border-radius:50%;background:#fff;position:absolute;top:2px;left:2px;
  transition:all .2s;box-shadow:0 1px 3px rgba(0,0,0,.15);
}
.toggle-switch.on .knob{left:20px}
.hex-popup .field{margin-bottom:10px}
.hex-popup .field label{font-size:11px;color:var(--fg2);text-transform:uppercase;letter-spacing:.5px;display:block;margin-bottom:4px;font-weight:500}
.hex-popup .field input{
  width:100%;padding:7px 10px;background:var(--bg2);border:1px solid var(--border);
  border-radius:6px;color:var(--fg);font-size:13px;font-family:inherit;
}
.hex-popup .field input:focus{outline:none;border-color:var(--accent);box-shadow:0 0 0 3px rgba(37,99,235,.12)}
.hex-popup .save-btn{margin-top:14px}

/* Dev */
.dev-tab{padding:32px 36px;overflow-y:auto}
.dev-tab h2{font-size:18px;margin-bottom:20px;font-weight:600}
.dev-actions{display:flex;gap:10px;margin-bottom:20px}
.dev-output{
  background:var(--bg2);border:1px solid var(--border);border-radius:10px;padding:16px;
  font-family:'Cascadia Code','Fira Code',monospace;font-size:12px;line-height:1.7;
  min-height:200px;white-space:pre-wrap;color:var(--fg2);overflow-y:auto;flex:1;
}

::-webkit-scrollbar{width:6px}
::-webkit-scrollbar-track{background:transparent}
::-webkit-scrollbar-thumb{background:var(--border);border-radius:3px}
</style>
</head>
<body>
<div class="layout">
  <div class="sidebar">
    <div class="logo"><h1>Proxycache</h1><span>Dashboard</span></div>
    <nav>
      <button class="active" data-tab="proxy" onclick="switchTab('proxy')">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/></svg>
        Proxy Control
      </button>
      <button data-tab="config" onclick="switchTab('config')">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polygon points="12 2 2 7 12 12 22 7 12 2"/><polyline points="2 17 12 22 22 17"/><polyline points="2 12 12 17 22 12"/></svg>
        Config
      </button>
      <button data-tab="dev" onclick="switchTab('dev')">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="16 18 22 12 16 6"/><polyline points="8 6 2 12 8 18"/></svg>
        Development
      </button>
    </nav>
    <div class="status-bar">
      <div id="sidebar-status"><span class="dot off"></span>Checking...</div>
    </div>
  </div>

  <div class="main">
    <div class="tab active" id="tab-proxy">
      <div class="proxy-tab">
        <h2>Proxy Control</h2>
        <div class="proxy-grid" id="proxy-cards"></div>
        <div class="proxy-actions">
          <button class="btn primary" onclick="proxyAction('start')">&#9654; Start</button>
          <button class="btn danger" onclick="proxyAction('stop')">&#9632; Stop</button>
          <button class="btn warn" onclick="proxyAction('reload')">&#8635; Reload</button>
          <button class="btn" onclick="proxyAction('ping')">&#9881; Ping</button>
        </div>
        <h2>Logs</h2>
        <div class="log-box" id="log-box">Loading...</div>
      </div>
    </div>

    <div class="tab" id="tab-config">
      <div class="config-tab" id="config-canvas">
        <div class="hex-wrap">
          <svg class="hex-svg" id="hex-svg"></svg>
          <div class="hex-nodes" id="hex-nodes"></div>
        </div>
        <div class="hex-popup" id="hex-popup"></div>
      </div>
    </div>

    <div class="tab" id="tab-dev">
      <div class="dev-tab" style="display:flex;flex-direction:column;height:100%">
        <h2>Development</h2>
        <div class="dev-actions">
          <button class="btn primary" onclick="devCompile()">&#9889; Compile Rust</button>
          <button class="btn warn" onclick="devReload()">&#8635; Full Reload</button>
        </div>
        <div class="dev-output" id="dev-output">Ready.</div>
      </div>
    </div>
  </div>
</div>

<script>
var modules = [];
var proxyStatus = {};
var activePopup = null;

function switchTab(name) {
  document.querySelectorAll('.tab').forEach(function(t){t.classList.remove('active')});
  document.querySelectorAll('.sidebar nav button').forEach(function(b){b.classList.remove('active')});
  document.getElementById('tab-'+name).classList.add('active');
  document.querySelector('[data-tab="'+name+'"]').classList.add('active');
  if (name==='config') {
    var tries = 0;
    function tryRender() {
      var c = document.getElementById('config-canvas');
      if (c && c.clientWidth > 50 && c.clientHeight > 50) {
        renderHexGrid();
      } else if (tries < 20) {
        tries++;
        requestAnimationFrame(tryRender);
      }
    }
    requestAnimationFrame(tryRender);
  }
}

var api = function(path, opts) {
  return fetch(path, opts).then(function(r){return r.json()}).catch(function(){return {}});
};

// ── Proxy ──
function refreshProxyStatus() {
  return api('/api/proxy/status').then(function(data) {
    proxyStatus = data;
    var cards = document.getElementById('proxy-cards');
    var up = data.process_running || data.api_responding;
    cards.innerHTML =
      card('Status', up?'Running':'Stopped', up?'green':'red') +
      card('API', data.api_responding?'Online':'Offline', data.api_responding?'green':'red') +
      card('Uptime', data.uptime||'\u2014', 'blue') +
      card('PID', data.pid||'\u2014', 'blue');
    document.getElementById('sidebar-status').innerHTML = up
      ? '<span class="dot on"></span>Proxy running'
      : '<span class="dot off"></span>Proxy stopped';
  });
}
function card(l,v,c){return '<div class="proxy-card"><div class="label">'+l+'</div><div class="value '+c+'">'+v+'</div></div>'}

function proxyAction(action) {
  api('/api/proxy/'+action, {method:'POST'}).then(function(r) {
    if (action==='ping' && r.alive!==undefined) alert(r.alive?'Pong! '+r.latency_ms+'ms':'Not responding');
    setTimeout(refreshProxyStatus, 800);
  });
}
function refreshLogs() {
  return api('/api/proxy/logs').then(function(r) {
    var box = document.getElementById('log-box');
    box.textContent = (r.logs||'').replace(/\x1b\[[0-9;]*m/g,'') || 'No logs yet';
    box.scrollTop = box.scrollHeight;
  });
}

// ── Hex grid ──
var DIRS = [{q:1,r:0},{q:1,r:-1},{q:0,r:-1},{q:-1,r:0},{q:-1,r:1},{q:0,r:1}];

function hexRing(radius) {
  if (radius===0) return [{q:0,r:0}];
  var res = [];
  var q = DIRS[4].q * radius;
  var r = DIRS[4].r * radius;
  for (var d=0; d<6; d++) {
    for (var s=0; s<radius; s++) {
      res.push({q:q, r:r});
      q += DIRS[d].q;
      r += DIRS[d].r;
    }
  }
  return res;
}

function genPositions(count) {
  var pos = [{q:0, r:0}];
  var ring = 1;
  while (pos.length < count) {
    var rp = hexRing(ring);
    rp.sort(function(a,b) {
      if (b.q !== a.q) return b.q - a.q;
      return Math.abs(a.r) - Math.abs(b.r);
    });
    for (var i=0; i<rp.length; i++) {
      if (pos.length >= count) break;
      pos.push(rp[i]);
    }
    ring++;
  }
  return pos;
}

function axToPixel(q, r, sz) {
  var S3 = Math.sqrt(3);
  var gap = sz * 0.18;
  return { x: (sz + gap) * (S3*q + S3/2*r), y: (sz + gap) * 1.5 * r };
}

function hexPts(cx, cy, r) {
  var pts = [];
  for (var i=0; i<6; i++) {
    var a = Math.PI/3*i - Math.PI/6;
    pts.push((cx+r*Math.cos(a)).toFixed(1)+','+(cy+r*Math.sin(a)).toFixed(1));
  }
  return pts.join(' ');
}

function renderHexGrid() {
  var nodesEl = document.getElementById('hex-nodes');
  var svgEl = document.getElementById('hex-svg');
  var canvas = document.getElementById('config-canvas');
  var popup = document.getElementById('hex-popup');
  if (!nodesEl || !svgEl || !canvas) return;
  popup.classList.remove('show');
  activePopup = null;

  try {
    if (!modules.length) {
      nodesEl.innerHTML = '<div style="position:absolute;top:50%;left:50%;transform:translate(-50%,-50%);color:#656d76;font-size:14px">No config loaded</div>';
      svgEl.innerHTML = '';
      return;
    }

    var cw = canvas.clientWidth;
    var ch = canvas.clientHeight;
    if (cw < 50 || ch < 50) {
      nodesEl.innerHTML = '<div style="padding:20px;color:#656d76">Loading grid (canvas: '+cw+'x'+ch+')...</div>';
      return;
    }

    var axial = genPositions(modules.length);
    var pad = 80;
    var R = 56;
    var S3 = Math.sqrt(3);
    var pixels, bx0, bx1, by0, by1;

    for (; R >= 18; R -= 2) {
      pixels = [];
      for (var i=0; i<axial.length; i++) pixels.push(axToPixel(axial[i].q, axial[i].r, R));
      bx0 = Infinity; bx1 = -Infinity; by0 = Infinity; by1 = -Infinity;
      for (var i=0; i<pixels.length; i++) {
        var hw = S3 * R / 2;
        if (pixels[i].x - hw < bx0) bx0 = pixels[i].x - hw;
        if (pixels[i].x + hw > bx1) bx1 = pixels[i].x + hw;
        if (pixels[i].y - R < by0) by0 = pixels[i].y - R;
        if (pixels[i].y + R > by1) by1 = pixels[i].y + R;
      }
      if (bx1-bx0+pad*2 <= cw && by1-by0+pad*2 <= ch) break;
    }

    var ox = cw/2 - (bx0+bx1)/2;
    var oy = ch/2 - (by0+by1)/2;
    var HW = S3 * R;
    var HH = 2 * R;

    nodesEl.innerHTML = '';
    var lines = '';
    var cx0 = ox + pixels[0].x;
    var cy0 = oy + pixels[0].y;

    for (var i=0; i<pixels.length; i++) {
      var mod = modules[i];
      if (!mod) continue;
      var cx = ox + pixels[i].x;
      var cy = oy + pixels[i].y;

      if (i > 0) {
        var lc = mod.enabled ? '#93bbf0' : '#c8cdd3';
        lines += '<line x1="'+cx0+'" y1="'+cy0+'" x2="'+cx+'" y2="'+cy+'" stroke="'+lc+'" stroke-width="1.5" stroke-opacity="0.4" stroke-dasharray="4,3"/>';
      }

      var fill, lbl, stat;
      if (mod.is_server) {
        fill='#c7dbf5'; lbl='#1e40af'; stat='core';
      } else if (mod.enabled) {
        fill='#c6f0d2'; lbl='#15803d'; stat='on';
      } else {
        fill='#e8eaed'; lbl='#6b7280'; stat='off';
      }

      var el = document.createElement('div');
      el.className = 'hex-node';
      el.style.cssText = 'left:'+(cx-HW/2)+'px;top:'+(cy-HH/2)+'px;width:'+HW+'px;height:'+HH+'px;';
      el.innerHTML =
        '<svg width="'+HW+'" height="'+HH+'" viewBox="0 0 '+HW+' '+HH+'">'+
        '<polygon points="'+hexPts(HW/2,HH/2,R)+'" fill="'+fill+'" stroke="none"/>'+
        '</svg>'+
        '<div class="hex-label" style="color:'+lbl+'">'+mod.name.replace(/_/g,' ')+'</div>'+
        '<div class="hex-status" style="color:'+lbl+'">'+stat+'</div>';

      (function(m, px, py) {
        el.addEventListener('click', function(e) {
          e.stopPropagation();
          if (activePopup === m.name) {
            document.getElementById('hex-popup').classList.remove('show');
            activePopup = null;
          } else {
            showPopup(m, px, py, R);
          }
        });
      })(mod, cx, cy);

      nodesEl.appendChild(el);
    }

    svgEl.innerHTML = lines;
  } catch(err) {
    nodesEl.innerHTML = '<div style="padding:20px;color:#cf222e">Hex grid error: '+err.message+'</div>';
  }
}

function showPopup(mod, hx, hy, R) {
  var popup = document.getElementById('hex-popup');
  var canvas = document.getElementById('config-canvas');
  var html = '<h3>'+mod.name+'</h3>';

  if (!mod.is_server) {
    html += '<div class="toggle-row"><span class="toggle-label">Enabled</span>';
    html += '<div class="toggle-switch '+(mod.enabled?'on':'')+'" onclick="toggleHex(\''+mod.name+'\')"><div class="knob"></div></div></div>';
  }

  var keys = Object.keys(mod.settings||{}).sort();
  for (var i=0; i<keys.length; i++) {
    var k = keys[i];
    var v = String(mod.settings[k]).replace(/"/g,'&quot;');
    html += '<div class="field"><label>'+k+'</label><input data-key="'+k+'" data-mod="'+mod.name+'" value="'+v+'"></div>';
  }
  if (keys.length) html += '<button class="btn primary save-btn" onclick="saveHex(\''+mod.name+'\')">Save</button>';

  popup.innerHTML = html;

  var left = hx + R + 24;
  var top = hy - 50;
  if (left + 280 > canvas.clientWidth) left = hx - R - 280;
  if (top < 10) top = 10;
  if (top + 300 > canvas.clientHeight) top = Math.max(10, canvas.clientHeight - 310);

  popup.style.left = left+'px';
  popup.style.top = top+'px';
  popup.classList.add('show');
  activePopup = mod.name;
}

function toggleHex(name) {
  api('/api/toggle/'+name, {method:'POST'}).then(function(){
    return loadConfig();
  }).then(renderHexGrid);
}

function saveHex(name) {
  var inputs = document.querySelectorAll('#hex-popup input[data-mod="'+name+'"]');
  var updates = {};
  inputs.forEach(function(inp) {
    var val = inp.value;
    if (val==='true') val=true;
    else if (val==='false') val=false;
    else if (/^\d+$/.test(val)) val=parseInt(val);
    else if (/^\d+\.\d+$/.test(val)) val=parseFloat(val);
    updates[inp.dataset.key] = val;
  });
  api('/api/update/'+name, {
    method:'POST',
    headers:{'Content-Type':'application/json'},
    body:JSON.stringify(updates)
  }).then(function(){ return loadConfig() }).then(renderHexGrid);
}

// Close popup on background click
document.addEventListener('click', function(e) {
  if (!e.target.closest('.hex-node') && !e.target.closest('.hex-popup')) {
    var p = document.getElementById('hex-popup');
    if (p) { p.classList.remove('show'); activePopup=null; }
  }
});

// ── Dev ──
function devCompile() {
  var out = document.getElementById('dev-output');
  out.textContent = 'Compiling Rust...\n';
  api('/api/proxy/compile', {method:'POST'}).then(function(r) {
    out.textContent += r.status==='success' ? '\u2713 Build successful\n' : '\u2717 Build failed: '+(r.error||'unknown')+'\n';
  });
}
function devReload() {
  var out = document.getElementById('dev-output');
  out.textContent = 'Reloading (stop \u2192 compile \u2192 start)...\n';
  api('/api/proxy/reload', {method:'POST'}).then(function() {
    out.textContent += 'Reload started...\n';
    setTimeout(function(){ refreshProxyStatus().then(function(){ out.textContent += 'Done.\n' }) }, 5000);
  });
}

// ── Init ──
function loadConfig() {
  return api('/api/config').then(function(data) {
    modules = Array.isArray(data) ? data : [];
  });
}

function init() {
  loadConfig()
    .then(refreshProxyStatus)
    .then(refreshLogs);
  setInterval(refreshProxyStatus, 5000);
  setInterval(refreshLogs, 10000);
  window.addEventListener('resize', function() {
    if (document.getElementById('tab-config').classList.contains('active')) renderHexGrid();
  });
}
init();
</script>
</body>
</html>` + ""
