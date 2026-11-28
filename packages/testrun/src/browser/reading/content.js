// Parse fetched markup without attaching it or executing document scripts.
(html, options, bound) => {
  const template=document.createElement('template');
  template.innerHTML=html;
  const content=template.content;
  const title=bound(content.querySelector('title')?.textContent||'',256).value;
  for(const element of content.querySelectorAll('script,style,template,noscript,svg,iframe,object,embed,title,head'))element.remove();
  const root=content.querySelector('article,main,[role=main]')||content;
  const blocks=[];const walker=document.createTreeWalker(root,NodeFilter.SHOW_TEXT);
  let node;while((node=walker.nextNode())){const value=node.textContent.replace(/\s+/g,' ').trim();if(value)blocks.push(value);}
  const metadata={title,url:bound(options.url,Math.min(1024,options.maxBytes)).value};
  const remaining=Math.max(0,options.maxBytes-new TextEncoder().encode(JSON.stringify({...metadata,value:'',truncated:true})).length);
  const result={...metadata,...bound(blocks.join('\n'),remaining)};
  return new TextEncoder().encode(JSON.stringify(result)).length>options.maxBytes
    ?{value:null,truncated:true,narrowingRequired:true}:result;
}
