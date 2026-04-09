/**
 * Centaur Psicode VS Code Extension
 *
 * Architecture:
 *   VS Code Extension  <--WebSocket-->  Agent Bridge (Rust)
 *                                           |
 *                                       IPC (msgpack)
 *                                           |
 *                                      Python Brain --> LLM API
 *
 * The extension:
 *   1. Writes a lockfile so the agent CLI can discover it
 *   2. Starts a WebSocket server on a random port
 *   3. Spawns the agent CLI (make dev) as a child process
 *   4. Provides a chat sidebar webview
 *   5. Routes messages between the webview and the agent
 */

import * as vscode from 'vscode';
import * as http from 'http';
import * as path from 'path';
import * as fs from 'fs';
import * as os from 'os';
import * as net from 'net';
import * as child_process from 'child_process';
import { WebSocketServer, WebSocket } from 'ws';

// ── State ──────────────────────────────────────────────────────────────────

let wsServer: WebSocketServer | undefined;
let httpServer: http.Server | undefined;
let agentProcess: child_process.ChildProcess | undefined;
let agentSocket: WebSocket | undefined;
let statusBarItem: vscode.StatusBarItem;
let chatProvider: ChatViewProvider | undefined;
let lockfilePath: string | undefined;

// ── Activation ─────────────────────────────────────────────────────────────

export function activate(context: vscode.ExtensionContext) {
  // Status bar
  statusBarItem = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 100);
  statusBarItem.text = '$(hubot) Centaur';
  statusBarItem.tooltip = 'Centaur Psicode AI Agent — Click to start';
  statusBarItem.command = 'centaur-psicode.startAgent';
  statusBarItem.show();
  context.subscriptions.push(statusBarItem);

  // Chat sidebar
  chatProvider = new ChatViewProvider(context.extensionUri);
  context.subscriptions.push(
    vscode.window.registerWebviewViewProvider('centaur-psicode.chat', chatProvider)
  );

  // Commands
  context.subscriptions.push(
    vscode.commands.registerCommand('centaur-psicode.startAgent', () => startAgent(context)),
    vscode.commands.registerCommand('centaur-psicode.stopAgent', stopAgent),
    vscode.commands.registerCommand('centaur-psicode.sendSelection', sendSelection),
  );

  // Auto-start if workspace is open
  if (vscode.workspace.workspaceFolders?.length) {
    startAgent(context);
  }
}

export function deactivate() {
  stopAgent();
}

// ── Agent Lifecycle ────────────────────────────────────────────────────────

async function startAgent(context: vscode.ExtensionContext) {
  if (agentProcess) {
    vscode.window.showInformationMessage('Agent is already running.');
    return;
  }

  const workspaceFolder = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
  if (!workspaceFolder) {
    vscode.window.showErrorMessage('Open a folder first.');
    return;
  }

  updateStatus('starting');

  // 1. Find a free port for the WebSocket server
  const port = await findFreePort();

  // 2. Start WebSocket server
  httpServer = http.createServer();
  wsServer = new WebSocketServer({ server: httpServer });

  wsServer.on('connection', (ws) => {
    agentSocket = ws;
    updateStatus('connected');
    chatProvider?.postMessage({ type: 'status', status: 'connected' });

    ws.on('message', (data) => {
      try {
        const msg = JSON.parse(data.toString());
        handleAgentMessage(msg);
      } catch (e) {
        console.error('Invalid message from agent:', e);
      }
    });

    ws.on('close', () => {
      agentSocket = undefined;
      updateStatus('disconnected');
      chatProvider?.postMessage({ type: 'status', status: 'disconnected' });
    });
  });

  httpServer.listen(port, '127.0.0.1', () => {
    console.log(`Centaur Psicode WS server on port ${port}`);
  });

  // 3. Write lockfile for agent discovery
  writeLockfile(port, workspaceFolder);

  // 4. Spawn the agent CLI
  const agentDir = findAgentDir(workspaceFolder);
  if (!agentDir) {
    vscode.window.showErrorMessage(
      'Could not find agent-core directory. Make sure the project has a Makefile.'
    );
    stopAgent();
    return;
  }

  agentProcess = child_process.spawn('make', ['dev'], {
    cwd: agentDir,
    env: {
      ...process.env,
      CENTAUR_IDE_PORT: String(port),
      CENTAUR_IDE_WORKSPACE: workspaceFolder,
    },
    stdio: ['pipe', 'pipe', 'pipe'],
    shell: true,
  });

  agentProcess.stdout?.on('data', (data: Buffer) => {
    const text = data.toString().trim();
    if (text) {
      console.log('[agent stdout]', text);
    }
  });

  agentProcess.stderr?.on('data', (data: Buffer) => {
    const text = data.toString().trim();
    if (text) {
      console.log('[agent stderr]', text);
    }
  });

  agentProcess.on('exit', (code) => {
    console.log(`Agent process exited with code ${code}`);
    agentProcess = undefined;
    updateStatus('stopped');
    chatProvider?.postMessage({ type: 'status', status: 'stopped' });
  });

  updateStatus('running');
}

