import type {
  JsonValue,
  ToolExecutionEnvelope,
  ToolInvocationContext,
} from "../src/contracts.js";
import { ToolResultForwarder } from "../src/ipc_forwarder.js";

const DEFAULT_MAX_MARKDOWN_LENGTH = 100_000;
const DEFAULT_MAX_REDIRECTS = 5;
const DEFAULT_USER_AGENT = "Centaur-Agent-Integrations/0.1";

type FetchLike = typeof fetch;

export interface WebFetchInput {
  url: string;
  prompt?: string;
  headers?: Record<string, string>;
  maxChars?: number;
}

export interface WebFetchRedirect {
  originalUrl: string;
  redirectUrl: string;
  status: number;
  crossOrigin: boolean;
}

export interface WebFetchOutput {
  tool: "WebFetch";
  url: string;
  resolvedUrl: string;
  status: number;
  statusText: string;
  contentType: string;
  bytes: number;
  durationMs: number;
  prompt: string;
  title: string;
  markdown: string;
  excerpt: string;
  truncated: boolean;
  redirect?: WebFetchRedirect;
}

export class WebFetchTool {
  constructor(
    private readonly options: {
      fetchImpl?: FetchLike;
      userAgent?: string;
      maxRedirects?: number;
    } = {},
  ) {}

  async execute(
    input: WebFetchInput,
    context?: ToolInvocationContext,
    forwarder?: ToolResultForwarder,
  ): Promise<ToolExecutionEnvelope<WebFetchOutput>> {
    const output = await this.fetchUrl(input, 0, context?.signal);
    if (context && forwarder) {
      await forwarder.forward(context, output);
    }
    return {
      tool: "WebFetch",
      ok: true,
      output,
    };
  }

  private async fetchUrl(
    input: WebFetchInput,
    redirectDepth: number,
    signal?: AbortSignal,
  ): Promise<WebFetchOutput> {
    const startedAt = Date.now();
    const requestedUrl = normalizeUrl(input.url);
    const response = await this.fetchImpl()(requestedUrl, {
      method: "GET",
      redirect: "manual",
      signal,
      headers: {
        "accept":
          "text/html,application/xhtml+xml,text/plain,application/json;q=0.9,*/*;q=0.8",
        "user-agent": this.options.userAgent ?? DEFAULT_USER_AGENT,
        ...input.headers,
      },
    });

    const location = response.headers.get("location");
    if (
      location &&
      response.status >= 300 &&
      response.status < 400 &&
      redirectDepth < (this.options.maxRedirects ?? DEFAULT_MAX_REDIRECTS)
    ) {
      const redirectUrl = new URL(location, requestedUrl).toString();
      const crossOrigin = isCrossOriginRedirect(requestedUrl, redirectUrl);
      if (!crossOrigin) {
        return this.fetchUrl(
          {
            ...input,
            url: redirectUrl,
          },
          redirectDepth + 1,
          signal,
        );
      }

      const markdown =
        "Redirect detected to a different host.\n\n" +
        `Original URL: ${requestedUrl}\n` +
        `Redirect URL: ${redirectUrl}\n` +
        `Status: ${response.status} ${response.statusText}`;
      return {
        tool: "WebFetch",
        url: requestedUrl,
        resolvedUrl: requestedUrl,
        status: response.status,
        statusText: response.statusText,
        contentType: "text/plain",
        bytes: utf8Length(markdown),
        durationMs: Date.now() - startedAt,
        prompt: input.prompt ?? "",
        title: "Redirect detected",
        markdown,
        excerpt: markdown,
        truncated: false,
        redirect: {
          originalUrl: requestedUrl,
          redirectUrl,
          status: response.status,
          crossOrigin: true,
        },
      };
    }

    const bodyBytes = new Uint8Array(await response.arrayBuffer());
    const contentType = response.headers.get("content-type") ?? "application/octet-stream";
    const decodedText = isTextContentType(contentType)
      ? new TextDecoder().decode(bodyBytes)
      : "";
    const title = isHtmlContentType(contentType)
      ? extractHtmlTitle(decodedText)
      : "";
    const markdownSource = isHtmlContentType(contentType)
      ? htmlToMarkdown(decodedText, response.url || requestedUrl)
      : isTextContentType(contentType)
        ? decodedText.trim()
        : `Binary content omitted. Content-Type: ${contentType}`;
    const maxChars = input.maxChars ?? DEFAULT_MAX_MARKDOWN_LENGTH;
    const truncated = markdownSource.length > maxChars;
    const markdown = truncated
      ? `${markdownSource.slice(0, maxChars)}\n\n[truncated]`
      : markdownSource;

    return {
      tool: "WebFetch",
      url: requestedUrl,
      resolvedUrl: response.url || requestedUrl,
      status: response.status,
      statusText: response.statusText,
      contentType,
      bytes: bodyBytes.byteLength,
      durationMs: Date.now() - startedAt,
      prompt: input.prompt ?? "",
      title,
      markdown,
      excerpt: markdown.slice(0, 280),
      truncated,
    };
  }

