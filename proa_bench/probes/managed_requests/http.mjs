// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import {writeFile,readFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import http from 'node:http';
import {pathToFileURL} from 'node:url';
const [root, originalBenchmarks] = process.argv.slice(2);
const {acquireRunLock,spawnWorkers,closeWorkers,startServer,stopServer,waitForHealth,fetchBody,runOpenLoopPhase,hostMetadata} = await import(pathToFileURL(`${root}/benchmarks/load/driver.mjs`));
const release=await acquireRunLock(originalBenchmarks,'local Proa head comparison');
const workers=await spawnWorkers(2);
const report={protocol:'Existing benchmark production server, load/driver.mjs and page-load/validate.mjs; one Tokio worker; 3 alternating pairs per workload, open-loop rates below saturation, 1s pre-roll + 5s measurement; validate every response by exact byte SHA256; fresh server each phase',host:hostMetadata(),phases:[]};
try {
 for(const [workload,rate] of [['cards',2000],['products',500],['table',50]]) {
  let expected;
  for(let repeat=0;repeat<3;repeat++) {
   for(const label of (repeat%2===0?['before','fixed']:['fixed','before'])) {
    const agent=new http.Agent({keepAlive:true});
    const server=startServer({command:`${root}/bin/${label}-proa_page_server`,args:[],cwd:root,label},{PORT:'5846',PROA_WORKER_THREADS:'1'});
    try {
     await waitForHealth(server,5846,agent,h=>{if(h.workerThreads!==1)throw Error('worker count');return h;});
     const response=await fetchBody(5846,`/${workload}`,agent);
     if(response.statusCode!==200)throw Error('status');
     const hash=createHash('sha256').update(response.body).digest('hex');
     expected??={expectedBytes:response.body.length,expectedSha256:hash};
     if(hash!==expected.expectedSha256)throw Error('cross-version output parity');
     for(let i=0;i<24;i++) await fetchBody(5846,`/${workload}`,agent);
     const phase=await runOpenLoopPhase({workers,server,port:5846,path:`/${workload}`,rate,preRollSeconds:1,durationSeconds:5,validatorModule:`${root}/benchmarks/page-load/validate.mjs`,validatorOptions:expected,validateEvery:1,metricsAgent:agent});
     if(phase.errorCount)throw Error(`response errors ${phase.errorCount}`);
     if(!phase.loadGeneratorValid)throw Error('generator limited');
     report.phases.push({label,workload,repeat,expected,...phase});
     await writeFile(`${root}/http-results.json`,JSON.stringify(report,null,2)+'\n');
     console.log(label,workload,repeat,JSON.stringify({rps:phase.completionThroughputRps,p50:phase.latencyMs.scheduledComplete.p50,p95:phase.latencyMs.scheduledComplete.p95,cpu:phase.cpu.cpuMsPerSuccessfulRequest,generator:phase.generator}), '\n');
    } finally {agent.destroy();await stopServer(server);}
   }
  }
 }
} finally {await closeWorkers(workers);await release();}
