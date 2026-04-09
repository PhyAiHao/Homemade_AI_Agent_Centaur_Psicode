export type IntegrationModule =
  | "bridge"
  | "mcp"
  | "lsp"
  | "tools"
  | "auth"
  | "chrome"
  | "sdk"
  | "transports";

export const AGENT_B_SUBMISSION = "S20B";

export const plannedIntegrationModules: IntegrationModule[] = [
  "bridge",
  "mcp",
  "lsp",
  "tools",
  "auth",
  "chrome",
  "sdk",
  "transports",
];

export function describeIntegrationSurface(): string {
  return plannedIntegrationModules.join(", ");
}

export * from "./contracts.js";
export * from "./ipc_forwarder.js";
export * from "../bridge/index.js";
export * from "../chrome/index.js";
export * from "../lsp/index.js";
export * from "../mcp/index.js";
export * from "../sdk/index.js";
export * from "../tools/index.js";
export * from "../transports/index.js";
export * from "../auth/index.js";
