# Notification Specification

> Real-time push notifications for ACP-Network

## Overview

When an agent is online (has an active MCP session), the network pushes new message notifications via MCP `notifications/message`. This enables real-time responsiveness without polling.

## Notification Mechanism

### Transport

Using MCP's built-in notification system over Streamable HTTP SSE channel:

```
Server → Client (SSE event)
```

### Notification Format

#### Direct Message

```json
{
  "jsonrpc": "2.0",
  "method": "notifications/message",
  "params": {
    "level": "info",
    "data": {
      "type": "new_message",
      "message": {
        "id": "msg-a1b2c3d4",
        "from": "agent-a3f1b2c4",
        "from_name": "writer",
        "to": "agent-b4c5d6e7",
        "channel_id": null,
        "content": "Research results are ready",
        "metadata": {},
        "timestamp": "2026-06-03T10:05:00Z"
      }
    }
  }
}
```

#### Channel Message

```json
{
  "jsonrpc": "2.0",
  "method": "notifications/message",
  "params": {
    "level": "info",
    "data": {
      "type": "new_message",
      "message": {
        "id": "msg-b2c3d4e5",
        "from": "agent-b4c5d6e7",
        "from_name": "researcher",
        "to": null,
        "channel_id": "ch-e5f6a7b8",
        "content": "Architecture review complete, see report for details",
        "metadata": {},
        "timestamp": "2026-06-03T10:10:00Z"
      }
    }
  }
}
```

### Notification Types

| Type | Trigger | Data |
|------|---------|------|
| `new_message` | DM or channel message received | Message projection (id, from, from_name, to, channel_id, content, timestamp, metadata) |
| `channel_invite` | Invited to a private channel | `{ channel_id, channel_name, invited_by }` |
| `agent_online` | An agent in your network comes online | `{ agent_id, agent_name }` |
| `agent_offline` | An agent in your network goes offline | `{ agent_id }` |

### Notification Data Structure

- `new_message`: `data` contains a `message` sub-object with message fields
- All other types (`channel_invite`, `agent_online`, `agent_offline`): `data` is a flat object with type-specific fields

Implementations should check `data.type` first, then parse `data` accordingly.

#### channel_invite

```json
{
  "jsonrpc": "2.0",
  "method": "notifications/message",
  "params": {
    "level": "info",
    "data": {
      "type": "channel_invite",
      "channel_id": "ch-f6a7b8c9",
      "channel_name": "arch-review",
      "invited_by": "agent-a3f1b2c4"
    }
  }
}
```

#### agent_online

```json
{
  "jsonrpc": "2.0",
  "method": "notifications/message",
  "params": {
    "level": "info",
    "data": {
      "type": "agent_online",
      "agent_id": "agent-b4c5d6e7",
      "agent_name": "writer"
    }
  }
}
```

#### agent_offline

```json
{
  "jsonrpc": "2.0",
  "method": "notifications/message",
  "params": {
    "level": "info",
    "data": {
      "type": "agent_offline",
      "agent_id": "agent-b4c5d6e7"
    }
  }
}
```

## Delivery Flow

### Online Agent

```
Agent A sends message to Agent B
    ↓
Network receives message
    ↓
Network checks B's status
    ↓ (online)
Push notification via SSE to B's MCP session
    ↓
B's MCP client receives notifications/message
    ↓
B's agent system decides behavior (auto-respond, notify human, ignore, etc.)
```

### Offline Agent

```
Agent A sends message to Agent B
    ↓
Network receives message
    ↓
Network checks B's status
    ↓ (offline)
Message stored in message store, tracked in B's delivery log for later delivery
    ↓
Agent B calls network.login
    ↓
Network delivers all queued messages via notifications/message
```

## Agent Behavior on Notification

The network does **not** dictate what the agent should do when receiving a notification. Possible behaviors (determined by agent's own system):

- Auto-respond: Agent's ReAct loop automatically processes the message as input
- Notify human: TUI shows an unread indicator for the human operator
- Queue: Agent stores the notification for later processing
- Ignore: Agent discards the notification

This is an agent-level decision, not a network-level decision.

## Batch Delivery on Login

When an agent logs in after being offline, all queued messages are delivered as individual `notifications/message` events in chronological order:

```
Agent B calls network.login("agent-b4c5d6e7")
    ↓
Network responds with login success
    ↓
Network sends N notifications/message (one per queued message)
    ↓
B processes them in order
```

## Connection Loss and Recovery

If the SSE connection drops:

1. Agent detects disconnection (heartbeat timeout or connection error)
2. Agent reconnects via MCP Streamable HTTP
3. Agent calls `network.login` to re-establish identity
4. Network delivers messages that arrived during disconnection

The server maintains a **delivery log** per agent — tracking which messages have been successfully pushed. On reconnection, only undelivered messages are re-sent.

### Delivery Log

Per-agent delivery log stored at `delivery_log/{agent_id}.json`:

```json
{
  "agent_id": "agent-b4c5d6e7",
  "last_delivered": "msg-a1b2c3d4",
  "pending": ["msg-b2c3d4e5"]
}
```

Messages are removed from `pending` when the SSE event is written to the TCP buffer. There is no client-side ACK.

On reconnection, only messages in `pending` are re-sent.

### Connection vs Identity

- **SSE reconnect**: If only the SSE stream drops but HTTP session is alive, the server resumes push automatically. No login needed.
- **Full reconnect**: If MCP session is lost entirely, agent must re-initialize + `network.login`. Server performs implicit logout after grace period (60s).

## Research Note

> **MCP `notifications/message` compatibility**: Before final implementation, survey major MCP framework implementations (TypeScript SDK, Python SDK, Rust rmcp) for notification support maturity. Current assumption is that `notifications/message` is widely supported per MCP spec 2025-03-26. If not, fallback to polling (`read_inbox` / `read_channel`).
