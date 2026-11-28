// Explicit inspection can execute getters/proxy traps; results never imply a pure read.
codec => (value, query) => {
  for (const key of query.path) value = value[key];
  if (query.properties) {
    const names=[];let skipped=0,more=false;
    for(const name in Object(value)) {
      if(!Object.prototype.hasOwnProperty.call(value,name))continue;
      if(skipped++<query.properties.offset)continue;
      if(names.length>=query.properties.limit){more=true;break;}
      if(new TextEncoder().encode(name).byteLength>1024)throw Error('canonic:PROPERTY_NAME_BOUNDS');
      if(new TextEncoder().encode(JSON.stringify([...names,name])).byteLength>2000){
        if(!names.length)throw Error('canonic:PROPERTY_NAME_BOUNDS');
        more=true;break;
      }
      names.push(name);
    }
    const next=query.properties.offset+names.length,exhausted=more&&next>10000;
    value={names,hasMore:more,nextOffset:exhausted?null:next,offsetLimitReached:exhausted,snapshot:false,keys:'own-enumerable-string'};
  }
  return codec.encode(value,32768);
}
