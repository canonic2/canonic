// Pinned Node is the SDK server's RegExp engine; compare property data directly.
const properties=[
  ['^\\p{Letter}$','u'],['^\\p{Number}$','u'],['^\\p{Assigned}$','u'],
  ['^\\s$','u'],['^\\w$','iu'],['^\\p{Script=Kawi}$','u'],
  ['^\\p{Script_Extensions=Hiragana}$','u'],['^\\p{RGI_Emoji}$','v'],
].map(([pattern,flags])=>{
  const re=new RegExp(pattern,flags),ranges=[];let begin=-1;
  for(let code=0;code<=0x110000;code++){
    const matches=code<0x110000&&re.test(String.fromCodePoint(code));
    if(matches&&begin<0)begin=code;
    if(!matches&&begin>=0){ranges.push([begin,code-1]);begin=-1;}
  }
  return {pattern,flags,ranges};
});
process.stdout.write(JSON.stringify({unicode:process.versions.unicode,v8:process.versions.v8,properties}));
