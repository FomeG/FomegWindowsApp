// Cách nối MCP chỉ đọc (fomeg-windows-app-mcp.exe) vào từng ứng dụng AI. `path` là đường dẫn exe do backend tính cho MÁY NÀY
// (bản phát hành: %LOCALAPPDATA%\fomeg-windows-app\fomeg-windows-app-mcp.exe của người dùng hiện tại), nên mỗi người thấy đúng của họ.
// Mỗi client: name, steps (các bước), file (tệp cấu hình), blocks: [{ label, lang, text(path) }].
// steps / file / label là getter dùng t() nên đọc trong template là tự đổi theo ngôn ngữ.
import { t } from './i18n.svelte.js';

export const MCP_NAME = 'fomeg-windows-app';

const json = (obj) => JSON.stringify(obj, null, 2);
const mcpServers = (path) => json({ mcpServers: { [MCP_NAME]: { command: path } } });
/** Chuỗi TOML: dạng '…' (không cần thoát "\"), trừ khi đường dẫn có dấu nháy đơn. */
const tomlStr = (s) => (s.includes("'") ? JSON.stringify(s) : `'${s}'`);
const merge = () => t('settings.mcp.merge', { name: MCP_NAME });
/** Các bước của client `id`: khoá settings.mcp.<id>.s1..s3 (tham số {merge}, {name}). */
const steps = (id) => [1, 2, 3].map((i) => t(`settings.mcp.${id}.s${i}`, { merge: merge(), name: MCP_NAME }));
/** Khối mã: nhãn theo khoá (Lệnh, Cấu hình JSON, Hoặc …). */
const block = (key, lang, text, params) => ({ get label() { return t(key, params); }, lang, text });
const cmd = (text) => block('settings.mcp.block.cmd', 'powershell', text);
const cfgJson = (text = mcpServers) => block('settings.mcp.block.json', 'json', text);

export const MCP_CLIENTS = [
  {
    id: 'claude-code',
    name: 'Claude Code',
    get steps() { return steps('claude_code'); },
    get file() { return t('settings.mcp.claude_code.file', { file: '%USERPROFILE%\\.claude.json' }); },
    blocks: [cmd((p) => `claude mcp add --scope user ${MCP_NAME} -- "${p}"`)],
  },
  {
    id: 'claude-desktop',
    name: 'Claude Desktop',
    get steps() { return steps('claude_desktop'); },
    file: '%APPDATA%\\Claude\\claude_desktop_config.json',
    blocks: [cfgJson()],
  },
  {
    id: 'codex',
    get name() { return t('settings.mcp.codex.name'); },
    get steps() { return steps('codex'); },
    file: '%USERPROFILE%\\.codex\\config.toml',
    blocks: [
      cmd((p) => `codex mcp add ${MCP_NAME} -- "${p}"`),
      block('settings.mcp.block.or', 'toml', (p) => `[mcp_servers.${MCP_NAME}]\ncommand = ${tomlStr(p)}`, { file: 'config.toml' }),
    ],
  },
  {
    id: 'antigravity',
    name: 'Google Antigravity',
    get steps() { return steps('antigravity'); },
    file: '%USERPROFILE%\\.gemini\\antigravity\\mcp_config.json',
    blocks: [cfgJson()],
  },
  {
    id: 'gemini',
    name: 'Gemini CLI',
    get steps() { return steps('gemini'); },
    file: '%USERPROFILE%\\.gemini\\settings.json',
    blocks: [
      cmd((p) => `gemini mcp add --scope user ${MCP_NAME} "${p}"`),
      block('settings.mcp.block.or', 'json', mcpServers, { file: 'settings.json' }),
    ],
  },
  {
    id: 'cursor',
    name: 'Cursor',
    get steps() { return steps('cursor'); },
    file: '%USERPROFILE%\\.cursor\\mcp.json',
    blocks: [cfgJson()],
  },
  {
    id: 'vscode',
    name: 'VS Code (Copilot)',
    get steps() { return steps('vscode'); },
    file: '%APPDATA%\\Code\\User\\mcp.json',
    blocks: [cfgJson((p) => json({ servers: { [MCP_NAME]: { type: 'stdio', command: p } } }))],
  },
  {
    id: 'windsurf',
    name: 'Windsurf',
    get steps() { return steps('windsurf'); },
    file: '%USERPROFILE%\\.codeium\\windsurf\\mcp_config.json',
    blocks: [cfgJson()],
  },
];
