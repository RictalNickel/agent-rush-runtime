const {test,expect}=require('@playwright/test');
// The real control plane serves the page, its headers (CSP, Permissions-Policy) and the
// operator login. QA has no worker, so the /v1 session API is served by an in-test fake.
const PASSWORD='qa-local-password-please-ignore';
// A 640x360 stand-in for a browser screenshot: a light page with a dark header, a card and a button.
const PNG=(()=>{const zlib=require('node:zlib');const w=640,h=360,raw=Buffer.alloc((w*3+1)*h);
  for(let y=0;y<h;y++)for(let x=0;x<w;x++){const header=y<48,card=x>=64&&x<576&&y>=96&&y<296,button=x>=96&&x<224&&y>=232&&y<264;
    const [r,g,b]=header?[32,40,52]:button?[37,99,235]:card?[255,255,255]:[241,243,246];const o=y*(w*3+1)+1+x*3;raw[o]=r;raw[o+1]=g;raw[o+2]=b;}
  const table=[...Array(256)].map((_,n)=>{let c=n;for(let k=0;k<8;k++)c=c&1?0xedb88320^(c>>>1):c>>>1;return c>>>0;});
  const crc=buf=>{let c=0xffffffff;for(const byte of buf)c=table[(c^byte)&255]^(c>>>8);return (c^0xffffffff)>>>0;};
  const chunk=(type,data)=>{const len=Buffer.alloc(4),sum=Buffer.alloc(4),body=Buffer.concat([Buffer.from(type),data]);len.writeUInt32BE(data.length);sum.writeUInt32BE(crc(body));return Buffer.concat([len,body,sum]);};
  const ihdr=Buffer.alloc(13);ihdr.writeUInt32BE(w,0);ihdr.writeUInt32BE(h,4);ihdr[8]=8;ihdr[9]=2;
  return Buffer.concat([Buffer.from([137,80,78,71,13,10,26,10]),chunk('IHDR',ihdr),chunk('IDAT',zlib.deflateSync(raw)),chunk('IEND',Buffer.alloc(0))]);})();
function wav(){const samples=4800,data=Buffer.alloc(samples*2),h=Buffer.alloc(44);h.write('RIFF',0);h.writeUInt32LE(36+data.length,4);h.write('WAVEfmt ',8);h.writeUInt32LE(16,16);h.writeUInt16LE(1,20);h.writeUInt16LE(1,22);h.writeUInt32LE(48000,24);h.writeUInt32LE(96000,28);h.writeUInt16LE(2,32);h.writeUInt16LE(16,34);h.write('data',36);h.writeUInt32LE(data.length,40);return Buffer.concat([h,data]);}

function watchErrors(page,ignore=[]){
  const errors=[];
  page.on('pageerror',error=>errors.push(error.message));
  page.on('console',message=>{const text=message.text();if(message.type()==='error'&&!text.includes('401 (Unauthorized)')&&!ignore.some(i=>text.includes(i)))errors.push(text);});
  return errors;
}

async function signIn(page){
  await page.getByRole('button',{name:'Sign in',exact:true}).click();
  await page.getByLabel('Operator password').fill(PASSWORD);
  await page.locator('#login-form').getByRole('button',{name:'Sign in'}).click();
  await expect(page.getByRole('button',{name:'Sign out',exact:true})).toBeVisible();
}

