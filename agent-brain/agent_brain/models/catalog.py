from __future__ import annotations

from typing import Any, Literal

from ..types.base import AgentBaseModel

APIProvider = Literal["first_party", "bedrock", "vertex", "foundry", "ollama", "openai", "gemini"]

DEFAULT_CONTEXT_WINDOW = 200_000


class ModelPricing(AgentBaseModel):
    input_tokens_per_million: float
    output_tokens_per_million: float
    prompt_cache_write_tokens_per_million: float
    prompt_cache_read_tokens_per_million: float
    web_search_request_usd: float = 0.01


class ProviderModelIds(AgentBaseModel):
    first_party: str
    bedrock: str
    vertex: str
    foundry: str
    ollama: str | None = None


class ModelDescriptor(AgentBaseModel):
    key: str
    family: Literal["haiku", "sonnet", "opus"]
    provider_ids: ProviderModelIds
    context_window: int = DEFAULT_CONTEXT_WINDOW
    default_max_output_tokens: int
    upper_max_output_tokens: int
    pricing: ModelPricing
    supports_1m_context: bool = False
    supports_fast_mode: bool = False
    description: str = ""


class ResolvedModel(AgentBaseModel):
    descriptor: ModelDescriptor
    resolved_model: str
    provider: APIProvider
    context_window: int
    default_max_output_tokens: int
    upper_max_output_tokens: int
    requested_model: str


COST_TIER_3_15 = ModelPricing(
    input_tokens_per_million=3,
    output_tokens_per_million=15,
    prompt_cache_write_tokens_per_million=3.75,
    prompt_cache_read_tokens_per_million=0.3,
)

COST_TIER_15_75 = ModelPricing(
    input_tokens_per_million=15,
    output_tokens_per_million=75,
    prompt_cache_write_tokens_per_million=18.75,
    prompt_cache_read_tokens_per_million=1.5,
)

COST_TIER_5_25 = ModelPricing(
    input_tokens_per_million=5,
    output_tokens_per_million=25,
    prompt_cache_write_tokens_per_million=6.25,
    prompt_cache_read_tokens_per_million=0.5,
)

COST_TIER_30_150 = ModelPricing(
    input_tokens_per_million=30,
    output_tokens_per_million=150,
    prompt_cache_write_tokens_per_million=37.5,
    prompt_cache_read_tokens_per_million=3,
)

COST_HAIKU_35 = ModelPricing(
    input_tokens_per_million=0.8,
    output_tokens_per_million=4,
    prompt_cache_write_tokens_per_million=1,
    prompt_cache_read_tokens_per_million=0.08,
)

COST_HAIKU_45 = ModelPricing(
    input_tokens_per_million=1,
    output_tokens_per_million=5,
    prompt_cache_write_tokens_per_million=1.25,
    prompt_cache_read_tokens_per_million=0.1,
)


