# Feature: 20260517_F001 - config-sync

## 需求背景

Perihelion 用户在多台机器上使用时，需要手动复制 settings.json、skills、MCP 配置、插件等文件来保持环境一致。缺乏一个便捷的配置同步机制，导致新环境搭建成本高、配置漂移难以管理。

## 目标

- 提供一键式的配置同步功能，sender 端申请配对码，receiver 端输入码后即可同步
- 支持选择性同步：receiver 可勾选需要同步的项（settings/skills/mcp/plugins）
- 端到端加密：配对码派生 AES-256-GCM 密钥，relay 服务端只转发密文，无法读取内容
- 单向覆盖式同步：sender → receiver，无合并逻辑

## 方案设计

### 整体架构

```
┌──────────┐     WebSocket      ┌──────────────┐     WebSocket      ┌──────────┐
│  Sender   │◄─────────────────►│  Relay Server │◄─────────────────►│ Receiver  │
│  (Client) │                   │  (Hono.js)    │                   │  (Client) │
└──────────┘                   └──────────────┘                   └──────────┘
                                     │
                              配对码映射表 (内存)
                              WS 连接管理
                              消息转发（密文透传）
```

三个组件，位于 `side-projects/peri-sync/` 下：

| 目录 | 职责 |
|------|------|
| `server/` | Relay Server，Hono.js + WebSocket，配对码管理 + 消息转发 |
| `cli/` | CLI 客户端，支持 sender/receiver 两种模式，打包/解包/加密/解密 |

Relay Server 无状态转发，不存储任何用户数据。配对码过期后自动清理。

### WebSocket 协议

所有消息均为 JSON 格式：

```typescript
interface WsMessage {
  type: string;
  payload: any;
  pairCode?: string;
}
```

**消息类型**：

| 方向 | 类型 | 说明 |
|------|------|------|
| Client → Server | `request_pair` | sender 请求生成配对码 |
| Client → Server | `join_pair` | receiver 输入配对码加入 |
| Client → Server | `sync_config` | receiver 告知 sender 要同步的项 |
| Client → Server | `data_chunk` | sender 发送加密数据块 |
| Client → Server | `transfer_complete` | sender 传输完成 |
| Server → Client | `pair_created` | 返回配对码给 sender |
| Server → Client | `pair_joined` | 通知双方配对成功 |
| Server → Client | `data_chunk` | 转发数据块给 receiver |
| Server → Client | `transfer_complete` | 转发传输完成 |
| Server → Client | `error` | 错误（码不存在/已过期/已使用） |

### E2E 加密方案

```
配对码 "482917"
    │
    ▼ PBKDF2-SHA256 (salt=pairCode, iterations=100000, keyLen=32)
    │
    ▼
AES-256-GCM Key (32 bytes)
    │
    ▼ AES-256-GCM Encrypt (随机 12 字节 IV)
    │
加密数据包 = IV(12B) + Ciphertext + AuthTag(16B)
```

- 配对码作为共享密钥种子，双方各自用 PBKDF2-SHA256 派生 AES-256 密钥
- 每次传输随机生成 12 字节 IV，保证相同明文产生不同密文
- AES-GCM 同时提供加密和完整性校验（AuthTag）
- Relay Server 只转发密文，无法解密

### 同步数据打包格式

```typescript
interface SyncPackage {
  version: 1;
  timestamp: number;    // Unix timestamp
  items: {
    settings?: {
      content: string;  // settings.json 原文 JSON
    };
    skills?: {
      files: {
        path: string;   // 相对路径，如 "skills/xxx/SKILL.md"
        content: Uint8Array;
      }[];
    };
    mcp?: {
      global?: { content: string };   // ~/.mcp.json
      project?: { content: string };  // 项目级 .mcp.json（如有）
    };
    plugins?: {
      files: {
        path: string;   // 相对路径
        content: Uint8Array;
      }[];
    };
  };
}
```

打包流程：收集文件 → 构建 SyncPackage → MessagePack 序列化 → AES-256-GCM 加密 → 分片（每片 64KB）→ WS 逐片发送。

解包流程：接收所有分片 → 合并 → AES-256-GCM 解密 → MessagePack 反序列化 → 写入文件。

### 交互流程

```
Sender                           Relay                          Receiver
  │                                │                                │
  │── request_pair ───────────────►│                                │
  │◄── pair_created("482917") ─────│                                │
  │   显示: 配对码 482917          │                                │
  │                                │◄── join_pair("482917") ────────│
  │◄── pair_joined ────────────────│─── pair_joined ───────────────►│
  │                                │                                │
  │   (等待选择)                    │                                │
  │                                │◄── sync_config({items}) ───────│
  │◄── sync_config({items}) ───────│   receiver 选择同步项 → confirm │
  │                                │                                │
  │   展示传输清单 → 打包加密        │                                │
  │── data_chunk(encrypted) ──────►│── data_chunk(encrypted) ──────►│
  │── data_chunk(encrypted) ──────►│── data_chunk(encrypted) ──────►│
  │── transfer_complete ──────────►│── transfer_complete ──────────►│
  │   ✅ 传输完成                   │              解密 → 解压 → 写入  │
  │                                │              ✅ 同步完成          │
```