// A fake /v1 backend with a session event log. `sse:false` makes the stream fail so the
// page has to fall back to polling /events. `throttleImages` refuses the first request for
// each screenshot with 429, the way the server does past its per-session limit.
async function fakeApi(page,{voice=false,sse=true,seed=[],throttleImages=false}={}){
  const st={sessions:[],events:new Map(),csrf:[],deletes:[],purges:[],streams:0,polls:0,speak:0,transcribe:0,cancelled:0,images:{inFlight:0,maxInFlight:0,requests:{}}};
  const emit=(id,kind,payload)=>{const log=st.events.get(id);const event={session_id:id,seq:log.length+1,kind,payload,at:Date.now()};log.push(event);st.sessions.find(s=>s.id===id).last_seq=event.seq;return event;};
  const status=(id,value,reason)=>{st.sessions.find(s=>s.id===id).status=value;emit(id,'status',reason?{status:value,reason}:{status:value});};
  st.emit=emit;st.status=status;
  const create=(id,title)=>{const s={id,title,status:'idle',box_id:null,created_at:Date.now(),updated_at:Date.now(),spend_usd:0,last_seq:0};st.sessions.unshift(s);st.events.set(id,[]);return s;};
  for(const s of seed){create(s.id,s.title);for(const [kind,payload] of s.events)emit(s.id,kind,payload);st.sessions[0].status=s.status||'idle';}
  // The first turn: a shell step, a browser visit and a screenshot, still running.
  const firstTurn=id=>{
    status(id,'running');
    emit(id,'tool_call',{tool:'bash',call:{tool:'bash',command:'npm test'}});
    emit(id,'tool_result',{tool:'bash',ok:true,output:'exit code 0\nstdout:\n  2 passing (14ms)\n',truncated:false});
    emit(id,'tool_call',{tool:'browser',call:{tool:'browser',action:'goto',url:'https://example.com/'}});
    emit(id,'tool_result',{tool:'browser',ok:true,output:'exit code 0\nstdout:\n{"url":"https://example.com/","title":"Example Domain"}',truncated:false});
    emit(id,'tool_call',{tool:'browser',call:{tool:'browser',action:'screenshot'}});
    emit(id,'tool_result',{tool:'browser',ok:true,output:'exit code 0\nstdout:\n{"path":"/workspace/.screenshots/shot-1.png"}',truncated:false});
  };
  // Every screenshot request holds for 120 ms, so overlapping requests show up in maxInFlight.
  const serveShot=async(route,p)=>{
    const images=st.images;
    images.requests[p]=(images.requests[p]??0)+1;
    images.inFlight+=1;images.maxInFlight=Math.max(images.maxInFlight,images.inFlight);
    try{
      await new Promise(resolve=>setTimeout(resolve,120));
      if(throttleImages&&images.requests[p]===1)return await route.fulfill({status:429,json:{error:'too many workspace calls'}});
      return await route.fulfill({status:200,headers:{'content-type':'image/png','content-disposition':'attachment; filename="shot-1.png"'},body:PNG});
    }finally{images.inFlight-=1;}
  };
  await page.route(/\/v1\//,async route=>{
    const req=route.request(),url=new URL(req.url()),path=url.pathname,method=req.method();
    if(method!=='GET')st.csrf.push(req.headers()['x-csrf-token']||'');
    const match=/^\/v1\/sessions\/([^/]+)(\/.*)?$/.exec(path);
    const id=match&&decodeURIComponent(match[1]),sub=match?.[2]||'';
    if(path==='/v1/status')return route.fulfill({json:{provider:'vultr',worker_connected:true,sessions_available:true,voice_available:voice,mcp_servers:[]}});
    if(path==='/v1/sessions'&&method==='GET')return route.fulfill({json:{sessions:st.sessions}});
    if(path==='/v1/sessions'&&method==='POST'){
      const {message}=req.postDataJSON();const s=create(`s${st.sessions.length+1}`,message.slice(0,80));
      emit(s.id,'user',{text:message});firstTurn(s.id);
      return route.fulfill({status:201,json:{session:{...s},sent:{interjection:false,seq:1}}});
    }
    if(path==='/v1/sessions/purge-closed'&&method==='POST'){
      const gone=st.sessions.filter(s=>s.status==='closed');
      st.sessions=st.sessions.filter(s=>s.status!=='closed');
      for(const s of gone)st.events.delete(s.id);
      st.purges.push(gone.map(s=>s.id));
      return route.fulfill({json:{deleted:gone.length}});
    }
    if(match&&!sub&&method==='DELETE'){
      const s=st.sessions.find(x=>x.id===id);
      if(!s)return route.fulfill({status:404,json:{error:'Session not found'}});
      if(url.searchParams.get('purge')!=='true'){status(id,'closed');return route.fulfill({json:{closed:true}});}
      st.sessions=st.sessions.filter(x=>x.id!==id);st.events.delete(id);st.deletes.push(id);
      return route.fulfill({json:{closed:true,deleted:true}});
    }
    if(sub==='/messages'){
      const {text}=req.postDataJSON();const s=st.sessions.find(x=>x.id===id);
      if(s.status==='running'){
        const e=emit(id,'interjection',{text});
        emit(id,'tool_call',{tool:'edit_file',call:{tool:'edit_file',path:'src/app.py',old:'routes = []',new:'routes = [health]'}});
        return route.fulfill({status:202,json:{interjection:true,seq:e.seq}});
      }
      const e=emit(id,'user',{text});status(id,'running');
      emit(id,'tool_call',{tool:'convert_file',call:{tool:'convert_file',input:'report.md',output:'report.pdf'}});
      emit(id,'tool_result',{tool:'convert_file',ok:true,output:'exit code 0\nstdout:\nreport.pdf',truncated:false});
      emit(id,'assistant',{text:'Done. The **report** is attached.\n\n- Converted `report.md` to `report.pdf`\n- Source: [example.com](https://example.com/)\n- Preview: [Open the preview](javascript:alert(1))',attachments:['report.pdf','/workspace/.screenshots/shot-1.png']});
      status(id,'idle','replied');
      return route.fulfill({status:202,json:{interjection:false,seq:e.seq}});
    }
    if(sub==='/cancel'){
      st.cancelled+=1;
      emit(id,'tool_result',{tool:'edit_file',ok:false,output:'turn cancelled',truncated:false});
      status(id,'idle','cancelled');
      return route.fulfill({json:{cancelled:true}});
    }
    if(sub==='/stream'){
      st.streams+=1;
      if(!sse)return route.fulfill({status:502,json:{error:'stream unavailable'}});
      const after=Number(req.headers()['last-event-id']??url.searchParams.get('after')??0);
      const body='retry: 150\n\n'+st.events.get(id).filter(e=>e.seq>after).map(e=>`id: ${e.seq}\nevent: ${e.kind}\ndata: ${JSON.stringify(e)}\n\n`).join('');
      return route.fulfill({status:200,headers:{'content-type':'text/event-stream'},body});
    }
    if(sub==='/events'){
      st.polls+=1;
      const after=Number(url.searchParams.get('after')||0);const events=st.events.get(id).filter(e=>e.seq>after);
      return route.fulfill({json:{events,next_after:events.length?events[events.length-1].seq:after,more:false}});
    }
    if(sub==='/files'){
      const p=url.searchParams.get('path');
      if(p==='.')return route.fulfill({json:{path:'.',entries:[{name:'report.pdf',is_dir:false,size:2048},{name:'app',is_dir:true,size:0}]}});
      if(p==='app')return route.fulfill({json:{path:'app',entries:[{name:'main.py',is_dir:false,size:312}]}});
      if(p.endsWith('.png'))return serveShot(route,p);
      return route.fulfill({status:200,headers:{'content-type':'application/pdf','content-disposition':`attachment; filename="${p.split('/').pop()}"`},body:'%PDF-1.4\n%fake\n'});
    }
    if(path==='/v1/voice/speak'){st.speak+=1;return route.fulfill({status:200,headers:{'content-type':'audio/wav'},body:wav()});}
    if(path==='/v1/voice/transcribe'){st.transcribe+=1;expect(req.headers()['content-type']).toContain('multipart/form-data');return route.fulfill({json:{text:'Read me the result'}});}
    return route.fulfill({status:404,json:{error:`not faked: ${method} ${path}`}});
  });
  return st;
}

