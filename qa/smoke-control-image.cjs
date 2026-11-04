'use strict';
// Trusted packaging QA; application/backend code remains native Rust.
const {execFileSync}=require('node:child_process');
const fs=require('node:fs');const os=require('node:os');const {randomUUID}=require('node:crypto');
const config=fs.mkdtempSync(os.tmpdir()+'/agent-rush-smoke-docker-');
const name='ar-control-smoke-'+randomUUID();
const password='synthetic-image-smoke-password-only';
const report={schema:'agent-rush.control-smoke.v1',passed:false,cleanup_verified:false,checks:[]};
function docker(...args){return execFileSync('docker',['--config',config,'--host=unix:///var/run/docker.sock',...args],{encoding:'utf8',timeout:90000,stdio:['ignore','pipe','pipe']}).trim();}
function ensure(value,message){if(!value)throw new Error(message);}
(async()=>{try{
 const image=JSON.parse(docker('image','inspect','agent-rush-control:local'))[0];
 Object.assign(report,{image:image.Id,source_revision:image.Config.Labels['org.opencontainers.image.revision'],user:image.Config.User});
 ensure(image.Config.User==='65532:65532','Image must be non-root');
 ensure(!(image.Config.Env||[]).some(v=>/^(VULTR_|AR_OPERATOR_PASSWORD=|AR_WORKER_TOKEN=|HTTP_PROXY=|HTTPS_PROXY=)/.test(v)),'Image contains prohibited credential/proxy configuration');
 docker('run','-d','--name',name,'--label','agent-rush.smoke=true','--read-only','--cap-drop=ALL','--security-opt=no-new-privileges','--pids-limit=64','--memory=256m','--cpus=1','--tmpfs','/data:rw,noexec,nosuid,nodev,size=128m,uid=65532,gid=65532,mode=0700','--tmpfs','/tmp:rw,noexec,nosuid,nodev,size=16m','-p','127.0.0.1::8080','--env','AR_OPERATOR_PASSWORD='+password,'--env','AR_SECURE_COOKIES=false','--env','AR_INFERENCE_BUDGET_USD=0','agent-rush-control:local');
 const container=JSON.parse(docker('inspect',name))[0];
 const base='http://127.0.0.1:'+container.NetworkSettings.Ports['8080/tcp'][0].HostPort;
 let cookie='',csrf='';
 async function request(path,method='GET',body=null){
  const response=await fetch(base+path,{method,headers:{'Content-Type':'application/json','Idempotency-Key':randomUUID(),...(cookie?{Cookie:cookie}:{}),...(csrf?{'X-CSRF-Token':csrf}:{})},...(method==='GET'?{}:{body:JSON.stringify(body)}),signal:AbortSignal.timeout(5000)});
  if(response.headers.has('set-cookie'))cookie=response.headers.get('set-cookie').split(';')[0];
  const value=await response.json();if(!response.ok){const error=new Error('HTTP '+response.status);error.status=response.status;throw error;}return value;
 }
 for(let attempt=0;attempt<40;attempt++){try{await request('/healthz');break;}catch(error){if(attempt===39)throw error;await new Promise(r=>setTimeout(r,250));}}
 report.checks.push('Native runtime and SQLite writable under non-root/read-only root');
 csrf=(await request('/api/login','POST',{password})).csrf;report.checks.push('Operator login and session cookie');
 const job=await request('/api/jobs','POST',{prompt:'Compute quarterly revenue totals.'});
 ensure(job.state==='queued','Job must queue while inference/worker disabled');
 ensure((await request('/api/jobs/'+job.id+'/contexts')).contexts.length===0,'Context export mismatch');
 ensure((await request('/api/jobs/'+job.id+'/cancel','POST')).state==='cancelled','Cancel failed');
 ensure((await request('/api/jobs/'+job.id+'/evidence')).state==='cancelled','Evidence export failed');
 report.checks.push('Real queued job, owner context export, cancellation and evidence');
 const note=await request('/api/memories','POST',{title:'QA fixture',text:'Use integer cents.'});
 ensure((await request('/api/memories?query=cents'))[0].id===note.id,'Memory retrieval failed');
 ensure((await request('/api/memories/'+note.id,'DELETE')).deleted,'Memory deletion failed');
 const status=await request('/api/status');ensure(status.inference_accounted_usd===0&&!status.inference_configured&&!status.worker_connected,'Unexpected paid/worker configuration');
 await request('/api/logout','POST');
 try{await request('/api/jobs');throw new Error('Logout did not revoke session');}catch(error){ensure(error.status===401,'Unexpected logout response');}
 report.checks.push('Memory save/search/delete and logout revocation');
 const info=JSON.parse(docker('info','--format','{{json .}}'));
 report.host_resource_enforcement=Object.fromEntries(['MemoryLimit','CpuCfsQuota','PidsLimit','CgroupDriver'].map(key=>[key,info[key]]));
 report.checks.push('No inference requests, worker executions or cloud resources');report.passed=true;
 }catch(error){report.error=String(error.message).slice(0,500);process.exitCode=1;}
 finally{
  try{docker('rm','-f',name);report.cleanup_verified=!docker('ps','-aq','--filter','name=^/'+name+'$');}catch(error){report.cleanup_error=String(error.message).slice(0,500);process.exitCode=1;}
  if(!report.cleanup_verified)process.exitCode=1;
  fs.writeFileSync('/workspace/artifacts/agent-rush-control-smoke.json',JSON.stringify(report,null,2)+'\n');
  fs.rmSync(config,{recursive:true,force:true});
  console.log(JSON.stringify({passed:report.passed,cleanup_verified:report.cleanup_verified,report:'/workspace/artifacts/agent-rush-control-smoke.json'}));
 }
})();
