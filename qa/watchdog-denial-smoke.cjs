'use strict';
// Failure-path integration only: a deliberately invalid credential can never authorize deletion.
const fs=require('node:fs');const os=require('node:os');const path=require('node:path');const {spawn}=require('node:child_process');const {randomUUID}=require('node:crypto');
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'agent-rush-watch-denial-'));const manifest=path.join(dir,'manifest.json');const heartbeat=path.join(dir,'heartbeat.json');const key=path.join(dir,'synthetic-key');
const report={schema:'agent-rush.watchdog-denial-smoke.v1',passed:false,credential_kind:'synthetic_invalid',provider_deletion_tested:false,cloud_resource_ids:0};
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));const ensure=(v,m)=>{if(!v)throw new Error(m);};
(async()=>{let child,closed;try{
 const at=new Date();fs.writeFileSync(manifest,JSON.stringify({schema:'agent-rush.deployment.v1',deployment_id:randomUUID(),credential_scope_sha256:'a'.repeat(64),region:'ord',created_at:at.toISOString(),expires_at:new Date(at.getTime()+7200000).toISOString(),hourly_estimate_usd:0.2,closed:false,resources:[]}),{mode:0o600});fs.writeFileSync(key,'deliberately-invalid-test-credential',{mode:0o600});
 child=spawn(path.resolve(__dirname,'../target/release/agent-rush'),['vultr-watch','--execute','--manifest',manifest,'--heartbeat',heartbeat,'--credential-file',key,'--poll-seconds','5'],{stdio:['ignore','pipe','pipe']});
 let output='';child.stdout.on('data',b=>{output=(output+b).slice(-16000);});child.stderr.resume();closed=new Promise((resolve,reject)=>{child.once('exit',(code,signal)=>resolve({code,signal}));child.once('error',reject);});
 let observed;
 for(let i=0;i<120;i++){if(fs.existsSync(heartbeat)){const v=JSON.parse(fs.readFileSync(heartbeat));if(v.phase==='error'){observed=v;break;}}await sleep(100);}
 ensure(observed,'No fail-closed heartbeat observed');ensure(observed.persistent&&observed.last_error,'API failure was not retained');ensure((fs.statSync(heartbeat).mode&0o777)===0o600,'Heartbeat permissions');
 report.error_phase_observed=true;report.heartbeat_owner_only=true;ensure(!output.includes('"watchdog_armed":true'),'Failed account check armed watchdog');report.false_arming_observed=false;
 child.kill('SIGTERM');const timeout=setTimeout(()=>child.kill('SIGKILL'),30000);const exit=await closed;clearTimeout(timeout);
 ensure(exit.code!==0&&!exit.signal,'Expected graceful nonzero exit on unresolved interrupt cleanup');ensure(JSON.parse(fs.readFileSync(manifest)).closed,'Cleanup intent fence was lost');
 report.process_exit_code=exit.code;report.process_stopped=true;report.cleanup_intent_fenced=true;report.passed=true;
 }catch(error){report.error=String(error.message).slice(0,500);process.exitCode=1;}
 finally{if(child&&child.exitCode===null&&child.signalCode===null){child.kill('SIGKILL');try{await closed;}catch{}}fs.rmSync(dir,{recursive:true,force:true});fs.writeFileSync('/workspace/artifacts/agent-rush-watchdog-denial-smoke.json',JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify(report));}
})();
