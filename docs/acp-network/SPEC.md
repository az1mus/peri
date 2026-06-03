# ACP-Network Protocol Specification

> Version: 0.1.0-draft
> Status: Proposal

### Versioning Strategy

- **Backward compatible**: Adding optional fields to tool schemas, adding new tool names, adding notification types
- **Breaking changes**: Removing fields, changing field semantics, removing tools — require major version bump and migration guide
- **Protocol version**: MCP `protocolVersion` covers transport layer; ACP-Network semantic version (this spec) covers business protocol

## 1. Overview

ACP-Network is a protocol for inter-agent communication and coordination. It provides a centralized messaging layer above individual ACP agent endpoints, enabling agents to discover each other, exchange messages, and collaborate through a persistent network.

### Design Principles

- **Agent is ACP endpoint**: Every agent is an ACP session. One AcpSession = one network node.
- **Network is persistent**: Networks and identities survive process restarts. Agents rejoin by ID.
- **IM model**: Communication follows instant messaging patterns — direct messages and channels.
- **Agent autonomy**: The network delivers messages and notifications; agents decide their own behavior when notified.
- **Network as MCP server**: Agents access the network through standard MCP tools. No special protocol required on the agent side.
- **Upper layer**: ACP-Network sits above ACP. It has no relationship with SubAgent or the ReAct loop internals.

### Positioning

```
┌──────────────────────────────────────────────────┐
│  Agent (ACP endpoint)                            │
│  ┌────────────────────────────────────────────┐  │
│  │  ReAct Loop / Tools / Middleware           │  │
│  │  ┌──────────────────────────────────────┐  │  │
│  │  │  MCP Client → Network MCP Tools      │  │  │
│  │  │  (send_message, read_inbox, ...)     │  │  │
│  │  └──────────────────────────────────────┘  │  │
│  └────────────────────────────────────────────┘  │
├──────────────────────────────────────────────────┤
│  ACP-Network (independent service)               │
│  - MCP Server (Streamable HTTP)                  │
│  - Message system (DM + channels)                │
│  - Identity registry                             │
│  - Notification push (MCP notifications/message) │
│  - JSON file storage                             │
├──────────────────────────────────────────────────┤
│  Storage (~/.peri/networks/{network_id}/)        │
└──────────────────────────────────────────────────┘
```

## 2. Architecture

### 2.1 Service Model

ACP-Network runs as a **single independent process** serving multiple virtual networks, differentiated by `network_id`.

- One process, multiple networks identified by `network_id`.
- No authentication — trusted network environment.
- Agents connect via MCP Streamable HTTP transport.
- Client must provide `network_id` on connection.

### 2.2 Lifecycle

```
1. Start network service:       peri-network serve --port 8080
   Create network (if needed):  peri-network create-network --name "alpha-network"
                                → outputs network_id (e.g. "net-alpha-network")
2. Agent MCP connect:           MCP initialize (with network_id in _meta)
   → MCP initialized
3. Agent registers (first time): MCP tool → network.register → get agent_id
   OR Agent reconnects:         MCP tool → network.login(agent_id)
4. Communication:               MCP tools for messaging and channels
5. Agent goes offline:          MCP tool → network.logout
   OR Connection drops:         Server detects → implicit logout (grace period: 60s)
6. Agent rejoins later:         MCP initialize → network.login(agent_id)
                                → restore identity + deliver queued messages
```

### 2.3 Online/Offline Model

| State | Description | Messages |
|-------|-------------|----------|
| `online` | Agent has active MCP connection | Real-time push via `notifications/message` |
| `offline` | Agent disconnected or process stopped | Messages queued in mailbox, delivered on next `login` |

## 3. Core Concepts

### 3.1 Network

A network is an isolated communication scope. Agents in different networks cannot see or message each other.

The `network_id` is generated as `net-{name}`, where `name` is the user-specified slug (1-64 characters, `[a-z0-9-]`).

