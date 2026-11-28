async options => {
  if(location.origin!==options.origin)return {error:'CLIPBOARD_ORIGIN_MISMATCH'};
  if(!isSecureContext||!navigator.clipboard)return {error:'CLIPBOARD_UNAVAILABLE'};
  try {
    if(options.action.kind==='write') {
      await navigator.clipboard.writeText(options.action.text);
      return {written:true,scope:'system-text-clipboard'};
    }
    const text=await navigator.clipboard.readText();
    if(new TextEncoder().encode(text).length>16384)return {error:'CLIPBOARD_BOUNDS'};
    return {text,scope:'system-text-clipboard'};
  } catch {return {error:'CLIPBOARD_PERMISSION_REQUIRED'};}
}
