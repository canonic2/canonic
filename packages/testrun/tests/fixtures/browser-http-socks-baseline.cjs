// Independent pinned SOCKS HTTP wire facts; every destination is this fixture's loopback origin.
const http=require('node:http');
const https=require('node:https');
const fs=require('node:fs');
const {request}=require(process.argv[2]);
const {listen,socks}=require('./browser-socks-wire.cjs');
(async()=>{
  const originRecords=[];let originPort;
  const origin=http.createServer((req,res)=>{
    originRecords.push({path:req.url,host:req.headers.host.replace(String(originPort),'PORT'),authorization:req.headers.authorization??'',proxyAuthorization:req.headers['proxy-authorization']??'',custom:req.headers['x-acme']??''});
    if(req.url==='/forward'||req.url==='/reverse') {
      const host=req.url==='/forward'?'127.0.0.1':'localhost';res.writeHead(302,{Location:`http://${host}:${originPort}/echo`});res.end();
    } else {res.writeHead(200,{'Content-Type':'text/plain','Connection':'close'});res.end('Acme');}
  });
  const originOwner=await listen(origin);originPort=originOwner.port;
  const cases=[
    {name:'socks5',scheme:'socks5'},{name:'socks5h',scheme:'socks5h'},{name:'socks',scheme:'socks'},
    {name:'socks4',scheme:'socks4'},{name:'socks4a',scheme:'socks4a'},
    {name:'uri-auth',scheme:'socks5',userinfo:'Acme%3A%C3%A9:p%40ss@'},
    {name:'object-auth-ignored',scheme:'socks5',username:'Acme ignored',password:'ignored example'},
    {name:'socks4-user',scheme:'socks4',userinfo:'Acme%3Auser:ignored@'},
    {name:'ipv4',scheme:'socks5',host:'127.0.0.1'},
    {name:'ipv6',scheme:'socks5',host:'[::1]'},
    {name:'socks4-ipv6',scheme:'socks4',host:'[::1]'},
    {name:'forward-bypass',scheme:'socks5',path:'/forward',bypass:'127.0.0.1'},
    {name:'reverse-bypass',scheme:'socks5',path:'/reverse',host:'127.0.0.1',bypass:'127.0.0.1'},
    {name:'forced-empty-auth',scheme:'socks5',forceAuth:true},
  ];
  const report=[];
  try {
    for(const spec of cases) {
      const proxy=await socks(originPort,spec.forceAuth);
      const options={server:`${spec.scheme}://${spec.userinfo??''}127.0.0.1:${proxy.port}`,bypass:spec.bypass,username:spec.username,password:spec.password};
      const client=await request.newContext({proxy:options,extraHTTPHeaders:{Authorization:'Bearer Acme origin','X-Acme':'origin'}});
      const before=originRecords.length;let result;
      try {
        const response=await client.get(`http://${spec.host??'localhost'}:${originPort}${spec.path??'/echo'}`);
        result={status:response.status(),body:await response.text(),path:new URL(response.url()).pathname};await response.dispose();
      } catch(error) {result={error:String(error.message).split('\n')[0]};}
      finally {await client.dispose();await proxy.close();}
      report.push({name:spec.name,...result,connections:proxy.connections,origin:originRecords.slice(before)});
    }
    for(const spec of [
      {name:'authentication-refused',scheme:'socks5',refusal:'authentication'},
      {name:'socks4-connect-refused',scheme:'socks4',refusal:'connect'},
      {name:'socks5-connect-refused',scheme:'socks5',refusal:'connect'},
      {name:'greeting-timeout',scheme:'socks5',stall:true},
    ]) {
      const proxy=await socks(originPort,false,spec.refusal,spec.stall);
      const client=await request.newContext({proxy:{server:`${spec.scheme}://Acme:example@127.0.0.1:${proxy.port}`}});
      const before=originRecords.length,started=performance.now();let error;
      try {const response=await client.get(`http://localhost:${originPort}/echo`,{maxRetries:2,...(spec.stall?{timeout:80}:{})});await response.dispose();}
      catch(cause){error=String(cause.message).split('\n')[0];}
      const elapsedMs=performance.now()-started;
      await client.dispose();await proxy.close();
      report.push({name:spec.name,error,elapsedMs,connections:proxy.connections,origin:originRecords.slice(before),prefixes:proxy.prefixes});
    }
    {
      const proxy=await socks(originPort,false,null,false,true);
      const client=await request.newContext({proxy:{server:`socks5://Acme:example@127.0.0.1:${proxy.port}`}});
      const before=originRecords.length;
      const pending=client.post(`http://localhost:${originPort}/echo`,{data:'Acme private body'}).then(response=>({status:response.status()}),error=>({error:String(error.message).split('\n')[0]}));
      const until=Date.now()+2000;while(!proxy.connections[0]?.destination){if(Date.now()>=until)throw Error('Acme held handshake not reached');await new Promise(resolve=>setTimeout(resolve,5));}
      const started=performance.now();await client.dispose();const result=await pending;const elapsedMs=performance.now()-started;
      proxy.grant();await proxy.settled(1);await proxy.close();
      report.push({name:'disposal-cancel',...result,elapsedMs,connections:proxy.connections,origin:originRecords.slice(before),prefixes:proxy.prefixes});
    }
    const secureRecords=[];let securePort;
    const secure=https.createServer({cert:fs.readFileSync(process.argv[3]+'/server.pem'),key:fs.readFileSync(process.argv[3]+'/server-key.pem')},(req,res)=>{
      secureRecords.push({path:req.url,host:req.headers.host.replace(String(securePort),'PORT'),authorization:req.headers.authorization??'',proxyAuthorization:req.headers['proxy-authorization']??'',custom:req.headers['x-acme']??''});
      res.writeHead(200,{'Content-Length':'4','Connection':'close'});res.end('Acme');
    });
    secure.on('tlsClientError',()=>{});
    const secureOwner=await listen(secure);securePort=secureOwner.port;
    try {
      for(const spec of [{name:'tls-omitted'},{name:'tls-explicit-false',policy:false},{name:'tls-exception',policy:true}]) {
        const proxy=await socks(securePort);
        const client=await request.newContext({proxy:{server:`socks5://127.0.0.1:${proxy.port}`},extraHTTPHeaders:{Authorization:'Bearer Acme origin','X-Acme':'origin'},...(spec.policy===undefined?{}:{ignoreHTTPSErrors:spec.policy})});
        const before=secureRecords.length;let result;
        try {const response=await client.get(`https://localhost:${securePort}/echo`);result={status:response.status(),body:await response.text()};await response.dispose();}
        catch(error){result={error:String(error.message).split('\n')[0]};}
        finally {await client.dispose();await proxy.close();}
        report.push({name:spec.name,...result,connections:proxy.connections,origin:secureRecords.slice(before),prefixes:proxy.prefixes});
      }
    } finally {await secureOwner.close();}
  } finally {await originOwner.close();}
  process.stdout.write(JSON.stringify(report));
})().catch(error=>{console.error(error);process.exitCode=1;});
