import { BridgeMainLoop } from "./bridgeMain.js";
import { createRemoteBridge, RemoteBridge } from "./remoteBridge.js";
import type {
  BridgeMainLoopConfig,
  ConnectedRemoteBridge,
  RemoteBridgeConnectOptions,
} from "./types.js";
import { HybridTransport } from "../transports/hybrid.js";

type FetchLike = typeof fetch;

export function createBridgeRuntime(options: {
  baseUrl: string;
  getAccessToken: () => string | undefined | Promise<string | undefined>;
  fetchImpl?: FetchLike;
  hybrid?: HybridTransport;
  config?: BridgeMainLoopConfig;
}): {
  remoteBridge: RemoteBridge;
  mainLoop: BridgeMainLoop;
} {
  const remoteBridge = createRemoteBridge({
    baseUrl: options.baseUrl,
    getAccessToken: options.getAccessToken,
    fetchImpl: options.fetchImpl,
    hybrid: options.hybrid,
  });
  const mainLoop = new BridgeMainLoop(remoteBridge, options.config);
  return {
    remoteBridge,
    mainLoop,
  };
}

export async function initializeBridge(options: {
  baseUrl: string;
  getAccessToken: () => string | undefined | Promise<string | undefined>;
  fetchImpl?: FetchLike;
  hybrid?: HybridTransport;
  config?: BridgeMainLoopConfig;
  initialSession?: RemoteBridgeConnectOptions;
}): Promise<{
  remoteBridge: RemoteBridge;
  mainLoop: BridgeMainLoop;
  session?: ConnectedRemoteBridge;
}> {
  const runtime = createBridgeRuntime(options);
  if (!options.initialSession) {
    return runtime;
  }
  const session = await runtime.mainLoop.startSession(options.initialSession);
  return {
    ...runtime,
    session,
  };
}
