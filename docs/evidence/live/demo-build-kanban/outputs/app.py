#!/usr/bin/env python3
import json, os
from http.server import BaseHTTPRequestHandler, HTTPServer

DATA = os.path.join(os.path.dirname(__file__), 'cards.json')

HTML = '''<!doctype html><html><head><meta charset="utf-8"><title>Kanban</title>
<style>
body{font-family:sans-serif;background:#f4f5f7;margin:20px}
.board{display:flex;gap:16px}
.col{background:#e3e5e8;border-radius:8px;padding:10px;width:260px;min-height:300px}
.col h2{margin:4px 0 10px;font-size:16px}
.card{background:#fff;border-radius:6px;padding:8px;margin-bottom:8px;box-shadow:0 1px 2px rgba(0,0,0,.2);cursor:pointer}
.card small{color:#888}
input{padding:6px;width:180px}
button{padding:6px 10px}
body.dark{background:#181a1f;color:#e8e8e8}
body.dark .col{background:#24262b}
body.dark .card{background:#33363d;color:#e8e8e8}
body.dark input{background:#24262b;color:#e8e8e8;border:1px solid #444}
#themeBtn{position:absolute;top:20px;right:20px}
</style></head><body>
<button id="themeBtn" onclick="toggleTheme()">&#127769; Dark</button>
<h1>Kanban</h1>
<div><input id="title" placeholder="Card title"><button onclick="addCard()">Add card</button></div>
<div class="board" id="board"></div>
<script>
const COLS=['todo','doing','done'];
const NAMES={todo:'To do',doing:'Doing',done:'Done'};
let cards=[];
async function load(){cards=await (await fetch('/api/cards')).json();render();}
async function save(){await fetch('/api/cards',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(cards)});}
function addCard(){const t=document.getElementById('title').value.trim();if(!t)return;cards.push({id:Date.now()+Math.random(),title:t,col:'todo'});document.getElementById('title').value='';save();render();}
function move(id){const c=cards.find(c=>c.id==id);if(!c)return;c.col=COLS[Math.min(COLS.indexOf(c.col)+1,2)];save();render();}
function delCard(id){cards=cards.filter(c=>c.id!=id);save();render();}
function toggleTheme(){document.body.classList.toggle('dark');const d=document.body.classList.contains('dark');document.getElementById('themeBtn').innerHTML=d?'&#9728; Light':'&#127769; Dark';localStorage.setItem('theme',d?'dark':'light');}
if(localStorage.getItem('theme')==='dark')document.body.classList.add('dark');
function render(){
 const b=document.getElementById('board');b.innerHTML='';
 for(const col of COLS){
  const d=document.createElement('div');d.className='col';d.id='col-'+col;
  d.innerHTML='<h2>'+NAMES[col]+'</h2>';
  for(const c of cards.filter(c=>c.col===col)){
   const el=document.createElement('div');el.className='card';el.id='card-'+c.title.replace(/\W/g,'-');
   el.innerHTML=c.title+' <button class="del" data-id="'+c.id+'">&times;</button><br><small>'+(col==='done'?'done':NAMES[COLS[COLS.indexOf(col)+1]]||'')+' &rarr;</small>';
   el.onclick=e=>{if(e.target.classList.contains('del')){delCard(c.id);e.stopPropagation();}else move(c.id);};d.appendChild(el);
  }
  b.appendChild(d);
 }
}
load();
</script></body></html>'''

class H(BaseHTTPRequestHandler):
    def _send(self, code, body, ctype='application/json'):
        data = body if isinstance(body, bytes) else json.dumps(body).encode()
        self.send_response(code)
        self.send_header('Content-Type', ctype)
        self.send_header('Content-Length', str(len(data)))
        self.end_headers()
        self.wfile.write(data)
    def do_GET(self):
        if self.path == '/api/cards':
            self._send(200, open(DATA).read().encode() if os.path.exists(DATA) else b'[]')
        elif self.path == '/':
            self._send(200, HTML.encode(), 'text/html')
        else:
            self._send(404, {'error': 'not found'})
    def do_POST(self):
        if self.path == '/api/cards':
            n = int(self.headers.get('Content-Length', 0))
            body = self.rfile.read(n)
            json.loads(body)  # validate
            open(DATA, 'wb').write(body)
            self._send(200, {'ok': True})
        else:
            self._send(404, {'error': 'not found'})
    def log_message(self, *a): pass

HTTPServer(('0.0.0.0', 8000), H).serve_forever()
