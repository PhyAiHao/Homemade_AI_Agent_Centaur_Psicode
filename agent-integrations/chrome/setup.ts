import { mkdir, readdir, writeFile } from "node:fs/promises";
import { homedir } from "node:os";
import { join } from "node:path";

import {
  CLAUDE_IN_CHROME_MCP_SERVER_NAME,
  getAllBrowserDataPaths,
  getAllNativeMessagingHostsDirs,
} from "./common.js";
import { getChromeSystemPrompt } from "./prompt.js";
import {
  CHROME_TOOL_NAMES,
  type ChromeNativeHostManifest,
  type ChromeSetupResult,
} from "./types.js";

export const CHROME_EXTENSION_URL = "https://claude.ai/chrome";
export const CHROME_EXTENSION_RECONNECT_URL = "https://clau.de/chrome/reconnect";
export const NATIVE_HOST_IDENTIFIER = "com.anthropic.claude_code_browser_extension";
export const NATIVE_HOST_MANIFEST_NAME = `${NATIVE_HOST_IDENTIFIER}.json`;

const PROD_EXTENSION_ID = "fcoeoabgfenejglbffodgkkbkcdhcgfn";
const DEV_EXTENSION_ID = "dihbgbndebgnbjfmelmegjepbnkhlgni";
const ANT_EXTENSION_ID = "dngcpimnedloihjnnfngkgjoidhnaolf";

export interface ShouldEnableChromeOptions {
  chromeFlag?: boolean;
  defaultEnabled?: boolean;
  nonInteractive?: boolean;
  env?: NodeJS.ProcessEnv;
}

export interface InstallChromeManifestOptions {
  userType?: string;
  platform?: NodeJS.Platform;
  homeDir?: string;
}

export interface ChromeExtensionDetectionResult {
  isInstalled: boolean;
  browser: string | null;
}

export interface SetupClaudeInChromeOptions {
  command: string;
  args?: string[];
  env?: Record<string, string>;
  serverName?: string;
}

function getExtensionIds(userType = process.env.USER_TYPE): string[] {
  return userType === "ant"
    ? [PROD_EXTENSION_ID, DEV_EXTENSION_ID, ANT_EXTENSION_ID]
    : [PROD_EXTENSION_ID];
}

function getAllowedOrigins(userType = process.env.USER_TYPE): string[] {
  return getExtensionIds(userType).map(
    extensionId => `chrome-extension://${extensionId}/`,
  );
}

export function shouldEnableClaudeInChrome(
  options: ShouldEnableChromeOptions = {},
): boolean {
  const env = options.env ?? process.env;
  if (options.nonInteractive && options.chromeFlag !== true) {
    return false;
  }
  if (options.chromeFlag === true) {
    return true;
  }
  if (options.chromeFlag === false) {
    return false;
  }
  if (env.CLAUDE_CODE_ENABLE_CFC === "1") {
    return true;
  }
  if (env.CLAUDE_CODE_ENABLE_CFC === "0") {
    return false;
  }
  return options.defaultEnabled ?? false;
}

export function shouldAutoEnableClaudeInChrome(options: {
  interactive: boolean;
  extensionInstalled: boolean;
  growthbookEnabled?: boolean;
  userType?: string;
}): boolean {
  return (
    options.interactive &&
    options.extensionInstalled &&
    (options.userType === "ant" || options.growthbookEnabled === true)
  );
}

export function buildChromeNativeHostManifest(
  manifestBinaryPath: string,
  options: InstallChromeManifestOptions = {},
): ChromeNativeHostManifest {
  return {
    name: NATIVE_HOST_IDENTIFIER,
    description: "Centaur Claude-in-Chrome Native Host",
    path: manifestBinaryPath,
    type: "stdio",
    allowed_origins: getAllowedOrigins(options.userType),
  };
}

function getNativeMessagingHostDirs(
  platform: NodeJS.Platform = process.platform,
  homeDir = homedir(),
): string[] {
  if (platform === "win32") {
    const appData = process.env.APPDATA || join(homeDir, "AppData", "Local");
    return [join(appData, "Centaur", "ChromeNativeHost")];
  }
  return getAllNativeMessagingHostsDirs(platform, homeDir).map(item => item.path);
}

export async function installChromeNativeHostManifest(
  manifestBinaryPath: string,
  options: InstallChromeManifestOptions = {},
): Promise<string[]> {
  const manifest = buildChromeNativeHostManifest(manifestBinaryPath, options);
  const manifestDirs = getNativeMessagingHostDirs(
    options.platform,
    options.homeDir,
  );
  const paths: string[] = [];

  for (const manifestDir of manifestDirs) {
    await mkdir(manifestDir, { recursive: true });
    const manifestPath = join(manifestDir, NATIVE_HOST_MANIFEST_NAME);
    await writeFile(manifestPath, JSON.stringify(manifest, null, 2), "utf8");
    paths.push(manifestPath);
  }

  return paths;
}

export async function detectExtensionInstallation(
  browserPaths = getAllBrowserDataPaths(),
  userType = process.env.USER_TYPE,
): Promise<ChromeExtensionDetectionResult> {
  const extensionIds = getExtensionIds(userType);

  for (const { browser, path: browserBasePath } of browserPaths) {
    let profileEntries;
    try {
      profileEntries = await readdir(browserBasePath, { withFileTypes: true });
    } catch {
      continue;
    }

    const profileDirs = profileEntries
      .filter(entry => entry.isDirectory())
      .filter(
        entry => entry.name === "Default" || entry.name.startsWith("Profile "),
      )
      .map(entry => entry.name);

    for (const profileDir of profileDirs) {
      for (const extensionId of extensionIds) {
        try {
          await readdir(
            join(browserBasePath, profileDir, "Extensions", extensionId),
          );
          return { isInstalled: true, browser };
        } catch {
          continue;
        }
      }
    }
  }

  return { isInstalled: false, browser: null };
}

export async function isChromeExtensionInstalled(
  browserPaths = getAllBrowserDataPaths(),
  userType = process.env.USER_TYPE,
): Promise<boolean> {
  const detection = await detectExtensionInstallation(browserPaths, userType);
  return detection.isInstalled;
}

export function setupClaudeInChrome(
  options: SetupClaudeInChromeOptions,
): ChromeSetupResult {
  const serverName = options.serverName ?? CLAUDE_IN_CHROME_MCP_SERVER_NAME;
  const allowedTools = CHROME_TOOL_NAMES.map(
    toolName => `mcp__${serverName}__${toolName}`,
  );

  return {
    mcpConfig: {
      [serverName]: {
        type: "stdio",
        command: options.command,
        args: options.args ?? [],
        env: options.env,
        scope: "dynamic",
      },
    },
    allowedTools,
    systemPrompt: getChromeSystemPrompt(),
  };
}