test('session chat: send, tool rows, screenshot, interjection, stop, reply, files',async({page})=>{
  const errors=watchErrors(page);
  const api=await fakeApi(page);
  await page.goto('/');await expect(page).toHaveTitle('Vultr-Placeholder-Test123');
  await expect(page.getByRole('heading',{level:1})).toHaveText('Give an agent its own sealed Linux workspace.');
  expect(await page.evaluate(()=>getComputedStyle(document.body).backgroundColor)).toBe('rgb(18, 22, 27)');
  await expect(page.locator('#deploy-label')).toHaveText('Vultr Serverless Inference');
  await expect(page.locator('#deploy-status')).toHaveText('Worker unavailable');
  await expect(page.getByLabel('Message the agent')).toBeDisabled();
  expect(await page.evaluate(async()=>{await document.fonts.ready;return document.fonts.check('16px Archivo');})).toBe(true);
  await page.getByRole('button',{name:'Safety'}).click();
  await expect(page.getByText('Authority stays outside the agent.',{exact:false})).toBeVisible();
  await page.getByRole('button',{name:'Close safety information'}).click();
  await signIn(page);
  await expect(page.getByRole('heading',{level:1})).toHaveText('What should the agent work on?');
  // Voice is off in this fake, so neither voice button shows.
  await expect(page.getByRole('button',{name:'Record a message'})).toBeHidden();
  await expect(page.getByRole('button',{name:'Read replies aloud'})).toBeHidden();
  await page.getByRole('button',{name:'Build and test a web app'}).click();
  await expect(page.getByLabel('Message the agent')).toHaveValue(/Flask/);

  // The first message creates the session; the turn's steps arrive over SSE.
  const prompt=page.getByLabel('Message the agent');
  await prompt.fill('Build a to-do app and test it in the browser.');
  await prompt.press('Enter');
  await expect(prompt).toHaveValue('');
  const log=page.locator('#log');
  await expect(log.locator('.msg.you').first()).toHaveText('Build a to-do app and test it in the browser.');
  const steps=log.locator('.step');
  await expect(steps).toHaveCount(3);
  await expect(steps.nth(0)).toContainText('Ran npm test — exit 0');
  await expect(steps.nth(0)).toHaveClass(/ok/);
  await expect(steps.nth(1)).toContainText('Opened example.com');
  await expect(steps.nth(2)).toContainText('Took a screenshot');
  // Output is behind a disclosure.
  await expect(steps.nth(0).locator('pre')).toBeHidden();
  await steps.nth(0).locator('summary').click();
  await expect(steps.nth(0).locator('pre')).toContainText('$ npm test');
  await expect(steps.nth(0).locator('pre')).toContainText('2 passing');
  // The screenshot renders inline from a blob: URL (the CSP allows img-src blob:).
  // Pictures load once they scroll near the viewport, so bring this step on screen first.
  const shot=steps.nth(2).locator('img');
  await steps.nth(2).scrollIntoViewIfNeeded();
  await expect(shot).toHaveAttribute('src',/^blob:/);
  expect(await shot.evaluate(img=>img.complete&&img.naturalWidth)).toBe(640);
  // The drawer lists the session as working.
  const item=page.locator('#session-list .session').first();
  await expect(item).toContainText('Build a to-do app');
  await expect(item).toHaveAttribute('aria-current','true');
  await expect(item.locator('.dot')).toHaveClass(/running/);
  await expect(page.locator('#session-state')).toHaveText('Working');
  await expect(page.locator('#working')).toBeVisible();
  await page.screenshot({path:'/tmp/agent-rush-qa/session-steps.png'});

  // Sending while it works is an interjection; the send button stays enabled.
  await expect(page.getByRole('button',{name:'Send'})).toBeEnabled();
  await prompt.fill('Also add a health check route.');
  await page.getByRole('button',{name:'Send'}).click();
  const interjection=log.locator('.msg.interjection');
  await expect(interjection).toContainText('Also add a health check route.');
  await expect(interjection.locator('.tag')).toHaveText('Sent while working');
  await expect(steps.nth(3)).toContainText('Editing src/app.py');
  await expect(steps.nth(3)).toHaveClass(/pending/);

  // Stop cancels the turn.
  await page.getByRole('button',{name:'Stop the agent'}).click();
  await expect(log.locator('.note')).toContainText('Stopped at your request.');
  await expect(steps.nth(3)).toHaveClass(/bad/);
  await expect(page.getByRole('button',{name:'Stop the agent'})).toBeHidden();
  await expect(page.locator('#session-state')).toHaveText('Ready');
  expect(api.cancelled).toBe(1);

  // A message to an idle session starts a turn that replies with attachments.
  await prompt.fill('Write it up as a PDF report.');
  await prompt.press('Enter');
  const answer=log.locator('.msg.agent').last();
  await expect(answer.locator('.answer > p')).toHaveText('Done. The report is attached.');
  // Markdown: bold, a list, code spans and links. A javascript: link stays plain text.
  const body=answer.locator('.answer');
  await expect(body.locator('strong')).toHaveText('report');
  await expect(body.locator('ul > li')).toHaveCount(3);
  await expect(body.locator('code')).toHaveText(['report.md','report.pdf']);
  const source=body.getByRole('link',{name:'example.com'});
  await expect(source).toHaveAttribute('href','https://example.com/');
  await expect(source).toHaveAttribute('target','_blank');
  await expect(source).toHaveAttribute('rel','noopener noreferrer');
  await expect(body.locator('a')).toHaveCount(1);
  await expect(body.locator('a[href^="javascript:"]')).toHaveCount(0);
  await expect(body).toContainText('Preview: Open the preview (javascript:alert(1))');
  const attached=answer.getByRole('link',{name:'report.pdf'});
  await expect(attached).toHaveAttribute('href',/\/v1\/sessions\/s1\/files\?path=report\.pdf$/);
  await answer.locator('.shot').scrollIntoViewIfNeeded();
  await expect(answer.locator('.shot img')).toHaveAttribute('src',/^blob:/);
  const replyDownload=page.waitForEvent('download');
  await attached.click();
  expect((await replyDownload).suggestedFilename()).toBe('report.pdf');
  await page.screenshot({path:'/tmp/agent-rush-qa/session-desktop.png'});

  // The files panel lists the workspace and downloads a file.
  await page.getByRole('button',{name:'Files',exact:true}).click();
  const files=page.locator('#files-dialog');
  await expect(files.locator('#files-path')).toHaveText('/workspace');
  await expect(files.locator('.entry').first()).toHaveText('app/');
  await files.getByRole('button',{name:'app/'}).click();
  await expect(files.locator('#files-path')).toHaveText('/workspace/app');
  await expect(files.getByRole('link',{name:'main.py'})).toHaveAttribute('download','main.py');
  await files.getByRole('button',{name:'Up one folder'}).click();
  const link=files.getByRole('link',{name:'report.pdf'});
  await expect(link).toHaveAttribute('download','report.pdf');
  const download=page.waitForEvent('download');
  await link.click();
  expect((await download).suggestedFilename()).toBe('report.pdf');
  await page.screenshot({path:'/tmp/agent-rush-qa/session-files.png'});
  await page.getByRole('button',{name:'Close files'}).click();

  expect(api.streams).toBeGreaterThan(0);
  expect(api.csrf.length).toBeGreaterThan(0);
  expect(api.csrf.every(token=>token.length>0)).toBe(true);
  // New session clears the thread back to the welcome.
  await page.getByRole('button',{name:'New session'}).click();
  await expect(page.getByRole('heading',{level:1})).toHaveText('What should the agent work on?');
  await expect(log.locator('li')).toHaveCount(0);
  await page.getByRole('button',{name:'Sign out',exact:true}).click();
  await expect(page.locator('#session-list .session')).toHaveCount(0);
  expect(errors).toEqual([]);
});

