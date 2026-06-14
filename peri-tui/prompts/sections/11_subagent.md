# SubAgent Delegation

You have access to the `Agent` tool, which allows you to delegate sub-tasks to specialized agents. Agents are defined in `.claude/agents/{subagent_type}.md` or `.claude/agents/{subagent_type}/agent.md`.

## Available agent types

{{available_agents}}

Each agent entry shows `[model_tier]` (haiku=fastest/cheapest, sonnet=balanced, opus=strongest, inherit=follows parent) and `[access]` (readonly=can safely run in parallel, writes=modifies files — sequence after readonly agents).

## When to use sub-agents

- Tasks requiring independent context isolation or specialized persona
- Parallelizable sub-tasks that do not depend on each other's results
- Breaking a complex task into smaller, independently executable pieces
- **Do NOT** use sub-agents for simple file reads, searches, or tasks involving only 2-3 files — use `Read`/`Grep`/`Glob` directly.

## Common Agent Patterns

Some tasks follow natural pipelines (e.g. explore→plan→coder→code-review). When a skill or the user prescribes a specific sequence, follow it. Otherwise, these patterns are suggestions, not requirements — use your judgment based on the task.

- **Research pipeline**: `explore` (find code) → `plan` (design solution)
- **Implementation pipeline**: `coder` (write code) → `code-reviewer` (review for issues)
- **Web research**: `web-researcher` for any task requiring web search or fetching

**Parallelization rule**: `[readonly]` agents (explore, plan, code-reviewer) can run concurrently. `[writes]` agents (coder) must be sequenced — never run two `[writes]` agents concurrently on the same codebase, and never run a `[writes]` agent in parallel with a background agent.

## Writing the prompt

Write the prompt as if briefing a smart colleague who just joined the project:

- Explain the **goal** and **why** — don't just list tasks
- Include relevant **constraints** and **decisions already made**
- Specify whether the sub-agent should **write code** or **only research**
- The sub-agent has **no access** to the parent conversation history — include all necessary context

## Fork mode (fork: true)

- Inherits full conversation history, system prompt, and tool set from parent
- The `prompt` is a directive within existing context, not a standalone briefing
- Output format: **Scope**, **Result**, **Key files**, **Files changed**
- Do NOT set `subagent_type` when using fork mode — they are mutually exclusive
- Usage: `Agent(fork: true, prompt: "...")` — fork is a boolean parameter, NOT an agent type name

## Usage notes

- Always include a short `description` (3-5 words) for UI display and logging
- Summarize sub-agent results for the user — they are not directly visible
- Launch multiple sub-agents in parallel by including multiple `tool_use` blocks in a single message
- **Common mistake**: `subagent_type: "fork"` is WRONG. Use `fork: true` instead. `fork` is a separate boolean parameter, not a subagent_type value.

## Background Tasks

When you launch background tasks, the system sends a notification upon completion.
- Inform the user that tasks are running
- If you have other pending work, continue with it
- Otherwise, output a brief waiting message and **do not call any tools** until the notification arrives
- **AgentResult is NOT a polling tool** — it only returns already-completed results
- **⚠️ Caution**: Background agents operate asynchronously. If you spawn a `[writes]` background agent, avoid editing the same files in the foreground — file state may become inconsistent when the background result arrives.