function stopAgent() {
  // Kill agent process
  if (agentProcess) {
    agentProcess.kill('SIGTERM');
    agentProcess = undefined;
  }

  // Close WebSocket
  if (agentSocket) {
    agentSocket.close();
    agentSocket = undefined;
  }

  // Close server
  if (wsServer) {
    wsServer.close();
    wsServer = undefined;
  }
  if (httpServer) {
    httpServer.close();
    httpServer = undefined;
  }

  // Remove lockfile
  if (lockfilePath && fs.existsSync(lockfilePath)) {
    fs.unlinkSync(lockfilePath);
    lockfilePath = undefined;
  }

  updateStatus('stopped');
}

// ── Message Handling ───────────────────────────────────────────────────────

function handleAgentMessage(msg: any) {
  switch (msg.type) {
    case 'text_delta':
      chatProvider?.postMessage({ type: 'text_delta', delta: msg.delta });
      break;
    case 'assistant_message':
      chatProvider?.postMessage({ type: 'assistant_message', content: msg.content });
      break;
    case 'tool_start':
      chatProvider?.postMessage({ type: 'tool_start', name: msg.name });
      break;
    case 'tool_done':
      chatProvider?.postMessage({ type: 'tool_done', name: msg.name, success: !msg.is_error });
      break;
    case 'done':
      chatProvider?.postMessage({ type: 'done' });
      break;
    case 'error':
      chatProvider?.postMessage({ type: 'error', message: msg.message });
      vscode.window.showErrorMessage(`Agent error: ${msg.message}`);
      break;
    case 'request_permission':
      handlePermissionRequest(msg);
      break;
    case 'file_changed':
      // Refresh the editor if a file was modified by the agent
      if (msg.path) {
        const uri = vscode.Uri.file(msg.path);
        vscode.workspace.fs.stat(uri).then(() => {
          vscode.commands.executeCommand('workbench.files.action.refreshFilesExplorer');
        });
      }
      break;
  }
}

function sendToAgent(msg: any) {
  if (agentSocket?.readyState === WebSocket.OPEN) {
    agentSocket.send(JSON.stringify(msg));
  }
}

async function handlePermissionRequest(msg: any) {
  const choice = await vscode.window.showWarningMessage(
    `Agent wants to use ${msg.tool_name}:\n${msg.description || ''}`,
    { modal: true },
    'Allow',
    'Allow Always',
    'Deny'
  );

  sendToAgent({
    type: 'permission_response',
    request_id: msg.request_id,
    approved: choice === 'Allow' || choice === 'Allow Always',
    always: choice === 'Allow Always',
  });
}

function sendSelection() {
  const editor = vscode.window.activeTextEditor;
  if (!editor) {return;}

  const selection = editor.document.getText(editor.selection);
  const filePath = editor.document.uri.fsPath;
  const lineStart = editor.selection.start.line + 1;
  const lineEnd = editor.selection.end.line + 1;

  if (selection) {
    const prompt = `Here is code from ${path.basename(filePath)}:${lineStart}-${lineEnd}:\n\`\`\`\n${selection}\n\`\`\``;
    sendToAgent({ type: 'user_message', content: prompt });
    chatProvider?.postMessage({ type: 'user_message', content: prompt });
  }
}

// ── IDE Discovery (Lockfile) ───────────────────────────────────────────────

function getLockfileDir(): string {
  const dir = path.join(os.homedir(), '.centaur-psicode', 'ide');
  fs.mkdirSync(dir, { recursive: true });
  return dir;
}