test('voice: mic transcribes and sends, speaker reads new replies aloud',async({page,context})=>{
  await context.grantPermissions(['microphone']);
  const errors=watchErrors(page);
  const api=await fakeApi(page,{voice:true,seed:[{id:'s1',title:'Earlier work',events:[['user',{text:'Hello'}],['assistant',{text:'Hi there.',attachments:[]}]]}]});
  await page.goto('/');
  await signIn(page);
  await page.locator('#session-list .session',{hasText:'Earlier work'}).click();
  await expect(page.locator('#log .answer')).toHaveText('Hi there.');
  const speaker=page.getByRole('button',{name:'Read replies aloud'});
  await expect(speaker).toBeVisible();
  // Replayed replies are not spoken.
  expect(api.speak).toBe(0);
  await speaker.click();
  await expect(speaker).toHaveAttribute('aria-pressed','true');
  const mic=page.getByRole('button',{name:'Record a message'});
  await mic.click();
  await expect(page.getByRole('button',{name:'Stop recording and send'})).toHaveAttribute('aria-pressed','true');
  await page.waitForTimeout(600);
  await page.getByRole('button',{name:'Stop recording and send'}).click();
  await expect(page.locator('#log .msg.you').last()).toHaveText('Read me the result');
  expect(api.transcribe).toBe(1);
  await expect(page.locator('#log .answer > p').last()).toHaveText('Done. The report is attached.');
  await expect.poll(()=>api.speak).toBe(1);
  expect(errors).toEqual([]);
});