MODEL_CATALOG: dict[str, ModelDescriptor] = {
    "haiku35": ModelDescriptor(
        key="haiku35",
        family="haiku",
        provider_ids=ProviderModelIds(
            first_party="claude-3-5-haiku-20241022",
            bedrock="us.anthropic.claude-3-5-haiku-20241022-v1:0",
            vertex="claude-3-5-haiku@20241022",
            foundry="claude-3-5-haiku",
            ollama="claude-3-5-haiku",
        ),
        default_max_output_tokens=8_192,
        upper_max_output_tokens=8_192,
        pricing=COST_HAIKU_35,
        description="Haiku 3.5 for simple tasks.",
    ),
    "haiku45": ModelDescriptor(
        key="haiku45",
        family="haiku",
        provider_ids=ProviderModelIds(
            first_party="claude-haiku-4-5-20251001",
            bedrock="us.anthropic.claude-haiku-4-5-20251001-v1:0",
            vertex="claude-haiku-4-5@20251001",
            foundry="claude-haiku-4-5",
            ollama="claude-haiku-4-5",
        ),
        default_max_output_tokens=32_000,
        upper_max_output_tokens=64_000,
        pricing=COST_HAIKU_45,
        description="Haiku 4.5 for the fastest low-cost responses.",
    ),
    "sonnet35": ModelDescriptor(
        key="sonnet35",
        family="sonnet",
        provider_ids=ProviderModelIds(
            first_party="claude-3-5-sonnet-20241022",
            bedrock="anthropic.claude-3-5-sonnet-20241022-v2:0",
            vertex="claude-3-5-sonnet-v2@20241022",
            foundry="claude-3-5-sonnet",
            ollama="claude-3-5-sonnet",
        ),
        default_max_output_tokens=8_192,
        upper_max_output_tokens=8_192,
        pricing=COST_TIER_3_15,
        description="Claude 3.5 Sonnet legacy line.",
    ),
    "sonnet37": ModelDescriptor(
        key="sonnet37",
        family="sonnet",
        provider_ids=ProviderModelIds(
            first_party="claude-3-7-sonnet-20250219",
            bedrock="us.anthropic.claude-3-7-sonnet-20250219-v1:0",
            vertex="claude-3-7-sonnet@20250219",
            foundry="claude-3-7-sonnet",
            ollama="claude-3-7-sonnet",
        ),
        default_max_output_tokens=32_000,
        upper_max_output_tokens=64_000,
        pricing=COST_TIER_3_15,
        description="Claude 3.7 Sonnet.",
    ),
    "sonnet40": ModelDescriptor(
        key="sonnet40",
        family="sonnet",
        provider_ids=ProviderModelIds(
            first_party="claude-sonnet-4-20250514",
            bedrock="us.anthropic.claude-sonnet-4-20250514-v1:0",
            vertex="claude-sonnet-4@20250514",
            foundry="claude-sonnet-4",
            ollama="claude-sonnet-4",
        ),
        default_max_output_tokens=32_000,
        upper_max_output_tokens=64_000,
        pricing=COST_TIER_3_15,
        supports_1m_context=True,
        description="Claude Sonnet 4.",
    ),
    "sonnet45": ModelDescriptor(
        key="sonnet45",
        family="sonnet",
        provider_ids=ProviderModelIds(
            first_party="claude-sonnet-4-5-20250929",
            bedrock="us.anthropic.claude-sonnet-4-5-20250929-v1:0",
            vertex="claude-sonnet-4-5@20250929",
            foundry="claude-sonnet-4-5",
            ollama="claude-sonnet-4-5",
        ),
        default_max_output_tokens=32_000,
        upper_max_output_tokens=64_000,
        pricing=COST_TIER_3_15,
        supports_1m_context=True,
        description="Claude Sonnet 4.5.",
    ),
    "sonnet46": ModelDescriptor(
        key="sonnet46",
        family="sonnet",
        provider_ids=ProviderModelIds(
            first_party="claude-sonnet-4-6",
            bedrock="us.anthropic.claude-sonnet-4-6",
            vertex="claude-sonnet-4-6",
            foundry="claude-sonnet-4-6",
            ollama="claude-sonnet-4-6",
        ),
        default_max_output_tokens=32_000,
        upper_max_output_tokens=128_000,
        pricing=COST_TIER_3_15,
        supports_1m_context=True,
        description="Claude Sonnet 4.6, the default everyday model.",
    ),
    "opus40": ModelDescriptor(
        key="opus40",
        family="opus",
        provider_ids=ProviderModelIds(
            first_party="claude-opus-4-20250514",
            bedrock="us.anthropic.claude-opus-4-20250514-v1:0",
            vertex="claude-opus-4@20250514",
            foundry="claude-opus-4",
            ollama="claude-opus-4",
        ),
        default_max_output_tokens=32_000,
        upper_max_output_tokens=32_000,
        pricing=COST_TIER_15_75,
        description="Claude Opus 4.",
    ),
    "opus41": ModelDescriptor(
        key="opus41",
        family="opus",
        provider_ids=ProviderModelIds(
            first_party="claude-opus-4-1-20250805",
            bedrock="us.anthropic.claude-opus-4-1-20250805-v1:0",
            vertex="claude-opus-4-1@20250805",
            foundry="claude-opus-4-1",
            ollama="claude-opus-4-1",
        ),
        default_max_output_tokens=32_000,
        upper_max_output_tokens=32_000,
        pricing=COST_TIER_15_75,
        description="Claude Opus 4.1.",
    ),
    "opus45": ModelDescriptor(
        key="opus45",
        family="opus",
        provider_ids=ProviderModelIds(
            first_party="claude-opus-4-5-20251101",
            bedrock="us.anthropic.claude-opus-4-5-20251101-v1:0",
            vertex="claude-opus-4-5@20251101",
            foundry="claude-opus-4-5",
            ollama="claude-opus-4-5",
        ),
        default_max_output_tokens=32_000,
        upper_max_output_tokens=64_000,
        pricing=COST_TIER_5_25,
        description="Claude Opus 4.5.",
    ),
    "opus46": ModelDescriptor(
        key="opus46",
        family="opus",
        provider_ids=ProviderModelIds(
            first_party="claude-opus-4-6",
            bedrock="us.anthropic.claude-opus-4-6-v1",
            vertex="claude-opus-4-6",
            foundry="claude-opus-4-6",
            ollama="claude-opus-4-6",
        ),
        default_max_output_tokens=64_000,
        upper_max_output_tokens=128_000,
        pricing=COST_TIER_5_25,
        supports_1m_context=True,
        supports_fast_mode=True,
        description="Claude Opus 4.6, the strongest reasoning model in the catalog.",
    ),
}


