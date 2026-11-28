// Page-owned tool callbacks stay in a retained, document-bound registry.
async function discoverDeveloperTools(Validator){
  let accepting=true,closed=false,truncated=false,invalidDefinitions=0;
  const tools=new Map(),validators=new Map(),groups=[];
  const bytes=value=>new TextEncoder().encode(value).length;
  const string=(value,max)=>typeof value==='string'&&bytes(value)<=max;
  const schemaBridge = original => {
    const marked=new Set(), resources=new Map(), escape=key=>encodeURIComponent(key.replace(/~/g,'~0').replace(/\//g,'~1'));
    const reference={type:'object',properties:{connectionId:{type:'string'},id:{type:'string'},generation:{type:'string'}},required:['connectionId','id','generation'],additionalProperties:false};
    let nodes=0;
    const convert=(schema,pointer,depth,resource='#')=>{
      if(++nodes>2048||depth>32)throw Error('Developer tool schema exceeds its bound');
      if(!schema||typeof schema!=='object'||Array.isArray(schema))return schema;
      if(typeof schema.$id==='string'&&schema.$id&&!schema.$id.startsWith('#'))resource=pointer;
      resources.set(pointer,resource);
      if(schema['x-mcp-type']==='HTMLElement'){
        marked.add(pointer);
        return {type:'object',properties:{elementHandle:reference},required:['elementHandle'],additionalProperties:false};
      }
      const result=Object.create(null);
      for(const [key,value] of Object.entries(schema)){
        const path=`${pointer}/${escape(key)}`;
        if(['properties','patternProperties','definitions','$defs','dependencies'].includes(key)&&value&&typeof value==='object'&&!Array.isArray(value)){
          result[key]=Object.fromEntries(Object.entries(value).map(([name,child])=>[name,Array.isArray(child)?child:convert(child,`${path}/${escape(name)}`,depth+1,resource)]));
        }else if(['allOf','anyOf','oneOf','items'].includes(key)&&Array.isArray(value))result[key]=value.map((child,index)=>convert(child,`${path}/${index}`,depth+1,resource));
        else if(['items','additionalItems','additionalProperties','contains','not','if','then','else','propertyNames'].includes(key))result[key]=convert(value,path,depth+1,resource);
        else result[key]=value;
      }
      return result;
    };
    const schema=convert(original,'#',0), compiled=new Map();let branchValidator;
    const resolve=pointer=>{
      if(pointer==='#')return schema;
      if(!pointer.startsWith('#/'))throw Error('External developer tool schema reference');
      let value=schema;
      for(const key of pointer.slice(2).split('/'))value=value?.[decodeURIComponent(key).replace(/~1/g,'/').replace(/~0/g,'~')];
      if(value===undefined)throw Error('Missing developer tool schema reference');
      return value;
    };
    const accepts=(pointer,value)=>{
      if(!compiled.has(pointer)){
        if(compiled.size>=128)throw Error('Developer tool schema branch bound');
        const key='urn:canonic:developer-tool-schema';
        if(!branchValidator){branchValidator=new Validator({ownProperties:true,allErrors:false,loopRequired:32,loopEnum:32});branchValidator.addSchema(schema,key);}
        compiled.set(pointer,branchValidator.compile({$ref:`${key}${pointer}`}));
      }
      const validate=compiled.get(pointer);
      if(validate.$async)throw Error('Asynchronous developer tool schema branches are unavailable');
      return validate(value)===true;
    };
    const paths=params=>{
      if(!marked.size)return [];
      const found=new Map(),active=new Set();let visits=0;
      const walk=(pointer,value,path,depth)=>{
        if(pointer!=='#'){
          if(!pointer.startsWith('#/'))throw Error('External developer tool schema reference');
          pointer='#/'+pointer.slice(2).split('/').map(key=>escape(decodeURIComponent(key).replace(/~1/g,'/').replace(/~0/g,'~'))).join('/');
        }
        if(++visits>4096||depth>32)throw Error('Developer tool element traversal bound');
        const token=JSON.stringify([pointer,path]);
        if(active.has(token))throw Error('Recursive developer tool schema without data progress');
        active.add(token);
        try{
          if(!accepts(pointer,value))return;
          if(marked.has(pointer)){
            found.set(JSON.stringify(path),path);
            if(found.size>32||bytes(JSON.stringify([...found.values()]))>4096)throw Error('Developer tool element path bound');
            return;
          }
          const node=resolve(pointer);
          if(!node||typeof node!=='object')return;
          if(node.$ref){
            if(node.$ref!=='#'&&!node.$ref.startsWith('#/'))throw Error('Element conversion requires local JSON Pointer references');
            walk(`${resources.get(pointer)??'#'}${node.$ref.slice(1)}`,value,path,depth+1);
          }
          for(const keyword of ['allOf','anyOf','oneOf'])if(Array.isArray(node[keyword]))node[keyword].forEach((_,index)=>walk(`${pointer}/${keyword}/${index}`,value,path,depth+1));
          if(node.if!==undefined){const branch=accepts(`${pointer}/if`,value)?'then':'else';if(node[branch]!==undefined)walk(`${pointer}/${branch}`,value,path,depth+1);}
          if(Array.isArray(value)){
            value.forEach((item,index)=>{
              const itemPath=Array.isArray(node.items)?index<node.items.length?`${pointer}/items/${index}`:node.additionalItems!==undefined?`${pointer}/additionalItems`:null:node.items!==undefined?`${pointer}/items`:null;
              if(itemPath)walk(itemPath,item,[...path,String(index)],depth+1);
              if(node.contains!==undefined)walk(`${pointer}/contains`,item,[...path,String(index)],depth+1);
            });
          }else if(value&&typeof value==='object'){
            for(const [key,item] of Object.entries(value)){
              let selected=false;
              if(Object.prototype.hasOwnProperty.call(node.properties??{},key)){walk(`${pointer}/properties/${escape(key)}`,item,[...path,key],depth+1);selected=true;}
              for(const pattern of Object.keys(node.patternProperties??{}))if(new RegExp(pattern,'u').test(key)){walk(`${pointer}/patternProperties/${escape(pattern)}`,item,[...path,key],depth+1);selected=true;}
              if(!selected&&node.additionalProperties!==undefined)walk(`${pointer}/additionalProperties`,item,[...path,key],depth+1);
            }
            for(const [key,child] of Object.entries(node.dependencies??{}))if(Object.prototype.hasOwnProperty.call(value,key)&&!Array.isArray(child))walk(`${pointer}/dependencies/${escape(key)}`,value,path,depth+1);
          }
        }finally{active.delete(token);}
      };
      walk('#',params,[],0);
      return [...found.values()];
    };
    return {schema,paths,elementParameters:[...marked].filter(pointer=>/^#\/properties\/[^/]+$/.test(pointer)).map(pointer=>decodeURIComponent(pointer.slice(13)).replace(/~1/g,'/').replace(/~0/g,'~'))};
  };
  let metadataBytes=0;
  const event=new CustomEvent('devtoolstooldiscovery');
  event.respondWith=group=>{
    if(!accepting||closed)return;
    try{
      if(!group||!string(group.name,256)||group.description!==undefined&&!string(group.description,4096)||!Array.isArray(group.tools)){invalidDefinitions++;return;}
      if(groups.length>=16){truncated=true;return;}
      const entries=[];
      for(const tool of group.tools){
        if(tools.size>=32){truncated=true;break;}
        if(!tool||!string(tool.name,256)||!string(tool.description,4096)||typeof tool.execute!=='function'||tool.inputSchema===null||!['object','boolean'].includes(typeof tool.inputSchema)){invalidDefinitions++;continue;}
        const encoded=JSON.stringify(tool.inputSchema);
        if(bytes(encoded)>8192){invalidDefinitions++;continue;}
        const bridge=schemaBridge(JSON.parse(encoded)),{schema,elementParameters}=bridge,toolId=`t${tools.size}`;
        const entry={toolId,name:tool.name,description:tool.description,inputSchema:schema,elementParameters};
        const size=bytes(JSON.stringify(entry));
        if(metadataBytes+size>120000){truncated=true;break;}
        metadataBytes+=size;entries.push(entry);
        tools.set(toolId,{execute:tool.execute,owner:tool,schema,elementParameters,bridge});
      }
      groups.push({name:group.name,description:group.description??'',tools:entries});
    }catch{invalidDefinitions++;}
  };
  window.dispatchEvent(event);
  if(!groups.length)await new Promise(resolve=>setTimeout(resolve,0));
  accepting=false;
  let description=JSON.stringify({groups,truncated,invalidDefinitions,source:'application-devtoolstooldiscovery',untrustedApplicationContent:true});
  if(bytes(description)>131072){tools.clear();validators.clear();groups.length=0;throw Error('Developer tool metadata exceeds its bound');}
  const errors=value=>(Array.isArray(value)?value:[]).slice(0,8).map(error=>({path:String(error.instancePath??'').slice(0,256),keyword:String(error.keyword??'').slice(0,128),message:String(error.message??'').slice(0,512)}));
  return {
    describe(){if(closed)throw Error('Developer tool registry closed');return description;},
    async validate(toolId,params){
      if(closed||!tools.has(toolId))return JSON.stringify({valid:false,unknownTool:true});
      let validate=validators.get(toolId);
      if(!validate){
        try{validate=new Validator({ownProperties:true,allErrors:false,loopRequired:32,loopEnum:32}).compile(tools.get(toolId).schema);validators.set(toolId,validate);}
        catch{return JSON.stringify({valid:false,unsupportedSchema:true});}
      }
      try{
        // Keep ordinary branch compilation inside the driver's CSP-bypassed
        // synchronous evaluation window; an unconditional await leaves it.
        const result=validate(params),valid=validate.$async?(await result,true):result===true;
        let elementPaths=[];
        if(valid)try{elementPaths=tools.get(toolId).bridge.paths(params);}catch(error){return JSON.stringify({valid:false,unsupportedSchema:true,elementConversionUnavailable:true,reason:String(error.message??error).slice(0,256)});}
        return JSON.stringify({valid,errors:errors(validate.errors),elementPaths});
      }
      catch(error){return JSON.stringify({valid:false,errors:errors(error.errors)});}
    },
    invoke(toolId,params,elementPaths){
      if(closed||!tools.has(toolId))throw Error('Developer tool registry closed or tool unavailable');
      const tool=tools.get(toolId);
      const paths=elementPaths??tool.elementParameters.filter(name=>Object.prototype.hasOwnProperty.call(params,name)).map(name=>[name]);
      if(!Array.isArray(paths)||paths.length>32)throw Error('Developer tool element path bound');
      for(const path of paths){
        if(!Array.isArray(path)||path.length>32)throw Error('Developer tool element path bound');
        let element=params;
        for(const key of path){if(!Object.prototype.hasOwnProperty.call(element,key))throw Error('Missing developer tool element');element=element[key];}
        if(!(element instanceof HTMLElement)||element.ownerDocument!==document||!element.isConnected)throw Error('Developer tool element is detached or belongs to another document');
      }
      return Reflect.apply(tool.execute,tool.owner,[params]);
    },
    close(){closed=true;accepting=false;tools.clear();validators.clear();groups.length=0;description='';return true;},
  };
}