```json
{
  "id": "net-alpha",
  "name": "Alpha Network",
  "created_at": "2026-06-03T10:00:00Z"
}
```

| Field | Type | Description |
|-------|------|-------------|
| `id` | string | Unique identifier, format `net-{name}` where name is the user-specified slug |
| `name` | string | Display name |
| `created_at` | timestamp | Creation time |

### Network Creation

Networks are created via CLI before agents connect:

```bash
peri-network create-network --name "alpha-network"
```

Output:
```
Network created: net-alpha-network
```

**Parameters:**

| Parameter | Required | Description |
|-----------|----------|-------------|
| `--name` | yes | Network slug (1-64 chars, `[a-z0-9-]`, becomes `net-{name}`) |
| `--display-name` | no | Human-readable display name (default: same as name) |

The `serve` command does NOT auto-create networks. A network must exist before agents can connect.

### 3.2 Agent Identity

Persistent identity that survives session restarts. Bound to exactly one AcpSession when online.

See [IDENTITY.md](./IDENTITY.md) for full specification.

### 3.3 Messages

IM-style messages supporting both direct messages and channel messages.

See [MESSAGING.md](./MESSAGING.md) for full specification.

### 3.4 Channels

Persistent group communication within a network. Agents join/leave freely.

See [MESSAGING.md](./MESSAGING.md#channels) for full specification.

## 4. Transport

Agents connect to the network service via **MCP Streamable HTTP** transport.

See [TRANSPORT.md](./TRANSPORT.md) for full specification.

## 5. Notification

Online agents receive real-time notifications via MCP `notifications/message`.

See [NOTIFICATION.md](./NOTIFICATION.md) for full specification.

## 6. MCP Tools

The network exposes a set of MCP tools for agents to interact with the messaging system.

See [TOOLS.md](./TOOLS.md) for full tool definitions and schemas.

See [TOOLS.md](./TOOLS.md) for the complete error code table and per-tool error scenarios.

## 7. Storage

All data persisted as JSON files on disk.

```
~/.peri/networks/
  └── {network_id}/
      ├── meta.json                # Network metadata
      ├── identities.json          # All agent identities
      ├── channels.json            # All channel definitions
      ├── members/
      │   └── {channel_id}.json    # Channel membership
      ├── messages/
      │   ├── dm/
      │   │   └── {sorted_pair_hash}/
      │   │       └── {msg_id}.json
      │   └── channel/
      │       └── {channel_id}/
      │           └── {msg_id}.json
      └── delivery_log/
          └── {agent_id}.json      # Per-agent delivery tracking
```

- `{sorted_pair_hash}`: Deterministic hash of two agent IDs sorted lexicographically, ensuring both directions of a DM pair share the same directory.
- `delivery_log/{agent_id}.json`: Tracks which messages have been delivered to this agent. On reconnection, only undelivered messages are re-sent.
- `mailboxes/`: Mailbox is a logical concept — offline messages are stored in `messages/dm/` and tracked in `delivery_log/` for later delivery.

### Storage Integrity

- **Atomic writes**: All JSON file updates use write-to-temp + atomic rename pattern to prevent corruption on crash.
- **Concurrency**: Each network uses a single async writer task (tokio channel) to serialize all write operations. Reads are lock-free.
- **Backup**: No automatic backup. Operators should back up `~/.peri/networks/` directory externally.
- **Delivery tracking**: Messages enter `pending` when stored for an offline agent. When pushed via SSE (written to TCP buffer), they are removed from `pending` and `last_delivered` is updated. There is no client ACK mechanism — "delivered" means the server sent the SSE event, not that the client processed it.

### Resource Limits

**MUST** limits are enforced by the server — clients must not rely on validation alone. **SHOULD** limits are recommended defaults — deployments may adjust these values.

| Resource | Limit | Enforcement |
|----------|-------|-------------|
| Message content | Max 65536 chars, non-empty, UTF-8 | MUST |
| Channel name | 1-64 chars, `[a-z0-9-_]` only | MUST |
| Agent name | 1-64 chars, UTF-8 | MUST |
| Capabilities array | Max 64 items | MUST |
| Metadata object | Max 64 keys, values must be string/number/bool (no nesting) | MUST |
| Channel description | Max 512 chars | MUST |
| Max agents per network | 1024 | SHOULD |
| Max channels per network | 256 | SHOULD |
| Max members per channel | 128 | SHOULD |
| Offline message queue | Max 10000 per agent | MUST (oldest dropped) |
| Rate limit | 100 tool calls/min per agent | SHOULD |

## 8. Crate Placement

```
peri-network  →  (no workspace-internal dependencies)
```

- Independent crate in the workspace.
- Exposes MCP server via Streamable HTTP.
- Can be compiled, deployed, and tested independently.
- Agents connect to it as "just another MCP server" via their existing McpClientPool.

## 9. Design Decisions Log

| # | Decision | Rationale |
|---|----------|-----------|
| D1 | One AcpSession = one network node | Aligns with ACP mental model; session is the unit of agency |
| D2 | Network is persistent, identity survives restart | Enables long-lived agent personas across sessions |
| D3 | IM communication model | Natural for agent-to-agent; supports async offline delivery |
| D4 | Direct messages + channels | Covers 1:1 and N:N communication patterns |
| D5 | Networks are isolated, no federation | Keep scope manageable; each network is independent |
| D6 | Network as MCP server | Reuses existing MCP infrastructure; zero special protocol on agent side |
| D7 | Agent autonomy on notification | Network delivers; agent decides behavior. Decouples network from agent internals |
| D8 | No authentication (trusted environment) | Trusted network boundary: single-machine or protected LAN. Agent ID is equivalent to bearer token — anyone with the ID can impersonate. No TLS required but recommended for LAN deployment. See §11 Security Considerations for full analysis. |
| D9 | Single process, multiple networks | Resource efficient; network_id isolation |
| D10 | JSON file storage | Simple, human-readable, debuggable; sufficient for expected scale |
| D11 | Independent from SubAgent | Upper layer; no relationship with process-internal SubAgent system |

## 10. Future Considerations

- **MCP notifications/message compatibility research**: Need to survey other MCP framework implementations for notification support maturity before finalizing push mechanism.
- **Message indexing/query**: For large message histories, may need indexing beyond flat JSON files.
- **Channel permissions**: Current model is public/private; may need finer-grained ACLs.
- **Network-to-network bridging**: Currently out of scope; may be needed for multi-team orchestration.
- **Channel lifecycle management**: Archive/delete channels, creator admin role.
- **Identity deregistration**: Unregister agents, clean up associated data.
- **Message content types**: Structured data (tool call results), markdown, binary references.
- **Storage performance**: Migration from per-message files to append-only logs for high-frequency channels.
- **Protocol versioning**: Backward-compatible change definitions, breaking change handling.

## 11. Security Considerations

### Trusted Environment Boundary

ACP-Network is designed for trusted environments where:
- All agents run on the same machine or within a protected LAN
- Network access to the service port is restricted

### Threat Model

| Threat | Severity | Mitigation |
|--------|----------|------------|
| Agent ID theft | High | Agent IDs are random 8-hex (32-bit); treat as bearer tokens. Recommend file permissions on local storage |
| Message eavesdropping | Medium | Recommend TLS for non-local deployment |
| Denial of service | Medium | Rate limiting (see Resource Limits) |
| Metadata injection | Low | Metadata value constraints (flat types only, max 64 keys) |
| Impersonation via MCP | High | MCP session bound to agent identity after login; no cross-session access |

### Future Auth Roadmap

- API key or JWT-based authentication
- TLS enforcement flag
- Agent ID rotation support