MODEL_ALIASES: dict[str, str] = {
    "best": "opus46",
    "haiku": "haiku45",
    "sonnet": "sonnet46",
    "opus": "opus46",
    "haiku[1m]": "haiku45",
    "sonnet[1m]": "sonnet46",
    "opus[1m]": "opus46",
}


def _strip_1m_suffix(model_name: str) -> tuple[str, bool]:
    normalized = model_name.strip()
    if normalized.lower().endswith("[1m]"):
        return normalized[:-4].strip(), True
    return normalized, False


def get_model_catalog() -> dict[str, ModelDescriptor]:
    return MODEL_CATALOG


def iter_model_descriptors() -> list[ModelDescriptor]:
    return list(MODEL_CATALOG.values())


def get_model_descriptor(key: str) -> ModelDescriptor | None:
    return MODEL_CATALOG.get(key)


def resolve_model(model_name: str, provider: APIProvider = "first_party") -> ResolvedModel:
    base_name, wants_1m = _strip_1m_suffix(model_name.lower())

    if base_name in MODEL_ALIASES:
        descriptor = MODEL_CATALOG[MODEL_ALIASES[base_name]]
    elif base_name in MODEL_CATALOG:
        descriptor = MODEL_CATALOG[base_name]
    else:
        descriptor = _match_descriptor(base_name)

    if descriptor is None:
        raise ValueError(f"Unknown model: {model_name}")

    resolved_model = getattr(descriptor.provider_ids, provider)
    if resolved_model is None:
        raise ValueError(f"Model {descriptor.key} does not expose a {provider} identifier")

    context_window = 1_000_000 if wants_1m and descriptor.supports_1m_context else descriptor.context_window
    if wants_1m and descriptor.supports_1m_context:
        resolved_model = f"{resolved_model}[1m]"

    return ResolvedModel(
        descriptor=descriptor,
        resolved_model=resolved_model,
        provider=provider,
        context_window=context_window,
        default_max_output_tokens=descriptor.default_max_output_tokens,
        upper_max_output_tokens=descriptor.upper_max_output_tokens,
        requested_model=model_name,
    )


def _match_descriptor(model_name: str) -> ModelDescriptor | None:
    exact_by_provider: dict[str, ModelDescriptor] = {}
    for descriptor in MODEL_CATALOG.values():
        for provider in ("first_party", "bedrock", "vertex", "foundry", "ollama"):
            provider_model = getattr(descriptor.provider_ids, provider)
            if provider_model is None:
                continue
            exact_by_provider[provider_model.lower()] = descriptor

    if model_name in exact_by_provider:
        return exact_by_provider[model_name]

    best_match: ModelDescriptor | None = None
    best_length = -1
    for descriptor in MODEL_CATALOG.values():
        for provider in ("first_party", "bedrock", "vertex", "foundry", "ollama"):
            provider_model = getattr(descriptor.provider_ids, provider)
            if provider_model is None:
                continue
            needle = provider_model.lower()
            if needle in model_name and len(needle) > best_length:
                best_match = descriptor
                best_length = len(needle)
    return best_match


def calculate_cost_usd(
    model_name: str,
    usage: dict[str, Any],
    *,
    provider: APIProvider = "first_party",
    fast_mode: bool = False,
) -> float:
    try:
        resolved = resolve_model(model_name, provider=provider)
    except ValueError:
        # Unknown model (e.g. Ollama local models) — no cost
        return 0.0
    descriptor = resolved.descriptor
    pricing = descriptor.pricing
    if descriptor.key == "opus46" and descriptor.supports_fast_mode and fast_mode:
        pricing = COST_TIER_30_150

    input_tokens = int(usage.get("input_tokens", 0) or 0)
    output_tokens = int(usage.get("output_tokens", 0) or 0)
    cache_read = int(usage.get("cache_read_input_tokens", 0) or 0)
    cache_write = int(usage.get("cache_creation_input_tokens", 0) or 0)
    web_search = int(usage.get("web_search_requests", 0) or 0)

    return (
        (input_tokens / 1_000_000) * pricing.input_tokens_per_million
        + (output_tokens / 1_000_000) * pricing.output_tokens_per_million
        + (cache_write / 1_000_000) * pricing.prompt_cache_write_tokens_per_million
        + (cache_read / 1_000_000) * pricing.prompt_cache_read_tokens_per_million
        + web_search * pricing.web_search_request_usd
    )


def get_default_model_key(
    subscriber_tier: Literal[
        "free",
        "payg",
        "pro",
        "max",
        "team_standard",
        "team_premium",
        "enterprise",
    ] = "payg",
) -> str:
    return "opus46" if subscriber_tier in {"max", "team_premium"} else "sonnet46"
