import test from 'node:test';
import assert from 'node:assert/strict';
import { rustSurface, mcpProfiles } from './browser-rust-surface.mjs';

test('Rust branch inventory retains aliases, guards, defaults, raw strings and nested options', () => {
  const source = `fn parse_command(args: &[String]) { match args[0] {
    "open" | "goto" => { let text = r###"not a brace } or // comment"###; match args[1] { "--wait" => wait(5), _ => default() } }
    Some("auth") if args[1] == "login" => login(),
    _ => { /* { /* nested */ } */ refuse() }
  } }
  #[cfg(test)] mod tests { fn parse_fake() {} }`;
  const surface = rustSurface(source);
  assert.equal(surface.functions.length, 1);
  assert.deepEqual(surface.functions[0].flags, ['--wait']);
  assert.deepEqual(surface.branches[0].literals, ['open', 'goto']);
  assert.deepEqual(surface.branches[1].literals, ['auth']);
  assert.equal(surface.branches.filter(row => row.pattern === '_').length, 2);
  assert.match(surface.functions[0].expression, /wait\(5\)/);
  assert.throws(() => rustSurface('fn parse_command() { match x { "open" = wrong } }'));
  assert.throws(() => rustSurface('fn parse_command() { /* unclosed'));
  const conditional = rustSurface('#[cfg(test)]\npub(crate) async fn parse_private() { match x { _ => () } }');
  assert.match(conditional.functions[0].expression, /^#\[cfg\(test\)\]/);
});

test('MCP profile membership preserves all-only tools and refuses unknown references', () => {
  const source = `const CORE_PROFILE_TOOLS: &[&str] = &[TOOL_OPEN];
  // enabled_tools: None; Self::from_profiles(vec![ToolProfile::Core])`;
  const declarations = [{constant:'TOOL_OPEN',name:'agent_browser_open'}];
  assert.deepEqual(mcpProfiles(source, declarations).map(p => p.name), ['core', 'all']);
  assert.throws(() => mcpProfiles(source.replace('TOOL_OPEN];', 'TOOL_UNKNOWN];'), declarations));
  const allOnly = mcpProfiles(source, [...declarations, {constant:'TOOL_NEW',name:'agent_browser_new'}]);
  assert.deepEqual(allOnly.find(p => p.name === 'core').tools, ['agent_browser_open']);
  assert.deepEqual(allOnly.find(p => p.name === 'all').tools, ['agent_browser_open', 'agent_browser_new']);
});
