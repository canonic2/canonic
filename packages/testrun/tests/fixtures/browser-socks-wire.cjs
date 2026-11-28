const net=require('node:net');
async function listen(server) {
  const sockets=new Set();server.on('connection',socket=>{sockets.add(socket);socket.on('close',()=>sockets.delete(socket));});
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  return {port:server.address().port,close:async()=>{for(const socket of sockets)socket.destroy();await new Promise(resolve=>server.close(resolve));}};
}
async function socks(port,forceAuth=false,refusal=null,stall=false,hold=false,browser=false) {
  const connections=[],prefixes=[],protocolErrors=[],protocolPrefixes=[];let completed=0,grant,unselected=0;
  const permitted=new Promise(resolve=>{grant=resolve;});
  const server=net.createServer(socket=>{
    let buffered=Buffer.alloc(0),wake,closed=false;
    const receive=chunk=>{buffered=Buffer.concat([buffered,chunk]);if(buffered.length>32768)socket.destroy();wake?.();};
    socket.on('data',receive);socket.on('close',()=>{closed=true;++completed;wake?.();});socket.on('error',()=>{});
    const read=async n=>{while(buffered.length<n){if(closed)throw Error('Acme closed handshake');await new Promise(resolve=>{wake=resolve;});wake=undefined;}const value=buffered.subarray(0,n);buffered=buffered.subarray(n);return value;};
    const zero=async()=>{const bytes=[];for(;;){const byte=(await read(1))[0];if(byte===0)return Buffer.from(bytes).toString();bytes.push(byte);if(bytes.length>1024)throw Error('Acme handshake bound');}};
    (async()=>{
      const version=(await read(1))[0];
      const record={version};if(browser)record.applicationData=false;else connections.push(record);
      let targetPort,host;
      if(version===4) {
        const header=await read(7);if(header[0]!==1)throw Error('Acme CONNECT command');
        targetPort=header.readUInt16BE(1);record.userId=await zero();
        if(header.subarray(3,6).every(byte=>byte===0)&&header[6]!==0) {host=await zero();record.destination={type:'domain',host};}
        else {host=[...header.subarray(3)].join('.');record.destination={type:'ipv4',addressHex:header.subarray(3).toString('hex')};}
      } else if(version===5) {
        record.methods=[...await read((await read(1))[0])];
        record.authentication=null;
        if(stall)return;
        const method=forceAuth||record.methods.includes(2)?2:0;socket.write(Buffer.from([5,method]));
        if(method===2) {
          if((await read(1))[0]!==1)throw Error('Acme user/password version');
          const userId=(await read((await read(1))[0])).toString();
          const password=(await read((await read(1))[0])).toString();
          record.authentication={userId,password};
          if(refusal==='authentication'){record.rejection='authentication';socket.end(Buffer.from([1,1]));return;}
          socket.write(Buffer.from([1,0]));
        }
        const header=await read(4);if(header[0]!==5||header[1]!==1||header[2]!==0)throw Error('Acme CONNECT command');
        if(header[3]===1) {const address=await read(4);host=[...address].join('.');record.destination={type:'ipv4',addressHex:address.toString('hex')};}
        else if(header[3]===3) {host=(await read((await read(1))[0])).toString();record.destination={type:'domain',host};}
        else if(header[3]===4) {const address=await read(16);host=address.equals(Buffer.from('00000000000000000000000000000001','hex'))?'::1':address.equals(Buffer.from('00000000000000000000ffff7f000001','hex'))?'::ffff:127.0.0.1':'';record.destination={type:'ipv6',addressHex:address.toString('hex')};}
        else throw Error('Acme address type');
        targetPort=(await read(2)).readUInt16BE();
      } else {const prefix=[version,...await read(3)];if(protocolErrors.length<64){protocolErrors.push(version);protocolPrefixes.push(prefix);}throw Error(`Acme SOCKS version ${version}`);}
      if(targetPort!==port||!['localhost','127.0.0.1','::1','::ffff:127.0.0.1','::ffff:7f00:1',...(browser?['acme.invalid']:[])].includes(host)) {
        if(!browser)throw Error('Acme selected destination');
        ++unselected;
        socket.end(version===4?Buffer.from([0,91,0,0,0,0,0,0]):Buffer.from([5,2,0,1,0,0,0,0,0,0]));return;
      }
      record.destination.port='PORT';if(browser)connections.push(record);
      if(hold){await permitted;if(closed)return;}
      if(refusal==='connect'){record.rejection='connect';socket.end(version===4?Buffer.from([0,91,0,0,0,0,0,0]):Buffer.from([5,5,0,1,0,0,0,0,0,0]));return;}
      const upstream=net.connect(port,'127.0.0.1');
      upstream.on('error',()=>socket.destroy());socket.on('close',()=>upstream.destroy());
      await new Promise((resolve,reject)=>{upstream.once('connect',resolve);upstream.once('error',reject);});
      socket.write(version===4?Buffer.from([0,90,0,0,127,0,0,1]):Buffer.from([5,0,0,1,127,0,0,1,0,0]));
      socket.off('data',receive);if(buffered.length){prefixes.push(buffered[0]);upstream.write(buffered);}else socket.once('data',chunk=>prefixes.push(chunk[0]));
      if(browser){record.applicationData=buffered.length>0;socket.on('data',()=>{record.applicationData=true;});}
      socket.pipe(upstream);upstream.pipe(socket);
    })().catch(error=>{if(!closed)console.error(error);socket.destroy();});
  });
  return {...await listen(server),connections,prefixes,protocolErrors,protocolPrefixes,grant,unselected:()=>unselected,settled:async count=>{const end=Date.now()+2000;while(completed!==count){if(Date.now()>=end)throw Error('Acme owned SOCKS stream did not settle');await new Promise(resolve=>setTimeout(resolve,5));}}};
}
module.exports={listen,socks};