function writeLockfile(port: number, workspaceFolder: string) {
  const dir = getLockfileDir();
  lockfilePath = path.join(dir, `vscode_${port}.json`);

  const lockfileData = {
    workspaceFolders: [workspaceFolder],
    port,
    pid: process.pid,
    ideName: 'VS Code',
    transport: 'ws',
    createdAt: new Date().toISOString(),
  };

  fs.writeFileSync(lockfilePath, JSON.stringify(lockfileData, null, 2));
}

// ── Utilities ──────────────────────────────────────────────────────────────

function findFreePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const srv = net.createServer();
    srv.listen(0, '127.0.0.1', () => {
      const addr = srv.address();
      if (addr && typeof addr === 'object') {
        const port = addr.port;
        srv.close(() => resolve(port));
      } else {
        reject(new Error('Could not find free port'));
      }
    });
  });
}

function findAgentDir(workspaceFolder: string): string | null {
  // Look for the Makefile in the workspace or its parent directories
  let dir = workspaceFolder;
  for (let i = 0; i < 5; i++) {
    if (fs.existsSync(path.join(dir, 'Makefile')) && fs.existsSync(path.join(dir, 'agent-core'))) {
      return dir;
    }
    if (fs.existsSync(path.join(dir, 'Homemade_AI_Agent_v1', 'Makefile'))) {
      return path.join(dir, 'Homemade_AI_Agent_v1');
    }
    dir = path.dirname(dir);
  }
  return null;
}

function updateStatus(state: 'starting' | 'running' | 'connected' | 'disconnected' | 'stopped') {
  const icons: Record<string, string> = {
    starting: '$(loading~spin)',
    running: '$(hubot)',
    connected: '$(check)',
    disconnected: '$(warning)',
    stopped: '$(circle-slash)',
  };
  const labels: Record<string, string> = {
    starting: 'Starting...',
    running: 'Running',
    connected: 'Connected',
    disconnected: 'Disconnected',
    stopped: 'Stopped',
  };
  statusBarItem.text = `${icons[state] || '$(hubot)'} Centaur: ${labels[state] || state}`;
  statusBarItem.command = state === 'stopped' ? 'centaur-psicode.startAgent' : 'centaur-psicode.stopAgent';
}

// ── Chat Sidebar Webview ───────────────────────────────────────────────────

class ChatViewProvider implements vscode.WebviewViewProvider {
  private _view?: vscode.WebviewView;

  constructor(private readonly _extensionUri: vscode.Uri) {}

  resolveWebviewView(webviewView: vscode.WebviewView) {
    this._view = webviewView;

    webviewView.webview.options = {
      enableScripts: true,
      localResourceRoots: [this._extensionUri],
    };

    webviewView.webview.html = this._getHtml();

    // Handle messages from the webview (user input)
    webviewView.webview.onDidReceiveMessage((msg) => {
      if (msg.type === 'user_message') {
        sendToAgent({ type: 'user_message', content: msg.content });
      }
    });
  }

  postMessage(msg: any) {
    this._view?.webview.postMessage(msg);
  }