test('mobile at 390 px: no overflow, drawer works, polling fallback when SSE fails',async({page})=>{
  await page.setViewportSize({width:390,height:844});
  // Signed out first.
  await page.goto('/');
  await expect(page.getByRole('heading',{level:1})).toBeVisible();
  expect(await page.evaluate(()=>document.documentElement.scrollWidth<=window.innerWidth)).toBe(true);
  await expect(page.locator('.composer')).toBeInViewport();
  await page.screenshot({path:'/tmp/agent-rush-qa/mobile-signed-out.png'});
  const errors=watchErrors(page,['502','text/event-stream']);
  const long='x'.repeat(300);
  const api=await fakeApi(page,{sse:false,seed:[{id:'s9',title:'A session with a very long title that must not widen the page at all',status:'running',events:[
    ['user',{text:`Run this: ${long}`}],
    ['status',{status:'running'}],
    ['tool_call',{tool:'bash',call:{tool:'bash',command:`echo ${long}`}}],
    ['tool_result',{tool:'bash',ok:false,output:`exit code 2\nstderr:\n${long}`,truncated:true}],
    ['tool_call',{tool:'browser',call:{tool:'browser',action:'screenshot'}}],
    ['tool_result',{tool:'browser',ok:true,output:'exit code 0\nstdout:\n{"path":"/workspace/.screenshots/wide.png"}',truncated:false}],
  ]}]});
  await page.reload();
  await expect(page.getByRole('button',{name:'Sign in',exact:true})).toBeVisible();
  await signIn(page);
  // The drawer is behind a toggle on narrow screens.
  const toggle=page.getByRole('button',{name:'Show sessions'});
  await expect(page.locator('#drawer')).toBeHidden();
  await toggle.click();
  await expect(toggle).toHaveAttribute('aria-expanded','true');
  await page.locator('#session-list .session').first().click();
  await expect(page.locator('#drawer')).toBeHidden();
  const steps=page.locator('#log .step');
  await expect(steps).toHaveCount(2);
  await expect(steps.nth(0)).toContainText('— exit 2');
  await expect(steps.nth(0)).toHaveClass(/bad/);
  await steps.nth(1).scrollIntoViewIfNeeded();
  await expect(steps.nth(1).locator('img')).toHaveAttribute('src',/^blob:/);
  await steps.nth(0).locator('summary').click();
  await expect(steps.nth(0).locator('.step-note')).toHaveText('The agent saw a shortened copy of this output.');
  // The stream failed, so the page polls the event log instead, and interjections still work.
  expect(api.polls).toBeGreaterThan(0);
  await page.getByLabel('Message the agent').fill('Stop printing x.');
  await page.getByLabel('Message the agent').press('Enter');
  await expect(page.locator('#log .msg.interjection .tag')).toHaveText('Sent while working');
  expect(await page.evaluate(()=>document.documentElement.scrollWidth<=window.innerWidth)).toBe(true);
  await expect(page.locator('.composer')).toBeInViewport();
  await page.screenshot({path:'/tmp/agent-rush-qa/mobile-session.png'});
  await page.getByRole('button',{name:'Files',exact:true}).click();
  await expect(page.locator('#files-dialog').getByRole('link',{name:'report.pdf'})).toBeVisible();
  expect(await page.evaluate(()=>document.documentElement.scrollWidth<=window.innerWidth)).toBe(true);
  await page.getByRole('button',{name:'Close files'}).click();
  expect(errors).toEqual([]);
});

