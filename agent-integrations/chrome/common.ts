import { spawn } from "node:child_process";
import { stat } from "node:fs/promises";
import { homedir, tmpdir, userInfo } from "node:os";
import { join } from "node:path";

import type { BrowserPath, BrowserRegistryKey, ChromiumBrowser } from "./types.js";

export const CLAUDE_IN_CHROME_MCP_SERVER_NAME = "claude-in-chrome";
export const CHROME_EXTENSION_FOCUS_TAB_URL_BASE = "https://clau.de/chrome/tab/";

type BrowserConfig = {
  name: string;
  macos: {
    appName: string;
    dataPath: string[];
    nativeMessagingPath: string[];
  };
  linux: {
    binaries: string[];
    dataPath: string[];
    nativeMessagingPath: string[];
  };
  windows: {
    dataPath: string[];
    registryKey: string;
    useRoaming?: boolean;
  };
};

export const CHROMIUM_BROWSERS: Record<ChromiumBrowser, BrowserConfig> = {
  chrome: {
    name: "Google Chrome",
    macos: {
      appName: "Google Chrome",
      dataPath: ["Library", "Application Support", "Google", "Chrome"],
      nativeMessagingPath: [
        "Library",
        "Application Support",
        "Google",
        "Chrome",
        "NativeMessagingHosts",
      ],
    },
    linux: {
      binaries: ["google-chrome", "google-chrome-stable"],
      dataPath: [".config", "google-chrome"],
      nativeMessagingPath: [".config", "google-chrome", "NativeMessagingHosts"],
    },
    windows: {
      dataPath: ["Google", "Chrome", "User Data"],
      registryKey: "HKCU\\Software\\Google\\Chrome\\NativeMessagingHosts",
    },
  },
  brave: {
    name: "Brave",
    macos: {
      appName: "Brave Browser",
      dataPath: ["Library", "Application Support", "BraveSoftware", "Brave-Browser"],
      nativeMessagingPath: [
        "Library",
        "Application Support",
        "BraveSoftware",
        "Brave-Browser",
        "NativeMessagingHosts",
      ],
    },
    linux: {
      binaries: ["brave-browser", "brave"],
      dataPath: [".config", "BraveSoftware", "Brave-Browser"],
      nativeMessagingPath: [
        ".config",
        "BraveSoftware",
        "Brave-Browser",
        "NativeMessagingHosts",
      ],
    },
    windows: {
      dataPath: ["BraveSoftware", "Brave-Browser", "User Data"],
      registryKey:
        "HKCU\\Software\\BraveSoftware\\Brave-Browser\\NativeMessagingHosts",
    },
  },
  arc: {
    name: "Arc",
    macos: {
      appName: "Arc",
      dataPath: ["Library", "Application Support", "Arc", "User Data"],
      nativeMessagingPath: [
        "Library",
        "Application Support",
        "Arc",
        "User Data",
        "NativeMessagingHosts",
      ],
    },
    linux: {
      binaries: [],
      dataPath: [],
      nativeMessagingPath: [],
    },
    windows: {
      dataPath: ["Arc", "User Data"],
      registryKey: "HKCU\\Software\\ArcBrowser\\Arc\\NativeMessagingHosts",
    },
  },
  chromium: {
    name: "Chromium",
    macos: {
      appName: "Chromium",
      dataPath: ["Library", "Application Support", "Chromium"],
      nativeMessagingPath: [
        "Library",
        "Application Support",
        "Chromium",
        "NativeMessagingHosts",
      ],
    },
    linux: {
      binaries: ["chromium", "chromium-browser"],
      dataPath: [".config", "chromium"],
      nativeMessagingPath: [".config", "chromium", "NativeMessagingHosts"],
    },
    windows: {
      dataPath: ["Chromium", "User Data"],
      registryKey: "HKCU\\Software\\Chromium\\NativeMessagingHosts",
    },
  },
  edge: {
    name: "Microsoft Edge",
    macos: {
      appName: "Microsoft Edge",
      dataPath: ["Library", "Application Support", "Microsoft Edge"],
      nativeMessagingPath: [
        "Library",
        "Application Support",
        "Microsoft Edge",
        "NativeMessagingHosts",
      ],
    },
    linux: {
      binaries: ["microsoft-edge", "microsoft-edge-stable"],
      dataPath: [".config", "microsoft-edge"],
      nativeMessagingPath: [".config", "microsoft-edge", "NativeMessagingHosts"],
    },
    windows: {
      dataPath: ["Microsoft", "Edge", "User Data"],
      registryKey: "HKCU\\Software\\Microsoft\\Edge\\NativeMessagingHosts",
    },
  },
  vivaldi: {
    name: "Vivaldi",
    macos: {
      appName: "Vivaldi",
      dataPath: ["Library", "Application Support", "Vivaldi"],
      nativeMessagingPath: [
        "Library",
        "Application Support",
        "Vivaldi",
        "NativeMessagingHosts",
      ],
    },
    linux: {
      binaries: ["vivaldi", "vivaldi-stable"],
      dataPath: [".config", "vivaldi"],
      nativeMessagingPath: [".config", "vivaldi", "NativeMessagingHosts"],
    },
    windows: {
      dataPath: ["Vivaldi", "User Data"],
      registryKey: "HKCU\\Software\\Vivaldi\\NativeMessagingHosts",
    },
  },
  opera: {
    name: "Opera",
    macos: {
      appName: "Opera",
      dataPath: ["Library", "Application Support", "com.operasoftware.Opera"],
      nativeMessagingPath: [
        "Library",
        "Application Support",
        "com.operasoftware.Opera",
        "NativeMessagingHosts",
      ],
    },
    linux: {
      binaries: ["opera"],
      dataPath: [".config", "opera"],
      nativeMessagingPath: [".config", "opera", "NativeMessagingHosts"],
    },
    windows: {
      dataPath: ["Opera Software", "Opera Stable"],
      registryKey: "HKCU\\Software\\Opera Software\\Opera Stable\\NativeMessagingHosts",
      useRoaming: true,
    },
  },
};

