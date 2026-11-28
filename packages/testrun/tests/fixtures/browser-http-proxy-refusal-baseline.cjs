// Unchanged pinned APIRequest: a refused CONNECT never receives origin bytes.
const net = require('node:net');
const {request} = require(process.argv[2]);
const responses = [
  {name:'authentication',scheme:'http',status:407,headers:'Proxy-Authenticate: Basic realm="Acme"\r\n'},
  {name:'https-post',scheme:'https',status:502,method:'POST'},
  {name:'redirect-disabled',scheme:'https',status:301,headers:'Location: /Acme-next\r\n',options:{maxRedirects:0}},
  {name:'status-refusal',scheme:'http',status:503,options:{failOnStatusCode:true}},
  {name:'created',scheme:'https',status:201},
  {name:'empty',scheme:'http',status:204,body:''},
  {name:'redirect',scheme:'http',status:302,headers:'Location: /Acme-next\r\n',next:403},
  {name:'chunked',scheme:'https',status:502,chunked:true},
  {name:'head',scheme:'https',status:502,method:'HEAD'},
  {name:'head-body',scheme:'https',status:502,method:'HEAD',invalidHead:true},
];
async function listen(server) {
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  return server.address().port;
}
(async()=>{
  let originConnections=0;
  const origin=net.createServer(socket=>{++originConnections;socket.destroy();});
  const originPort=await listen(origin);
  const report=[];
  try {
    for(const spec of responses) {
      const connections=[];
      let count=0;
      const proxy=net.createServer(socket=>{
        const record={headers:'',trailingBytes:0};connections.push(record);
        let head=Buffer.alloc(0),responded=false;
        socket.on('error',()=>{});
        socket.on('data',chunk=>{
          if(responded) {record.trailingBytes+=chunk.length;return;}
          head=Buffer.concat([head,chunk]);
          if(head.length>32768) throw new Error('Acme CONNECT header bound');
          const end=head.indexOf('\r\n\r\n');if(end<0)return;
          record.headers=head.subarray(0,end+4).toString();
          record.trailingBytes+=head.length-end-4;responded=true;
          if(!record.headers.startsWith(`CONNECT 127.0.0.1:${originPort} HTTP/1.1\r\n`))throw new Error('Acme selected destination');
          const status=count++===0?spec.status:spec.next;
          if(!status)throw new Error('Acme unexpected proxy replay');
          const body=status===spec.status?(spec.body??'Acme'): 'Acme';
          const headers=status===spec.status?(spec.headers??''):'';
          const entity=spec.chunked?`Transfer-Encoding: chunked\r\nConnection: close\r\n\r\n4\r\nAcme\r\n0\r\n\r\n`:`Content-Length: ${Buffer.byteLength(body)}\r\nConnection: close\r\n\r\n${spec.method==='HEAD'&&!spec.invalidHead?'':body}`;
          socket.end(`HTTP/1.1 ${status} Acme refusal\r\n${headers}${entity}`);
        });
      });
      const proxyPort=await listen(proxy);
      const client=await request.newContext({
        proxy:{server:`http://127.0.0.1:${proxyPort}`,username:'Acme proxy',password:'proxy example'},
        extraHTTPHeaders:{Authorization:'Bearer Acme origin secret','X-Acme':'origin secret',Cookie:'Acme=private'},
      });
      const url=`${spec.scheme}://127.0.0.1:${originPort}/Acme`;
      let result;
      try {
        const response=await client.fetch(url,{method:spec.method??'GET',maxRetries:2,...(spec.method==='POST'?{data:'Acme private body'}:{}),...(spec.options??{})});
        result={status:response.status(),statusText:response.statusText(),body:await response.text(),headers:response.headersArray(),url:new URL(response.url()).pathname};
        await response.dispose();
      } catch(error) {
        result={error:String(error.message).split('\n')[0]};
      } finally {
        await client.dispose();
        await new Promise(resolve=>proxy.close(resolve));
      }
      report.push({name:spec.name,...result,connections:connections.map(row=>{
        const headers=Object.fromEntries(row.headers.trimEnd().split('\r\n').slice(1).map(line=>{const i=line.indexOf(':');return [line.slice(0,i).toLowerCase(),line.slice(i+1).trim()];}));
        headers.host=headers.host.replace(String(originPort),'PORT');
        return {headers,trailingBytes:row.trailingBytes};
      })});
    }
  } finally {await new Promise(resolve=>origin.close(resolve));}
  process.stdout.write(JSON.stringify({cases:report,originConnections}));
})().catch(error=>{console.error(error);process.exitCode=1;});