test('eight browser screenshots: 429s are retried, at most two load at once, all eight display',async({page})=>{
  // Chromium logs each refused request. The fake counts them, so only that message is ignored.
  const errors=watchErrors(page,['429 (Too Many Requests)']);
  const paths=[...Array(8)].map((_,n)=>`/workspace/shot-${n+1}.png`);
  const api=await fakeApi(page,{throttleImages:true,seed:[{id:'s1',title:'Eight screenshots',events:[
    ['user',{text:'Take a screenshot of each page.'}],
    ...paths.flatMap(path=>[
      ['tool_call',{tool:'browser',call:{tool:'browser',action:'screenshot'}}],
      ['tool_result',{tool:'browser',ok:true,output:`exit code 0\nstdout:\n{"path":"${path}"}`,truncated:false}],
    ]),
  ]}]});
  await page.goto('/');
  await signIn(page);
  await page.locator('#session-list .session',{hasText:'Eight screenshots'}).click();
  const steps=page.locator('#log .step');
  await expect(steps).toHaveCount(8);
  // Each picture loads once it scrolls near the viewport. Its first request gets a 429, and the
  // retry after the 500 ms backoff is served, so every picture ends up displayed at full size.
  for(const n of paths.keys()){
    const figure=steps.nth(n).locator('figure.shot');
    await figure.scrollIntoViewIfNeeded();
    const shot=figure.locator('img');
    await expect(shot).toHaveAttribute('src',/^blob:/);
    await expect.poll(()=>shot.evaluate(img=>img.complete&&img.naturalWidth)).toBe(640);
  }
  await expect(page.locator('#log')).not.toContainText('could not be loaded');
  // At most two image requests were in flight at once, and each picture was asked for exactly twice:
  // refused, then served.
  expect(api.images.maxInFlight).toBeLessThanOrEqual(2);
  expect(Object.keys(api.images.requests).sort()).toEqual([...paths].sort());
  for(const path of paths)expect(api.images.requests[path]).toBe(2);
  expect(errors).toEqual([]);
});

