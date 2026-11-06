const { defineConfig } = require('@playwright/test');
const {randomUUID}=require('node:crypto');
const {tmpdir}=require('node:os');
const {join}=require('node:path');
module.exports=defineConfig({testDir:'.',testMatch:'*.spec.cjs',workers:1,use:{baseURL:'http://127.0.0.1:8080',headless:true,launchOptions:{executablePath:'/usr/bin/chromium',args:['--no-sandbox','--use-fake-device-for-media-stream','--use-fake-ui-for-media-stream','--autoplay-policy=no-user-gesture-required']},viewport:{width:1487,height:1058}},webServer:{command:'../target/debug/agent-rush serve',url:'http://127.0.0.1:8080/healthz',reuseExistingServer:false,env:{AR_OPERATOR_PASSWORD:'qa-local-password-please-ignore',AR_SECURE_COOKIES:'false',AR_DATA_DIR:join(tmpdir(),'agent-rush-qa-'+randomUUID()),AR_BIND:'127.0.0.1:8080',VULTR_INFERENCE_API_KEY:'',AR_INFERENCE_MODEL:'',AR_WORKER_URL:'',AR_WORKER_TOKEN:'',AR_INFERENCE_BUDGET_USD:'0'}},reporter:'list',outputDir:'/tmp/agent-rush-browser-results'});
