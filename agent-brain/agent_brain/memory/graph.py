"""Palace graph — wing/room spatial navigation for the memory system.

Organizes memories into wings (projects/domains) and rooms (aspects/topics).
Cross-domain connections ("tunnels") are discovered when rooms appear in
multiple wings, enabling graph traversal across knowledge domains.

Inspired by MemPalace's palace_graph.py.
"""

from __future__ import annotations

from collections import defaultdict
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from .memdir import MemoryRecord, MemoryStore


class MemoryGraph:
    """Navigate the memory palace: wings, rooms, tunnels between domains."""

    def __init__(self, store: "MemoryStore") -> None:
        self.store = store

    def build_graph(self) -> dict:
        """Build wing→room→[slugs] graph from existing metadata."""
        wings: dict[str, dict[str, list[str]]] = defaultdict(lambda: defaultdict(list))

        for record in self.store.list_memories("private"):
            meta = record.metadata
            wing = meta.wing or "(ungrouped)"
            room = meta.room or meta.page_type or "(general)"
            wings[wing][room].append(meta.slug)

        return {
            wing: dict(rooms)
            for wing, rooms in sorted(wings.items())
        }

    def list_wings(self) -> list[dict]:
        """All wings with memory counts."""
        graph = self.build_graph()
        return [
            {
                "wing": wing,
                "rooms": len(rooms),
                "memories": sum(len(slugs) for slugs in rooms.values()),
            }
            for wing, rooms in graph.items()
        ]

    def list_rooms(self, wing: str) -> list[dict]:
        """All rooms in a wing with their memory slugs."""
        graph = self.build_graph()
        rooms = graph.get(wing, {})
        return [
            {"room": room, "count": len(slugs), "memories": slugs}
            for room, slugs in sorted(rooms.items())
        ]

    def find_tunnels(self, wing_a: str, wing_b: str) -> list[str]:
        """Rooms that exist in BOTH wings — cross-domain connections."""
        graph = self.build_graph()
        rooms_a = set(graph.get(wing_a, {}).keys())
        rooms_b = set(graph.get(wing_b, {}).keys())
        return sorted(rooms_a & rooms_b - {"(ungrouped)", "(general)"})

    def traverse(self, start_room: str, max_hops: int = 2) -> list[dict]:
        """BFS from a room to find connected rooms via shared wings.

        Returns rooms reachable within max_hops, with the connecting wing.
        """
        graph = self.build_graph()

        # Build room→wings mapping
        room_wings: dict[str, set[str]] = defaultdict(set)
        for wing, rooms in graph.items():
            for room in rooms:
                room_wings[room].add(wing)

        visited = {start_room}
        frontier = [(start_room, 0)]
        results: list[dict] = []

        while frontier:
            current_room, depth = frontier.pop(0)
            if depth >= max_hops:
                continue

            # Find all wings this room appears in
            for wing in room_wings.get(current_room, set()):
                # Find all other rooms in this wing
                for neighbor_room in graph.get(wing, {}):
                    if neighbor_room not in visited:
                        visited.add(neighbor_room)
                        results.append({
                            "room": neighbor_room,
                            "via_wing": wing,
                            "hops": depth + 1,
                        })
                        frontier.append((neighbor_room, depth + 1))

        return results

    def graph_stats(self) -> dict:
        """Overview stats: total wings, rooms, tunnels."""
        graph = self.build_graph()
        all_rooms: dict[str, set[str]] = defaultdict(set)
        for wing, rooms in graph.items():
            for room in rooms:
                all_rooms[room].add(wing)

        tunnel_rooms = [r for r, wings in all_rooms.items() if len(wings) > 1]

        return {
            "wings": len(graph),
            "rooms": len(all_rooms),
            "tunnels": len(tunnel_rooms),
            "tunnel_rooms": tunnel_rooms[:20],
        }
