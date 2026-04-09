import {
  getChromeFocusTabUrl,
  trackClaudeInChromeTabId,
} from "./common.js";
import type { ChromeToolName } from "./types.js";

function truncate(value: string, maxLength = 32): string {
  if (value.length <= maxLength) {
    return value;
  }
  return `${value.slice(0, maxLength - 3)}...`;
}

export function summarizeChromeToolUse(
  input: Record<string, unknown>,
  toolName: ChromeToolName,
  verbose = false,
): string | null {
  const tabId = input.tabId;
  if (typeof tabId === "number") {
    trackClaudeInChromeTabId(tabId);
  }

  if (toolName === "javascript_tool") {
    if (verbose && typeof input.text === "string") {
      return input.text;
    }
    return "Running browser script";
  }

  if (toolName === "navigate" && typeof input.url === "string") {
    try {
      return `Navigating to ${new URL(input.url).hostname}`;
    } catch {
      return `Navigating to ${truncate(input.url)}`;
    }
  }

  if (toolName === "find" && typeof input.query === "string") {
    return `Finding ${truncate(input.query)}`;
  }

  if (toolName === "computer" && typeof input.action === "string") {
    return `Browser action: ${input.action}`;
  }

  if (toolName === "resize_window") {
    const width = typeof input.width === "number" ? input.width : null;
    const height = typeof input.height === "number" ? input.height : null;
    if (width && height) {
      return `Resizing browser to ${width}x${height}`;
    }
  }

  if (
    toolName === "read_console_messages" &&
    typeof input.pattern === "string"
  ) {
    return `Reading console logs matching ${truncate(input.pattern)}`;
  }

  const fallbackSummaries: Partial<Record<ChromeToolName, string>> = {
    tabs_context_mcp: "Reading browser tabs",
    tabs_create_mcp: "Creating browser tab",
    read_page: "Reading active page",
    get_page_text: "Extracting page text",
    form_input: "Filling browser form",
    gif_creator: "Recording browser GIF",
    upload_image: "Uploading image",
    update_plan: "Updating browser plan",
    read_network_requests: "Reading network requests",
    shortcuts_list: "Listing browser shortcuts",
    shortcuts_execute: "Executing browser shortcut",
  };

  return fallbackSummaries[toolName] ?? null;
}

export function summarizeChromeToolResult(
  _output: unknown,
  toolName: ChromeToolName,
  verbose = false,
): string | null {
  if (verbose) {
    return null;
  }
  const resultSummaries: Partial<Record<ChromeToolName, string>> = {
    navigate: "Navigation completed",
    tabs_create_mcp: "Tab created",
    tabs_context_mcp: "Tab context retrieved",
    form_input: "Form input completed",
    computer: "Browser action completed",
    resize_window: "Window resized",
    find: "Search completed",
    gif_creator: "GIF action completed",
    read_console_messages: "Console messages retrieved",
    read_network_requests: "Network requests retrieved",
    shortcuts_list: "Shortcuts retrieved",
    shortcuts_execute: "Shortcut executed",
    javascript_tool: "Script executed",
    read_page: "Page read",
    upload_image: "Image uploaded",
    get_page_text: "Page text retrieved",
    update_plan: "Plan updated",
  };

  return resultSummaries[toolName] ?? null;
}

export function getChromeViewTabUrl(input: unknown): string | null {
  if (typeof input !== "object" || input === null || !("tabId" in input)) {
    return null;
  }
  const value = (input as { tabId?: unknown }).tabId;
  const tabId =
    typeof value === "number"
      ? value
      : typeof value === "string"
        ? Number.parseInt(value, 10)
        : Number.NaN;
  if (!Number.isFinite(tabId)) {
    return null;
  }
  trackClaudeInChromeTabId(tabId);
  return getChromeFocusTabUrl(tabId);
}