const DELETE_SEED=[
  {id:'old1',title:'Old closed one',status:'closed',events:[['user',{text:'Hi'}],['status',{status:'closed'}]]},
  {id:'old2',title:'Another closed one',status:'closed',events:[['user',{text:'Hey'}],['status',{status:'closed'}]]},
  {id:'keep',title:'Keep this session',events:[['user',{text:'Keep'}],['assistant',{text:'Kept.',attachments:[]}]]},
  {id:'gone',title:'Delete me',events:[['user',{text:'Hello'}],['assistant',{text:'Hi there.',attachments:[]}]]},
];

test('deleting sessions: confirm dialog, the open session returns to the welcome, clear closed',async({page})=>{
  const errors=watchErrors(page);
  const api=await fakeApi(page,{seed:DELETE_SEED});
  await page.goto('/');
  await signIn(page);
  const rows=page.locator('#session-list .session');
  await expect(rows).toHaveCount(4);
  await rows.filter({hasText:'Delete me'}).click();
  await expect(page.locator('#log .answer')).toHaveText('Hi there.');
  const clear=page.getByRole('button',{name:'Clear closed sessions'});
  await expect(clear).toBeVisible();

  // The delete button is hidden until the row is hovered or the button has keyboard focus.
  const remove=page.getByRole('button',{name:'Delete session Delete me'});
  await page.locator('#thread').focus();
  await page.mouse.move(900,500);
  await expect(remove).toHaveCSS('opacity','0');
  await page.locator('#session-list li',{hasText:'Delete me'}).hover();
  await expect(remove).toHaveCSS('opacity','1');
  await page.mouse.move(900,500);
  await expect(remove).toHaveCSS('opacity','0');
  await rows.filter({hasText:'Delete me'}).focus();
  await page.keyboard.press('Tab');
  await expect(remove).toBeFocused();
  await expect(remove).toHaveCSS('opacity','1');
  expect(await remove.evaluate(b=>getComputedStyle(b).outlineStyle)).toBe('solid');

  // Enter opens the confirm dialog with Cancel focused; Escape cancels and nothing is deleted.
  await page.keyboard.press('Enter');
  const dialog=page.getByRole('alertdialog',{name:'Delete this session?'});
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText('Its workspace files and history are removed for good.');
  await expect(dialog.getByRole('button',{name:'Cancel'})).toBeFocused();
  const confirm=dialog.getByRole('button',{name:'Delete',exact:true});
  await expect(confirm).toHaveCSS('background-color','rgb(236, 110, 118)');
  await page.keyboard.press('Escape');
  await expect(dialog).toBeHidden();
  expect(api.deletes).toEqual([]);
  await expect(rows).toHaveCount(4);
  await expect(page.locator('#log .answer')).toHaveText('Hi there.');

  // Cancel also keeps it.
  await remove.click();
  await dialog.getByRole('button',{name:'Cancel'}).click();
  await expect(dialog).toBeHidden();
  expect(api.deletes).toEqual([]);

  // Delete removes it for good; it was open, so the chat returns to the welcome.
  await remove.click();
  await expect(dialog).toBeVisible();
  await page.screenshot({path:'/tmp/agent-rush-qa/session-delete.png'});
  await confirm.click();
  await expect(dialog).toBeHidden();
  await expect(rows).toHaveCount(3);
  await expect(page.locator('#session-list')).not.toContainText('Delete me');
  expect(api.deletes).toEqual(['gone']);
  await expect(page.getByRole('heading',{level:1})).toHaveText('What should the agent work on?');
  await expect(page.locator('#chat-head')).toBeHidden();
  await expect(page.locator('#log li')).toHaveCount(0);
  await expect(page.locator('#toast')).toHaveText('Session deleted.');
  // Focus moves to the row that took its place.
  await expect(rows.filter({hasText:'Keep this session'})).toBeFocused();

  // Clear closed sessions removes both closed ones, after the same confirmation.
  await clear.click();
  const clearDialog=page.getByRole('alertdialog',{name:'Clear closed sessions?'});
  await expect(clearDialog).toContainText('All 2 closed sessions and their history are removed for good.');
  await clearDialog.getByRole('button',{name:'Delete',exact:true}).click();
  await expect(rows).toHaveCount(1);
  await expect(rows.first()).toContainText('Keep this session');
  await expect(clear).toBeHidden();
  expect(api.purges).toEqual([['old2','old1']]);
  await expect(page.locator('#toast')).toHaveText('Deleted 2 closed sessions.');
  // The 5 s list refresh does not bring anything back.
  await page.waitForTimeout(5500);
  await expect(rows).toHaveCount(1);
  expect(errors).toEqual([]);
});