  private fetchImpl(): FetchLike {
    if (this.options.fetchImpl) {
      return this.options.fetchImpl;
    }
    if (!globalThis.fetch) {
      throw new Error("global fetch is not available in this runtime");
    }
    return globalThis.fetch.bind(globalThis);
  }
}

export function normalizeUrl(url: string): string {
  const parsed = new URL(url);
  if (parsed.protocol === "http:") {
    parsed.protocol = "https:";
  }
  return parsed.toString();
}

export function htmlToMarkdown(html: string, baseUrl?: string): string {
  let text = html;
  text = text.replace(/<!--[\s\S]*?-->/g, "");
  text = text.replace(/<(script|style|noscript|svg)[^>]*>[\s\S]*?<\/\1>/gi, "");
  text = text.replace(/<br\s*\/?>/gi, "\n");
  text = text.replace(/<\/(p|div|section|article|header|footer|aside|main)>/gi, "\n\n");
  text = text.replace(/<(ul|ol)[^>]*>/gi, "\n");
  text = text.replace(/<\/(ul|ol)>/gi, "\n");
  text = text.replace(/<li[^>]*>([\s\S]*?)<\/li>/gi, (_, inner: string) => {
    return `- ${collapseWhitespace(stripTags(inner))}\n`;
  });
  text = text.replace(/<h([1-6])[^>]*>([\s\S]*?)<\/h\1>/gi, (_, level: string, inner: string) => {
    return `${"#".repeat(Number(level))} ${collapseWhitespace(stripTags(inner))}\n\n`;
  });
  text = text.replace(/<pre[^>]*>([\s\S]*?)<\/pre>/gi, (_, inner: string) => {
    return `\n\`\`\`\n${decodeHtmlEntities(stripTags(inner, true)).trim()}\n\`\`\`\n`;
  });
  text = text.replace(/<code[^>]*>([\s\S]*?)<\/code>/gi, (_, inner: string) => {
    return `\`${decodeHtmlEntities(stripTags(inner, true)).trim()}\``;
  });
  text = text.replace(/<a[^>]*href=["']([^"']+)["'][^>]*>([\s\S]*?)<\/a>/gi, (_, href: string, inner: string) => {
    const label = collapseWhitespace(stripTags(inner));
    const resolved = resolveMaybeRelativeUrl(href, baseUrl);
    return label ? `[${label}](${resolved})` : resolved;
  });
  text = text.replace(/<img[^>]*alt=["']([^"']*)["'][^>]*>/gi, (_, alt: string) => {
    return alt ? `![${collapseWhitespace(alt)}]` : "";
  });
  text = decodeHtmlEntities(stripTags(text));
  text = text.replace(/\r\n/g, "\n");
  text = text.replace(/\n{3,}/g, "\n\n");
  text = text
    .split("\n")
    .map(line => line.trimEnd())
    .join("\n")
    .trim();
  return text;
}

function resolveMaybeRelativeUrl(href: string, baseUrl?: string): string {
  if (!baseUrl) {
    return href;
  }
  try {
    return new URL(href, baseUrl).toString();
  } catch {
    return href;
  }
}

function stripTags(text: string, preserveWhitespace = false): string {
  const stripped = text.replace(/<[^>]+>/g, preserveWhitespace ? "" : " ");
  return preserveWhitespace ? stripped : collapseWhitespace(stripped);
}

function collapseWhitespace(text: string): string {
  return decodeHtmlEntities(text).replace(/\s+/g, " ").trim();
}

function decodeHtmlEntities(text: string): string {
  return text
    .replace(/&nbsp;/g, " ")
    .replace(/&amp;/g, "&")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, "\"")
    .replace(/&#39;/g, "'");
}

function extractHtmlTitle(html: string): string {
  const match = html.match(/<title[^>]*>([\s\S]*?)<\/title>/i);
  return match ? collapseWhitespace(match[1]) : "";
}

function isHtmlContentType(contentType: string): boolean {
  return /text\/html|application\/xhtml\+xml/i.test(contentType);
}

function isTextContentType(contentType: string): boolean {
  return (
    isHtmlContentType(contentType) ||
    /^text\//i.test(contentType) ||
    /application\/(json|xml|javascript|x-www-form-urlencoded)/i.test(contentType)
  );
}

function isCrossOriginRedirect(originalUrl: string, redirectUrl: string): boolean {
  return new URL(originalUrl).origin !== new URL(redirectUrl).origin;
}

function utf8Length(value: string): number {
  return new TextEncoder().encode(value).byteLength;
}

export function asWebFetchJson(output: WebFetchOutput): JsonValue {
  return output as unknown as JsonValue;
}
