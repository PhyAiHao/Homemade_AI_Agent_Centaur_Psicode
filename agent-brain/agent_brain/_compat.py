from __future__ import annotations

from copy import deepcopy

try:
    from pydantic import BaseModel, ConfigDict, Field
except ImportError:  # pragma: no cover - exercised in this workspace
    _MISSING = object()

    class _FieldInfo:
        def __init__(self, default=_MISSING, *, default_factory=None):
            self.default = default
            self.default_factory = default_factory

    def Field(default=_MISSING, *, default_factory=None, **_kwargs):
        return _FieldInfo(default=default, default_factory=default_factory)

    class ConfigDict(dict):
        pass

    class BaseModel:
        model_config = ConfigDict()

        def __init__(self, **data):
            annotations = {}
            for cls in reversed(self.__class__.mro()):
                annotations.update(getattr(cls, "__annotations__", {}))

            extra_mode = getattr(self, "model_config", {}).get("extra")
            extra_keys = [key for key in data if key not in annotations]
            if extra_mode == "forbid" and extra_keys:
                names = ", ".join(sorted(extra_keys))
                raise TypeError(f"Extra fields not permitted: {names}")

            for name in annotations:
                if name in data:
                    value = data[name]
                else:
                    value = _resolve_default(self.__class__, name)
                setattr(self, name, value)

            for key in extra_keys:
                setattr(self, key, data[key])

        def model_dump(self):
            return _dump(self.__dict__)

    def _resolve_default(cls, name):
        value = getattr(cls, name, _MISSING)
        if isinstance(value, _FieldInfo):
            if value.default_factory is not None:
                return value.default_factory()
            if value.default is not _MISSING:
                return deepcopy(value.default)
            return None
        if value is _MISSING:
            return None
        return deepcopy(value)

    def _dump(value):
        if isinstance(value, BaseModel):
            return value.model_dump()
        if isinstance(value, dict):
            return {key: _dump(item) for key, item in value.items()}
        if isinstance(value, list):
            return [_dump(item) for item in value]
        return value


__all__ = ["BaseModel", "ConfigDict", "Field"]