### CLI 交互设计

**Sender 模式**：

```
$ peri-sync sender

Requesting pair code...
Your pair code: 482917
Waiting for receiver...

Receiver connected!

Sync items requested:
  ✓ Settings (settings.json)
  ✓ Skills (3 files)
  ✓ MCP Config (~/.mcp.json)
  ✓ Plugins (2 plugins)

Packing and encrypting...
Sending: ████████████████░░░ 87%

✅ Transfer complete!
```

**Receiver 模式**：

```
$ peri-sync receiver
Enter pair code: 482917

Connected! Select items to sync:

  [x] Settings (settings.json)
  [x] Skills (3 files in ~/.claude/skills/)
  [ ] MCP Config (~/.mcp.json)
  [x] Plugins (2 plugins)

  ↑↓ Navigate  Space Toggle  Enter Confirm

Ready to sync 3 items. Confirm? [y/N]: y

Receiving data... ████████████████░░░ 87%
Decrypting... done
Writing files... done

✅ Synced: settings.json, 3 skills, 2 plugins
```

### 项目结构

```
side-projects/peri-sync/
├── package.json              # monorepo (npm workspaces)
├── server/
│   ├── package.json
│   ├── src/
│   │   ├── index.ts          # 入口，Hono app
│   │   ├── pair-manager.ts   # 配对码生成、校验、过期清理
│   │   ├── relay.ts          # WS 连接管理 + 消息转发
│   │   └── types.ts          # 消息类型定义
│   └── tsconfig.json
├── cli/
│   ├── package.json
│   ├── src/
│   │   ├── index.ts          # CLI 入口，命令解析
│   │   ├── sender.ts         # sender 模式逻辑
│   │   ├── receiver.ts       # receiver 模式逻辑
│   │   ├── crypto.ts         # PBKDF2 + AES-256-GCM
│   │   ├── packer.ts         # 打包/解包 SyncPackage
│   │   ├── scanner.ts        # 扫描本地配置文件
│   │   ├── writer.ts         # 写入同步文件
│   │   ├── ui.ts             # CLI 交互界面（选择/进度条）
│   │   └── types.ts          # ���享类型
│   └── tsconfig.json
└── shared/
    ├── package.json
    ├── src/
    │   ├── protocol.ts       # WS 消息协议定义
    │   └── constants.ts      # 常量（分片大小、配对码长度等）
    └── tsconfig.json
```

### 配对码管理

- 格式：6 位随机数字（100000-999999）
- 有效期：5 分钟
- 一次性使用：配对成功后自动失效
- 清理：定时器每 60 秒清理过期配对码
- 存储：内存 Map，无需持久化

## 实现要点

### 关键技术决策

1. **Hono.js**：轻量、支持多运行时（Node.js / Bun / Deno），WebSocket 原生支持
2. **MessagePack**：比 JSON 更紧凑的二进制序列化，适合传输文件内容
3. **PBKDF2 + AES-256-GCM**：Node.js crypto 原生支持，无需额外依赖
4. **64KB 分片**：避免 WebSocket 大帧导致的内存压力和超时

### 难点

1. **大文件传输**：skills 目录可能较大，需分片 + 进度条 + 断点处理
2. **文件路径安全**：解包时必须校验路径无 `..` 穿越，防止恶意包覆盖系统文件
3. **并发配对码冲突**：6 位数字空间约 90 万，低并发场景足够，高并发需扩位或加前缀

### 依赖

- `hono` — HTTP 框架 + WebSocket
- `@msgpack/msgpack` — MessagePack 序列化
- `node:crypto` — PBKDF2 + AES-GCM（内置）
- `prompts` / `inquirer` — CLI 交互界面
- `cli-progress` — 进度条

## 约束一致性

本方案为独立 side-project，不修改主项目（peri-agent/peri-middlewares/peri-tui）代码。与主项目的关系：

- 同步的目标文件是主项目的配置文件（settings.json、skills 目录、.mcp.json、plugins 目录）
- 主项目无需任何改动即可感知同步结果（下次启动自动读取新配置）
- 无需与 `spec/global/constraints.md` 和 `spec/global/architecture.md` 中的 Rust 架构约束保持一致

## 验收标准

- [ ] Relay Server 可启动，支持 sender 申请配对码和 receiver 加入配对
- [ ] 配对码 6 位数字，5 分钟过期，一次性使用
- [ ] Sender 可打包 settings.json + skills + .mcp.json + plugins 为 SyncPackage
- [ ] 数据传输端到端加密（AES-256-GCM），relay 无法解密
- [ ] Receiver 可交互式选择同步项，显示进度条
- [ ] 同步完成后，receiver 端文件正确写入目标路径
- [ ] 路径穿越防护：解包时拒绝 `..` 和绝对路径
- [ ] CLI 支持 sender 和 receiver 两种模式