  private _getHtml(): string {
    return `<!DOCTYPE html>
<html>
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width,initial-scale=1.0">
<style>
  * { margin: 0; padding: 0; box-sizing: border-box; }
  body {
    font-family: var(--vscode-font-family);
    font-size: var(--vscode-font-size);
    color: var(--vscode-foreground);
    background: var(--vscode-sideBar-background);
    display: flex;
    flex-direction: column;
    height: 100vh;
  }

  #status {
    padding: 6px 12px;
    font-size: 11px;
    color: var(--vscode-descriptionForeground);
    border-bottom: 1px solid var(--vscode-panel-border);
  }
  #status.connected { color: var(--vscode-charts-green); }
  #status.error { color: var(--vscode-errorForeground); }

  #messages {
    flex: 1;
    overflow-y: auto;
    padding: 8px;
  }

  .msg {
    margin-bottom: 12px;
    padding: 8px 10px;
    border-radius: 6px;
    white-space: pre-wrap;
    word-break: break-word;
    line-height: 1.4;
  }
  .msg.user {
    background: var(--vscode-input-background);
    border-left: 3px solid var(--vscode-charts-blue);
  }
  .msg.assistant {
    background: var(--vscode-editor-background);
    border-left: 3px solid var(--vscode-charts-green);
  }
  .msg.system {
    font-size: 11px;
    color: var(--vscode-descriptionForeground);
    font-style: italic;
  }
  .msg.tool {
    font-size: 11px;
    color: var(--vscode-descriptionForeground);
    padding: 4px 10px;
  }
  .msg.error {
    color: var(--vscode-errorForeground);
    border-left: 3px solid var(--vscode-errorForeground);
  }

  #streaming {
    padding: 8px 10px;
    background: var(--vscode-editor-background);
    border-left: 3px solid var(--vscode-charts-yellow);
    margin: 0 8px;
    border-radius: 6px;
    white-space: pre-wrap;
    display: none;
  }

  #input-area {
    display: flex;
    padding: 8px;
    gap: 6px;
    border-top: 1px solid var(--vscode-panel-border);
  }
  #input {
    flex: 1;
    padding: 6px 10px;
    border: 1px solid var(--vscode-input-border);
    background: var(--vscode-input-background);
    color: var(--vscode-input-foreground);
    border-radius: 4px;
    font-family: inherit;
    font-size: inherit;
    resize: none;
    min-height: 32px;
    max-height: 120px;
  }
  #input:focus { outline: 1px solid var(--vscode-focusBorder); }
  #send {
    padding: 6px 14px;
    background: var(--vscode-button-background);
    color: var(--vscode-button-foreground);
    border: none;
    border-radius: 4px;
    cursor: pointer;
    font-size: inherit;
    align-self: flex-end;
  }
  #send:hover { background: var(--vscode-button-hoverBackground); }
</style>
</head>
<body>
  <div id="status">Waiting for agent...</div>
  <div id="messages"></div>
  <div id="streaming"></div>
  <div id="input-area">
    <textarea id="input" rows="1" placeholder="Ask the AI agent..."></textarea>
    <button id="send">Send</button>
  </div>

<script>
  const vscode = acquireVsCodeApi();
  const messagesEl = document.getElementById('messages');
  const streamingEl = document.getElementById('streaming');
  const statusEl = document.getElementById('status');
  const inputEl = document.getElementById('input');
  const sendBtn = document.getElementById('send');
  let streamingText = '';

  function addMessage(role, content) {
    const div = document.createElement('div');
    div.className = 'msg ' + role;
    div.textContent = content;
    messagesEl.appendChild(div);
    messagesEl.scrollTop = messagesEl.scrollHeight;
  }

  function send() {
    const text = inputEl.value.trim();
    if (!text) return;
    addMessage('user', text);
    vscode.postMessage({ type: 'user_message', content: text });
    inputEl.value = '';
    inputEl.style.height = '32px';
  }

  sendBtn.addEventListener('click', send);
  inputEl.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      send();
    }
  });
  // Auto-resize textarea
  inputEl.addEventListener('input', () => {
    inputEl.style.height = '32px';
    inputEl.style.height = Math.min(inputEl.scrollHeight, 120) + 'px';
  });

  // Handle messages from extension
  window.addEventListener('message', (event) => {
    const msg = event.data;
    switch (msg.type) {
      case 'status':
        statusEl.textContent = msg.status === 'connected'
          ? 'Agent connected'
          : msg.status === 'stopped'
          ? 'Agent stopped'
          : 'Status: ' + msg.status;
        statusEl.className = msg.status === 'connected' ? 'connected' : '';
        break;

      case 'text_delta':
        streamingText += msg.delta;
        streamingEl.textContent = streamingText;
        streamingEl.style.display = 'block';
        messagesEl.scrollTop = messagesEl.scrollHeight;
        break;

      case 'assistant_message':
        // Finalize streamed content
        if (streamingText) {
          addMessage('assistant', streamingText);
          streamingText = '';
          streamingEl.style.display = 'none';
          streamingEl.textContent = '';
        } else if (msg.content) {
          addMessage('assistant', msg.content);
        }
        break;

      case 'tool_start':
        addMessage('tool', '> Running: ' + msg.name + '...');
        break;
      case 'tool_done':
        addMessage('tool', (msg.success ? '> Done' : '> Error') + ': ' + msg.name);
        break;

      case 'done':
        if (streamingText) {
          addMessage('assistant', streamingText);
          streamingText = '';
          streamingEl.style.display = 'none';
          streamingEl.textContent = '';
        }
        break;

      case 'error':
        addMessage('error', 'Error: ' + msg.message);
        break;

      case 'user_message':
        addMessage('user', msg.content);
        break;
    }
  });
</script>
</body>
</html>`;
  }
}