test('mobile at 390 px: the delete button always shows and the dialog fits',async({page})=>{
  await page.setViewportSize({width:390,height:844});
  const errors=watchErrors(page);
  const api=await fakeApi(page,{seed:DELETE_SEED});
  await page.goto('/');
  await signIn(page);
  await page.getByRole('button',{name:'Show sessions'}).click();
  const remove=page.getByRole('button',{name:'Delete session Keep this session'});
  await expect(remove).toBeVisible();
  await expect(remove).toHaveCSS('opacity','1');
  const box=await remove.boundingBox();
  expect(box.x+box.width).toBeLessThanOrEqual(390);
  await remove.click();
  const dialog=page.getByRole('alertdialog',{name:'Delete this session?'});
  await expect(dialog).toBeVisible();
  const sheet=await dialog.boundingBox();
  expect(sheet.x).toBeGreaterThanOrEqual(0);
  expect(sheet.x+sheet.width).toBeLessThanOrEqual(390);
  expect(await page.evaluate(()=>document.documentElement.scrollWidth<=window.innerWidth)).toBe(true);
  await page.screenshot({path:'/tmp/agent-rush-qa/mobile-session-delete.png'});
  await dialog.getByRole('button',{name:'Delete',exact:true}).click();
  await expect(page.locator('#session-list .session')).toHaveCount(3);
  expect(api.deletes).toEqual(['keep']);
  // The drawer stays open after the delete.
  await expect(page.locator('#drawer')).toBeVisible();
  expect(errors).toEqual([]);
});
