from __future__ import annotations

from dataclasses import dataclass
from enum import Enum
from typing import Any

try:
    from enum import StrEnum
except ImportError:  # pragma: no cover - Python < 3.11
    class StrEnum(str, Enum):
        pass

try:
    import httpx
except ImportError:  # pragma: no cover - dependency is declared in pyproject
    httpx = None  # type: ignore[assignment]


SSL_ERROR_CODES = {
    "UNABLE_TO_VERIFY_LEAF_SIGNATURE",
    "UNABLE_TO_GET_ISSUER_CERT",
    "UNABLE_TO_GET_ISSUER_CERT_LOCALLY",
    "CERT_SIGNATURE_FAILURE",
    "CERT_NOT_YET_VALID",
    "CERT_HAS_EXPIRED",
    "CERT_REVOKED",
    "CERT_REJECTED",
    "CERT_UNTRUSTED",
    "DEPTH_ZERO_SELF_SIGNED_CERT",
    "SELF_SIGNED_CERT_IN_CHAIN",
    "CERT_CHAIN_TOO_LONG",
    "PATH_LENGTH_EXCEEDED",
    "ERR_TLS_CERT_ALTNAME_INVALID",
    "HOSTNAME_MISMATCH",
    "ERR_TLS_HANDSHAKE_TIMEOUT",
    "ERR_SSL_WRONG_VERSION_NUMBER",
    "ERR_SSL_DECRYPTION_FAILED_OR_BAD_RECORD_MAC",
}


class ApiErrorCategory(StrEnum):
    ABORTED = "aborted"
    API_TIMEOUT = "api_timeout"
    RATE_LIMIT = "rate_limit"
    SERVER_OVERLOAD = "server_overload"
    PROMPT_TOO_LONG = "prompt_too_long"
    PDF_TOO_LARGE = "pdf_too_large"
    PDF_PASSWORD_PROTECTED = "pdf_password_protected"
    IMAGE_TOO_LARGE = "image_too_large"
    TOOL_USE_MISMATCH = "tool_use_mismatch"
    INVALID_MODEL = "invalid_model"
    CREDIT_BALANCE_LOW = "credit_balance_low"
    INVALID_API_KEY = "invalid_api_key"
    TOKEN_REVOKED = "token_revoked"
    OAUTH_ORG_NOT_ALLOWED = "oauth_org_not_allowed"
    AUTH_ERROR = "auth_error"
    SERVER_ERROR = "server_error"
    CLIENT_ERROR = "client_error"
    SSL_CERT_ERROR = "ssl_cert_error"
    CONNECTION_ERROR = "connection_error"
    UNKNOWN = "unknown"


@dataclass
class ConnectionErrorDetails:
    code: str
    message: str
    is_ssl_error: bool


class AgentApiError(RuntimeError):
    def __init__(
        self,
        message: str,
        *,
        category: ApiErrorCategory,
        status_code: int | None = None,
        retryable: bool | None = None,
        raw: BaseException | None = None,
    ) -> None:
        super().__init__(message)
        self.category = category
        self.status_code = status_code
        self.retryable = (
            retryable if retryable is not None else category in {ApiErrorCategory.RATE_LIMIT, ApiErrorCategory.SERVER_ERROR}
        )
        self.raw = raw

    @classmethod
    def from_exception(cls, error: BaseException) -> "AgentApiError":
        return cls(
            format_api_error(error),
            category=classify_api_error(error),
            status_code=extract_status_code(error),
            retryable=is_retryable_api_error(error),
            raw=error,
        )


def classify_api_error(error: BaseException | Exception | Any) -> ApiErrorCategory:
    message = extract_error_message(error).lower()
    status_code = extract_status_code(error)

    if isinstance(error, KeyboardInterrupt) or message == "request was aborted.":
        return ApiErrorCategory.ABORTED

    if _is_timeout(error, message):
        return ApiErrorCategory.API_TIMEOUT

    if status_code == 429:
        return ApiErrorCategory.RATE_LIMIT

    if status_code == 529 or '"type":"overloaded_error"' in message:
        return ApiErrorCategory.SERVER_OVERLOAD

    if "prompt is too long" in message:
        return ApiErrorCategory.PROMPT_TOO_LONG

    if "maximum of" in message and "pdf pages" in message:
        return ApiErrorCategory.PDF_TOO_LARGE

    if "password protected" in message and "pdf" in message:
        return ApiErrorCategory.PDF_PASSWORD_PROTECTED

    if "image exceeds" in message and "maximum" in message:
        return ApiErrorCategory.IMAGE_TOO_LARGE

    if "image dimensions exceed" in message and "many-image" in message:
        return ApiErrorCategory.IMAGE_TOO_LARGE

    if "`tool_use` ids were found without `tool_result`" in message:
        return ApiErrorCategory.TOOL_USE_MISMATCH

    if "unexpected `tool_use_id` found in `tool_result`" in message:
        return ApiErrorCategory.TOOL_USE_MISMATCH

    if "invalid model name" in message or "unknown model" in message:
        return ApiErrorCategory.INVALID_MODEL

    if "credit balance is too low" in message:
        return ApiErrorCategory.CREDIT_BALANCE_LOW

    if "x-api-key" in message or "invalid api key" in message:
        return ApiErrorCategory.INVALID_API_KEY

    if "oauth token has been revoked" in message:
        return ApiErrorCategory.TOKEN_REVOKED

    if "oauth authentication is currently not allowed for this organization" in message:
        return ApiErrorCategory.OAUTH_ORG_NOT_ALLOWED

    if status_code in {401, 403}:
        return ApiErrorCategory.AUTH_ERROR

    details = extract_connection_error_details(error)
    if details is not None:
        return ApiErrorCategory.SSL_CERT_ERROR if details.is_ssl_error else ApiErrorCategory.CONNECTION_ERROR

    if status_code is not None and status_code >= 500:
        return ApiErrorCategory.SERVER_ERROR

    if status_code is not None and status_code >= 400:
        return ApiErrorCategory.CLIENT_ERROR

    return ApiErrorCategory.UNKNOWN