export const BROWSER_DETECTION_ORDER: ChromiumBrowser[] = [
  "chrome",
  "brave",
  "arc",
  "edge",
  "chromium",
  "vivaldi",
  "opera",
];

function normalizePlatform(
  platform: NodeJS.Platform,
): "macos" | "linux" | "windows" {
  if (platform === "darwin") {
    return "macos";
  }
  if (platform === "win32") {
    return "windows";
  }
  return "linux";
}

function getUsername(): string {
  try {
    return userInfo().username.replace(/[^A-Za-z0-9._-]/g, "_");
  } catch {
    return "user";
  }
}

export function normalizeNameForMcp(name: string): string {
  return name
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

export function isClaudeInChromeMCPServer(name: string): boolean {
  return normalizeNameForMcp(name) === CLAUDE_IN_CHROME_MCP_SERVER_NAME;
}

export function getAllBrowserDataPaths(
  platform: NodeJS.Platform = process.platform,
  homeDir = homedir(),
): BrowserPath[] {
  const normalized = normalizePlatform(platform);
  const paths: BrowserPath[] = [];

  for (const browser of BROWSER_DETECTION_ORDER) {
    const config = CHROMIUM_BROWSERS[browser];
    if (normalized === "windows") {
      const appDataBase = config.windows.useRoaming
        ? join(homeDir, "AppData", "Roaming")
        : join(homeDir, "AppData", "Local");
      paths.push({
        browser,
        path: join(appDataBase, ...config.windows.dataPath),
      });
      continue;
    }

    const browserPath =
      normalized === "macos" ? config.macos.dataPath : config.linux.dataPath;
    if (browserPath.length > 0) {
      paths.push({ browser, path: join(homeDir, ...browserPath) });
    }
  }

  return paths;
}

export function getAllNativeMessagingHostsDirs(
  platform: NodeJS.Platform = process.platform,
  homeDir = homedir(),
): BrowserPath[] {
  const normalized = normalizePlatform(platform);
  const paths: BrowserPath[] = [];
  if (normalized === "windows") {
    return paths;
  }

  for (const browser of BROWSER_DETECTION_ORDER) {
    const config = CHROMIUM_BROWSERS[browser];
    const browserPath =
      normalized === "macos"
        ? config.macos.nativeMessagingPath
        : config.linux.nativeMessagingPath;
    if (browserPath.length > 0) {
      paths.push({ browser, path: join(homeDir, ...browserPath) });
    }
  }

  return paths;
}

export function getAllWindowsRegistryKeys(): BrowserRegistryKey[] {
  return BROWSER_DETECTION_ORDER.map(browser => ({
    browser,
    key: CHROMIUM_BROWSERS[browser].windows.registryKey,
  }));
}

async function pathExists(path: string): Promise<boolean> {
  try {
    await stat(path);
    return true;
  } catch {
    return false;
  }
}

export async function detectAvailableBrowser(
  platform: NodeJS.Platform = process.platform,
  homeDir = homedir(),
): Promise<ChromiumBrowser | null> {
  const normalized = normalizePlatform(platform);
  const dataPaths = getAllBrowserDataPaths(platform, homeDir);

  for (const candidate of dataPaths) {
    if (normalized === "linux") {
      const binaries = CHROMIUM_BROWSERS[candidate.browser].linux.binaries;
      for (const bin of binaries) {
        if (await pathExists(bin)) {
          return candidate.browser;
        }
      }
      continue;
    }
    if (await pathExists(candidate.path)) {
      return candidate.browser;
    }
  }

  return null;
}

export function buildOpenInChromeCommand(
  url: string,
  browser: ChromiumBrowser,
  platform: NodeJS.Platform = process.platform,
): { command: string; args: string[] } | null {
  const normalized = normalizePlatform(platform);
  const config = CHROMIUM_BROWSERS[browser];

  if (normalized === "macos") {
    return {
      command: "open",
      args: ["-a", config.macos.appName, url],
    };
  }
  if (normalized === "windows") {
    return {
      command: "rundll32",
      args: ["url,OpenURL", url],
    };
  }
  const linuxBinary = config.linux.binaries[0];
  if (!linuxBinary) {
    return null;
  }
  return {
    command: linuxBinary,
    args: [url],
  };
}

export async function openInChrome(
  url: string,
  browser?: ChromiumBrowser,
): Promise<boolean> {
  const chosenBrowser = browser ?? (await detectAvailableBrowser());
  if (!chosenBrowser) {
    return false;
  }

  const command = buildOpenInChromeCommand(url, chosenBrowser);
  if (!command) {
    return false;
  }

  return await new Promise(resolve => {
    const child = spawn(command.command, command.args, {
      stdio: "ignore",
      detached: process.platform !== "win32",
    });

    child.once("error", () => resolve(false));
    child.once("spawn", () => {
      if (process.platform !== "win32") {
        child.unref();
      }
      resolve(true);
    });
  });
}

export function getSocketName(username = getUsername()): string {
  return `claude-mcp-browser-bridge-${username}`;
}

export function getSocketDir(username = getUsername()): string {
  return join(tmpdir(), getSocketName(username));
}

export function getSecureSocketPath(
  platform: NodeJS.Platform = process.platform,
  pid = process.pid,
  username = getUsername(),
): string {
  if (platform === "win32") {
    return `\\\\.\\pipe\\${getSocketName(username)}`;
  }
  return join(getSocketDir(username), `${pid}.sock`);
}

const trackedTabIds = new Set<number>();

export function trackClaudeInChromeTabId(tabId: number): void {
  if (trackedTabIds.size >= 200 && !trackedTabIds.has(tabId)) {
    trackedTabIds.clear();
  }
  trackedTabIds.add(tabId);
}

export function isTrackedClaudeInChromeTabId(tabId: number): boolean {
  return trackedTabIds.has(tabId);
}

export function getChromeFocusTabUrl(tabId: number): string {
  return `${CHROME_EXTENSION_FOCUS_TAB_URL_BASE}${tabId}`;
}
