const vscode = require('vscode');
const fs = require('node:fs/promises');
const path = require('node:path');

exports.activate = async (context) => {
  const folder = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
  if (!folder || folder !== process.env.STUDIO_CHECKOUT) return;
  const sessionId = process.env.STUDIO_SESSION_ID;
  try {
    const request = JSON.parse(
      await fs.readFile(path.join(folder, '.studio-proof-request.json'), 'utf8'),
    );
    if (request.sessionId !== sessionId) return;
  } catch {
    return;
  }
  const result = path.join(folder, 'ide-proof.json');
  try {
    const document = await vscode.workspace.openTextDocument(path.join(folder, 'title.txt'));
    const editor = await vscode.window.showTextDocument(document);
    const title = `Studio smoke — ${sessionId}`;
    await editor.edit((builder) =>
      builder.replace(new vscode.Range(0, 0, document.lineCount, 0), title),
    );
    await document.save();
    const terminal = vscode.window.createTerminal({ name: 'Studio verification', cwd: folder });
    context.subscriptions.push(terminal);
    terminal.show();
    const quote = (value) => "'" + value.replaceAll("'", "'\\''") + "'";
    terminal.sendText(
      `printf '%s\\n%s' "$STUDIO_SESSION_ID" "$PWD" > ${quote(path.join(folder, 'terminal-proof.txt'))}`,
      true,
    );
    await fs.writeFile(
      result,
      JSON.stringify({ sessionId, folder, title, saved: !document.isDirty }),
    );
  } catch (error) {
    await fs.writeFile(result, JSON.stringify({ error: error.message }));
  }
};
