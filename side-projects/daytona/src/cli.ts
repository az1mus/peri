#!/usr/bin/env bun
// ---------------------------------------------------------------------------
// cli.ts —— peri-sandbox CLI 入口
// ---------------------------------------------------------------------------
import { Command } from "commander";
import { runInit } from "./commands/init";
import { runCreate } from "./commands/create";
import { askPeri } from "./commands/ask";

const program = new Command();

program
    .name("peri-sandbox")
    .description("在 Daytona 沙箱中运行 peri AI Agent")
    .version("0.1.0");

program
    .command("init")
    .description("初始化 Daytona 连接")
    .action(async () => {
        await runInit();
    });

program
    .command("create")
    .description("创建新沙箱（交互式填表）")
    .action(async () => {
        await runCreate();
    });

program
    .command("ask")
    .description("向 peri AI Agent 发送单轮问答")
    .argument("<prompt>", "要发给 peri 的问题")
    .option("--sandbox <name>", "指定沙箱名称或 ID")
    .action(async (prompt, opts) => {
        await askPeri({
            sandbox: opts.sandbox,
            prompt,
        });
    });

program.parseAsync().catch((err) => {
    console.error("错误:", err instanceof Error ? err.message : err);
    process.exit(1);
});
