(text, maxBytes) => {
  let end = Math.min(text.length, Math.floor(maxBytes / 4));
  const last = text.charCodeAt(end - 1);
  if (last >= 0xD800 && last <= 0xDBFF) --end;
  return {value:text.slice(0,end),truncated:end<text.length};
}