def format_api_error(error: BaseException | Exception | Any) -> str:
    details = extract_connection_error_details(error)
    if details is not None:
        if details.code == "ETIMEDOUT":
            return "Request timed out. Check your internet connection and proxy settings."
        if details.is_ssl_error:
            return (
                f"Unable to connect to API: SSL error ({details.code}). "
                "Check your corporate proxy or local certificate configuration."
            )
        return f"Unable to connect to API ({details.code})."

    status_code = extract_status_code(error)
    message = extract_error_message(error)
    if message:
        return _sanitize_html_message(message)

    if status_code is not None:
        return f"API error (status {status_code})"

    return "Unknown API error"


def categorize_retryable_api_error(error: BaseException | Exception | Any) -> str:
    category = classify_api_error(error)
    if category in {ApiErrorCategory.SERVER_OVERLOAD, ApiErrorCategory.RATE_LIMIT}:
        return "rate_limit"
    if category in {ApiErrorCategory.AUTH_ERROR, ApiErrorCategory.INVALID_API_KEY, ApiErrorCategory.TOKEN_REVOKED}:
        return "authentication_failed"
    if category in {ApiErrorCategory.SERVER_ERROR, ApiErrorCategory.API_TIMEOUT, ApiErrorCategory.CONNECTION_ERROR}:
        return "server_error"
    return "unknown"


def is_retryable_api_error(error: BaseException | Exception | Any) -> bool:
    category = classify_api_error(error)
    return category in {
        ApiErrorCategory.API_TIMEOUT,
        ApiErrorCategory.RATE_LIMIT,
        ApiErrorCategory.SERVER_OVERLOAD,
        ApiErrorCategory.SERVER_ERROR,
        ApiErrorCategory.CONNECTION_ERROR,
    }


def extract_connection_error_details(error: BaseException | Exception | Any) -> ConnectionErrorDetails | None:
    current = error
    for _ in range(5):
        if current is None:
            return None

        code = getattr(current, "code", None)
        if isinstance(code, str):
            return ConnectionErrorDetails(
                code=code,
                message=str(current),
                is_ssl_error=code in SSL_ERROR_CODES,
            )

        current = getattr(current, "__cause__", None) or getattr(current, "__context__", None) or getattr(current, "cause", None)
    return None


def extract_status_code(error: BaseException | Exception | Any) -> int | None:
    for attr in ("status_code", "status"):
        value = getattr(error, attr, None)
        if isinstance(value, int):
            return value

    response = getattr(error, "response", None)
    if response is not None:
        status_code = getattr(response, "status_code", None)
        if isinstance(status_code, int):
            return status_code

    return None


def extract_error_message(error: BaseException | Exception | Any) -> str:
    message = getattr(error, "message", None)
    if isinstance(message, str) and message:
        return message

    args = getattr(error, "args", ())
    if args:
        first = args[0]
        if isinstance(first, str):
            return first

    response = getattr(error, "response", None)
    if response is not None:
        text = getattr(response, "text", None)
        if callable(text):
            try:
                value = text()
            except TypeError:
                value = None
            if isinstance(value, str) and value:
                return value
        if isinstance(text, str) and text:
            return text

    return str(error) if error else ""


def _is_timeout(error: BaseException | Exception | Any, message: str) -> bool:
    if "timeout" in message:
        return True
    if httpx is not None and isinstance(error, (httpx.TimeoutException, httpx.ReadTimeout, httpx.WriteTimeout)):
        return True
    return False


def _sanitize_html_message(message: str) -> str:
    if "<!doctype html" not in message.lower() and "<html" not in message.lower():
        return message
    lower = message.lower()
    start = lower.find("<title>")
    end = lower.find("</title>")
    if start != -1 and end != -1 and end > start:
        return message[start + 7 : end].strip()
    return "Unexpected HTML error response from API"
