import type {
  ToolExecutionEnvelope,
  ToolInvocationContext,
} from "../src/contracts.js";
import { ToolResultForwarder } from "../src/ipc_forwarder.js";

type FetchLike = typeof fetch;

const DEFAULT_SEARCH_ENDPOINT = "https://api.duckduckgo.com/";

export interface WebSearchInput {
  query: string;
  allowedDomains?: string[];
  blockedDomains?: string[];
  maxResults?: number;
}

export interface WebSearchResult {
  title: string;
  url: string;
  snippet: string;
  domain: string;
}

export interface WebSearchOutput {
  tool: "WebSearch";
  query: string;
  provider: string;
  durationMs: number;
  totalResults: number;
  results: WebSearchResult[];
}

export interface SearchProvider {
  readonly name: string;
  search(
    input: WebSearchInput,
    signal?: AbortSignal,
  ): Promise<WebSearchResult[]>;
}

export class DuckDuckGoSearchProvider implements SearchProvider {
  readonly name = "duckduckgo_instant_answer";

  constructor(
    private readonly options: {
      endpoint?: string;
      fetchImpl?: FetchLike;
    } = {},
  ) {}

  async search(
    input: WebSearchInput,
    signal?: AbortSignal,
  ): Promise<WebSearchResult[]> {
    const url = new URL(this.options.endpoint ?? DEFAULT_SEARCH_ENDPOINT);
    url.searchParams.set("q", input.query);
    url.searchParams.set("format", "json");
    url.searchParams.set("no_html", "1");
    url.searchParams.set("no_redirect", "1");
    url.searchParams.set("skip_disambig", "1");

    const response = await this.fetchImpl()(url, {
      method: "GET",
      signal,
      headers: {
        "accept": "application/json",
      },
    });
    if (!response.ok) {
      throw new Error(`Web search failed with ${response.status} ${response.statusText}`);
    }

    const payload = (await response.json()) as Record<string, unknown>;
    return parseDuckDuckGoResults(payload);
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

export class WebSearchTool {
  constructor(
    private readonly options: {
      provider?: SearchProvider;
    } = {},
  ) {}

  async execute(
    input: WebSearchInput,
    context?: ToolInvocationContext,
    forwarder?: ToolResultForwarder,
  ): Promise<ToolExecutionEnvelope<WebSearchOutput>> {
    validateSearchInput(input);
    const startedAt = Date.now();
    const provider = this.options.provider ?? new DuckDuckGoSearchProvider();
    const rawResults = await provider.search(input, context?.signal);
    const results = applyDomainFilters(rawResults, input).slice(
      0,
      input.maxResults ?? 8,
    );
    const output: WebSearchOutput = {
      tool: "WebSearch",
      query: input.query,
      provider: provider.name,
      durationMs: Date.now() - startedAt,
      totalResults: results.length,
      results,
    };
    if (context && forwarder) {
      await forwarder.forward(context, output);
    }
    return {
      tool: "WebSearch",
      ok: true,
      output,
    };
  }
}

export function validateSearchInput(input: WebSearchInput): void {
  if (input.query.trim().length < 2) {
    throw new Error("Web search query must be at least 2 characters long.");
  }
  if (input.allowedDomains?.length && input.blockedDomains?.length) {
    throw new Error(
      "allowedDomains and blockedDomains cannot be used together.",
    );
  }
}

export function applyDomainFilters(
  results: WebSearchResult[],
  input: WebSearchInput,
): WebSearchResult[] {
  const allowed = normalizeDomainList(input.allowedDomains);
  const blocked = normalizeDomainList(input.blockedDomains);

  return results.filter(result => {
    const domain = result.domain.toLowerCase();
    if (allowed.length > 0) {
      return allowed.some(candidate => domain === candidate || domain.endsWith(`.${candidate}`));
    }
    if (blocked.length > 0) {
      return !blocked.some(candidate => domain === candidate || domain.endsWith(`.${candidate}`));
    }
    return true;
  });
}

function normalizeDomainList(value?: string[]): string[] {
  return (value ?? [])
    .map(item => item.trim().toLowerCase())
    .filter(Boolean);
}

function parseDuckDuckGoResults(payload: Record<string, unknown>): WebSearchResult[] {
  const results: WebSearchResult[] = [];
  const seen = new Set<string>();

  const pushResult = (title: string, url: string, snippet: string): void => {
    if (!url || seen.has(url)) {
      return;
    }
    try {
      const parsedUrl = new URL(url);
      seen.add(url);
      results.push({
        title: title || parsedUrl.hostname,
        url,
        snippet,
        domain: parsedUrl.hostname,
      });
    } catch {
      // Ignore malformed URLs.
    }
  };

  const abstractUrl = asString(payload["AbstractURL"]);
  const abstractText = asString(payload["AbstractText"]);
  const heading = asString(payload["Heading"]);
  if (abstractUrl) {
    pushResult(heading || abstractUrl, abstractUrl, abstractText);
  }

  collectTopicResults(payload["Results"], pushResult);
  collectTopicResults(payload["RelatedTopics"], pushResult);
  return results;
}

function collectTopicResults(
  value: unknown,
  pushResult: (title: string, url: string, snippet: string) => void,
): void {
  if (!Array.isArray(value)) {
    return;
  }

  for (const item of value) {
    if (!item || typeof item !== "object") {
      continue;
    }

    if ("Topics" in item) {
      collectTopicResults((item as Record<string, unknown>)["Topics"], pushResult);
      continue;
    }

    const record = item as Record<string, unknown>;
    const url = asString(record["FirstURL"]);
    const text = asString(record["Text"]);
    if (!url) {
      continue;
    }
    const title = text.includes(" - ") ? text.split(" - ")[0] : text;
    pushResult(title, url, text);
  }
}

function asString(value: unknown): string {
  return typeof value === "string" ? value : "";
}
