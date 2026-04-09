/**
 * OAuth 2.0 flow for third-party authentication.
 *
 * Mirrors `src/services/oauth/` from the original TypeScript source.
 * Handles the OAuth 2.0 Authorization Code flow with PKCE for secure
 * authentication against Anthropic's identity provider and third-party
 * services (Google Calendar, Gmail, etc.).
 */

import { randomBytes, createHash } from "node:crypto";
import { createServer, type IncomingMessage, type ServerResponse } from "node:http";

export interface OAuthConfig {
  clientId: string;
  authorizationEndpoint: string;
  tokenEndpoint: string;
  redirectUri: string;
  scopes: string[];
}

export interface OAuthTokens {
  accessToken: string;
  refreshToken?: string;
  expiresAt?: number;
  tokenType: string;
  scope?: string;
}

interface PkceChallenge {
  codeVerifier: string;
  codeChallenge: string;
}

function generatePkceChallenge(): PkceChallenge {
  const codeVerifier = randomBytes(32).toString("base64url");
  const codeChallenge = createHash("sha256")
    .update(codeVerifier)
    .digest("base64url");
  return { codeVerifier, codeChallenge };
}

function generateState(): string {
  return randomBytes(16).toString("hex");
}

export function buildAuthorizationUrl(
  config: OAuthConfig,
  pkce: PkceChallenge,
  state: string,
): string {
  const params = new URLSearchParams({
    response_type: "code",
    client_id: config.clientId,
    redirect_uri: config.redirectUri,
    scope: config.scopes.join(" "),
    state,
    code_challenge: pkce.codeChallenge,
    code_challenge_method: "S256",
  });
  return `${config.authorizationEndpoint}?${params.toString()}`;
}

export async function exchangeCodeForTokens(
  config: OAuthConfig,
  code: string,
  codeVerifier: string,
): Promise<OAuthTokens> {
  const body = new URLSearchParams({
    grant_type: "authorization_code",
    code,
    redirect_uri: config.redirectUri,
    client_id: config.clientId,
    code_verifier: codeVerifier,
  });

  const response = await fetch(config.tokenEndpoint, {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: body.toString(),
  });

  if (!response.ok) {
    const text = await response.text();
    throw new Error(`Token exchange failed (${response.status}): ${text}`);
  }

  const data = (await response.json()) as Record<string, unknown>;

  return {
    accessToken: data.access_token as string,
    refreshToken: data.refresh_token as string | undefined,
    expiresAt: data.expires_in
      ? Date.now() + (data.expires_in as number) * 1000
      : undefined,
    tokenType: (data.token_type as string) || "Bearer",
    scope: data.scope as string | undefined,
  };
}

export async function refreshAccessToken(
  config: OAuthConfig,
  refreshToken: string,
): Promise<OAuthTokens> {
  const body = new URLSearchParams({
    grant_type: "refresh_token",
    refresh_token: refreshToken,
    client_id: config.clientId,
  });

  const response = await fetch(config.tokenEndpoint, {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: body.toString(),
  });

  if (!response.ok) {
    const text = await response.text();
    throw new Error(`Token refresh failed (${response.status}): ${text}`);
  }

  const data = (await response.json()) as Record<string, unknown>;

  return {
    accessToken: data.access_token as string,
    refreshToken: (data.refresh_token as string | undefined) || refreshToken,
    expiresAt: data.expires_in
      ? Date.now() + (data.expires_in as number) * 1000
      : undefined,
    tokenType: (data.token_type as string) || "Bearer",
    scope: data.scope as string | undefined,
  };
}

/**
 * Run a local OAuth callback server on the given port.
 * Returns a promise that resolves with the authorization code when the
 * redirect is received.
 */
export function startCallbackServer(
  port: number,
  expectedState: string,
): Promise<string> {
  return new Promise((resolve, reject) => {
    const server = createServer((req: IncomingMessage, res: ServerResponse) => {
      const url = new URL(req.url || "/", `http://localhost:${port}`);
      const code = url.searchParams.get("code");
      const state = url.searchParams.get("state");
      const error = url.searchParams.get("error");

      if (error) {
        res.writeHead(400, { "Content-Type": "text/html" });
        res.end("<h1>Authentication failed</h1><p>You can close this window.</p>");
        server.close();
        reject(new Error(`OAuth error: ${error}`));
        return;
      }

      if (state !== expectedState) {
        res.writeHead(400, { "Content-Type": "text/html" });
        res.end("<h1>Invalid state</h1><p>You can close this window.</p>");
        server.close();
        reject(new Error("OAuth state mismatch"));
        return;
      }

      if (!code) {
        res.writeHead(400, { "Content-Type": "text/html" });
        res.end("<h1>Missing code</h1><p>You can close this window.</p>");
        server.close();
        reject(new Error("OAuth callback missing authorization code"));
        return;
      }

      res.writeHead(200, { "Content-Type": "text/html" });
      res.end("<h1>Authenticated!</h1><p>You can close this window and return to the terminal.</p>");
      server.close();
      resolve(code);
    });

    server.listen(port, "127.0.0.1");
    server.on("error", reject);

    // Auto-close after 5 minutes to avoid dangling server
    setTimeout(() => {
      server.close();
      reject(new Error("OAuth callback timed out (5 minutes)"));
    }, 5 * 60 * 1000);
  });
}

/**
 * Run the full OAuth 2.0 PKCE flow:
 * 1. Generate PKCE challenge + state
 * 2. Return the authorization URL (caller opens it in a browser)
 * 3. Start local callback server
 * 4. Exchange code for tokens
 */
export async function runOAuthFlow(
  config: OAuthConfig,
  callbackPort: number = 19836,
): Promise<OAuthTokens> {
  const pkce = generatePkceChallenge();
  const state = generateState();
  const authUrl = buildAuthorizationUrl(config, pkce, state);

  // Log the URL — caller is responsible for opening it
  console.error(`Open this URL to authenticate:\n${authUrl}\n`);

  const code = await startCallbackServer(callbackPort, state);
  return exchangeCodeForTokens(config, code, pkce.codeVerifier);
}
