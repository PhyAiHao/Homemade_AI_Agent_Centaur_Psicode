import { resolve } from "node:path";

export type LspServerState =
  | "stopped"
  | "starting"
  | "running"
  | "stopping"
  | "error";

export interface LspScopedServerConfig {
  name: string;
  command: string;
  args?: string[];
  env?: Record<string, string>;
  workspaceFolder?: string;
  extensionToLanguage: Record<string, string>;
  initializationOptions?: Record<string, unknown>;
  startupTimeoutMs?: number;
  requestTimeoutMs?: number;
  shutdownTimeoutMs?: number;
  maxRestartCount?: number;
  transientRetryCount?: number;
  transientRetryBaseDelayMs?: number;
}

export interface LspConfigEnvelope {
  servers: LspScopedServerConfig[];
}

export function normalizeLspServerConfig(
  config: LspScopedServerConfig,
): LspScopedServerConfig {
  const extensionToLanguage = Object.fromEntries(
    Object.entries(config.extensionToLanguage).map(([extension, language]) => [
      normalizeFileExtension(extension),
      language,
    ]),
  );

  return {
    ...config,
    ...(config.args ? { args: [...config.args] } : {}),
    ...(config.env ? { env: { ...config.env } } : {}),
    ...(config.workspaceFolder
      ? { workspaceFolder: resolve(config.workspaceFolder) }
      : {}),
    extensionToLanguage,
  };
}

export function loadLspConfigsFromEnv(
  env: NodeJS.ProcessEnv = process.env,
): LspScopedServerConfig[] {
  const raw = env.CENTAUR_LSP_CONFIG_JSON;
  if (!raw) {
    return [];
  }

  const parsed = JSON.parse(raw) as unknown;
  const configs = Array.isArray(parsed)
    ? parsed
    : typeof parsed === "object" && parsed !== null && Array.isArray((parsed as LspConfigEnvelope).servers)
      ? (parsed as LspConfigEnvelope).servers
      : null;

  if (!configs) {
    throw new Error(
      "CENTAUR_LSP_CONFIG_JSON must be either a JSON array or an object with a servers array.",
    );
  }

  return configs.map(validateAndNormalizeConfig);
}

export function validateAndNormalizeConfig(
  config: unknown,
): LspScopedServerConfig {
  if (typeof config !== "object" || config === null || Array.isArray(config)) {
    throw new Error("LSP server config must be a JSON object.");
  }

  const candidate = config as Record<string, unknown>;
  const name = requireNonEmptyString(candidate.name, "name");
  const command = requireNonEmptyString(candidate.command, "command");
  const extensionToLanguage = requireExtensionMap(candidate.extensionToLanguage);

  return normalizeLspServerConfig({
    name,
    command,
    ...(Array.isArray(candidate.args)
      ? { args: candidate.args.map((value, index) => requireString(value, `args[${index}]`)) }
      : {}),
    ...(isRecord(candidate.env)
      ? {
          env: Object.fromEntries(
            Object.entries(candidate.env).map(([key, value]) => [
              key,
              requireString(value, `env.${key}`),
            ]),
          ),
        }
      : {}),
    ...(typeof candidate.workspaceFolder === "string"
      ? { workspaceFolder: candidate.workspaceFolder }
      : {}),
    extensionToLanguage,
    ...(isRecord(candidate.initializationOptions)
      ? { initializationOptions: candidate.initializationOptions }
      : {}),
    ...(typeof candidate.startupTimeoutMs === "number"
      ? { startupTimeoutMs: candidate.startupTimeoutMs }
      : {}),
    ...(typeof candidate.requestTimeoutMs === "number"
      ? { requestTimeoutMs: candidate.requestTimeoutMs }
      : {}),
    ...(typeof candidate.shutdownTimeoutMs === "number"
      ? { shutdownTimeoutMs: candidate.shutdownTimeoutMs }
      : {}),
    ...(typeof candidate.maxRestartCount === "number"
      ? { maxRestartCount: candidate.maxRestartCount }
      : {}),
    ...(typeof candidate.transientRetryCount === "number"
      ? { transientRetryCount: candidate.transientRetryCount }
      : {}),
    ...(typeof candidate.transientRetryBaseDelayMs === "number"
      ? { transientRetryBaseDelayMs: candidate.transientRetryBaseDelayMs }
      : {}),
  });
}

export function normalizeFileExtension(extension: string): string {
  return extension.startsWith(".")
    ? extension.toLowerCase()
    : `.${extension.toLowerCase()}`;
}

function requireExtensionMap(value: unknown): Record<string, string> {
  if (!isRecord(value) || Object.keys(value).length === 0) {
    throw new Error('Expected "extensionToLanguage" to be a non-empty object.');
  }

  return Object.fromEntries(
    Object.entries(value).map(([extension, language]) => [
      normalizeFileExtension(extension),
      requireString(language, `extensionToLanguage.${extension}`),
    ]),
  );
}

function requireNonEmptyString(value: unknown, fieldName: string): string {
  const text = requireString(value, fieldName);
  if (text.trim() === "") {
    throw new Error(`Expected "${fieldName}" to be a non-empty string.`);
  }
  return text.trim();
}

function requireString(value: unknown, fieldName: string): string {
  if (typeof value !== "string") {
    throw new Error(`Expected "${fieldName}" to be a string.`);
  }
  return value;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
