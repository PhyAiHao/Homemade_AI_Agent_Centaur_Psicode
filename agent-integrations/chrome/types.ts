export type ChromiumBrowser =
  | "chrome"
  | "brave"
  | "arc"
  | "chromium"
  | "edge"
  | "vivaldi"
  | "opera";

export interface BrowserPath {
  browser: ChromiumBrowser;
  path: string;
}

export interface BrowserRegistryKey {
  browser: ChromiumBrowser;
  key: string;
}

export const CHROME_TOOL_NAMES = [
  "javascript_tool",
  "read_page",
  "find",
  "form_input",
  "computer",
  "navigate",
  "resize_window",
  "gif_creator",
  "upload_image",
  "get_page_text",
  "tabs_context_mcp",
  "tabs_create_mcp",
  "update_plan",
  "read_console_messages",
  "read_network_requests",
  "shortcuts_list",
  "shortcuts_execute",
] as const;

export type ChromeToolName = (typeof CHROME_TOOL_NAMES)[number];

export interface ChromeToolDescriptor {
  name: ChromeToolName;
  description: string;
}

export interface ChromeNativeHostManifest {
  name: string;
  description: string;
  path: string;
  type: "stdio";
  allowed_origins: string[];
}

export interface ChromeMcpServerConfig {
  type: "stdio";
  command: string;
  args: string[];
  env?: Record<string, string>;
  scope?: "dynamic" | "session";
}

export interface ChromeSetupResult {
  mcpConfig: Record<string, ChromeMcpServerConfig>;
  allowedTools: string[];
  systemPrompt: string;
}

export type ChromeInboundMessage =
  | { type: "ping" }
  | { type: "get_status" }
  | ({ type: "tool_response" } & Record<string, unknown>)
  | ({ type: "notification" } & Record<string, unknown>);

export type ChromeOutboundMessage =
  | { type: "pong"; timestamp: number }
  | { type: "status_response"; native_host_version: string }
  | { type: "tool_request"; method: string; params?: unknown }
  | { type: "mcp_connected" }
  | { type: "mcp_disconnected" }
  | { type: "error"; error: string };
